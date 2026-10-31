# Pulpo — plan

How Pulpo is built and what comes next. For using it, see the README.

## Scope

Pulpo is a VS Code replacement in one Ghostty window: workspaces (your
repositories), terminals (shells and Claude Code), changes, files, search.
Terminals run in a background server, so quitting or rebuilding Pulpo
leaves them running (see "Terminals that keep running"). It doesn't create
or manage git worktrees (a repository that already has some shows them as
sessions, read only); task sessions are copies made without git.

## Architecture

```
pulpo (one binary)
├── main.rs     terminal setup + event loop (one mpsc channel for everything)
├── app.rs      state: focus, selection, keys, mouse, terminal tabs
├── workspace.rs  repo registry + per-repository state (status, tree)
├── search.rs   search files (nucleo fuzzy matcher), search inside files, workspace picker
├── ui.rs       ratatui rendering
├── term.rs     the terminal as Pulpo shows it: a vt100 mirror; snapshots, key and mouse encoding
├── server.rs   `pulpo serve`: owns every terminal (PTY + emulator + state), upgrades in place
├── client.rs   connect to the server, or start it; its messages into the event loop
├── proto.rs    the frames and messages between the two
├── pty.rs      pseudo-terminals with libc: a master fd and a pid, nothing to rebuild
├── procs.rs    process trees and listening ports (`ps`, `lsof`)
├── session.rs  task sessions: clone, registry, ports, compose guess, setup and removal scripts
├── layout.rs   where you were, and what was open, across restarts and reboots
├── viewer.rs   read-only file view (syntect), inline diffs, find in file
├── editor.rs   editing a file, highlighted by the viewer's own code (identical by construction)
├── theme.rs    Linear Dark colors (UI, syntax, terminal palette)
├── tree.rs     lazy file tree, gitignore-aware (ignore crate)
├── git.rs      `git status --porcelain=v2`, `--numstat` and `log --graph` parsers
├── status.rs   program status reports (OSC 7501)
└── agents.rs   optional hook-based agent status (`pulpo emit`, `pulpo hooks`)
```

Event sources feeding the loop:

| Source         | Mechanism                                  | Triggers                    |
|----------------|--------------------------------------------|-----------------------------|
| keyboard/mouse | crossterm reader thread                    | navigation, terminal input  |
| server         | one reader thread on its socket            | terminal output, state, exit |
| files          | `notify` watcher per workspace, throttled  | tree, git, viewer refresh   |
| agents         | `notify` watcher on the event log          | agent status refresh        |
| search         | background thread per query                | search results              |

### Sessions

The sidebar has repositories (Workspaces) and, under the selected one, its
checkouts (Sessions): `main` and its git worktrees, each a `Workspace`.
The current session decides what the right side and the panels show: its
terminals, files, changes and graph. A terminal belongs to the checkout it
was started in (`info.cwd`), so terminals that kept running fall back into
their session on their own; one started outside every checkout shows in
all of them. The app keeps the active terminal by id, remembered per
session, and the session last used in each repository. A session's and a
repository's mark is the most urgent state of their terminals.

### Agent status

Works with no setup. A terminal is working while the program prints on its
own: output within 0.4 s of input (typing, pasting, clicks, scrolling,
resizing) is the program answering you, and runs shorter than 0.3 s are
redraws. When a run of at least a second ends, or the bell rings, the
terminal has something to read (magenta) until you go into it. The Claude
Code hooks (`pulpo hooks install`) say exactly when Claude waits or stops, and
what it asks, for the notification; the sidebar shows only the state. Each terminal Pulpo starts gets a
`PULPO_TAB` env var; `pulpo emit` records it, so hook events map to the right tab.
A program that reports its own status (OSC 7501, `status.rs`) is taken at its
word over both: blocked shows as waiting, working as working, and done or
error leave something to read. A new shell prompt (OSC 133 A) ends its runs.

### Diffs

A change is shown as the whole file: `git diff --unified=1000000 HEAD` gives
one hunk with every line. It is laid out twice: inline (removed lines in
place) and side by side (each run of removed lines next to the added lines
that replaced it), with row maps between the two so switching layout on
resize keeps the same place. The viewer uses side by side when each side
gets at least 85 columns.
Each side is syntax highlighted by its own highlighter, so both follow their
own version of the file.

## Milestones

- [x] **v0** — single repo: Files, Changes.
- [x] **v1** — agent status via `pulpo emit` + Claude Code hooks.
- [x] **v2** — everything in the app: embedded terminal tabs (Claude, shells),
      read-only file viewer, mouse support.
- [x] **v3** — workspaces from anywhere: every repo Pulpo was opened in is kept in
      `~/.config/pulpo/workspaces`.
- [x] **v4** — navigation: go to file, search in files, find in file.
- [x] **v5** — mnemonic keys, full-height viewer, inline and side-by-side
      diffs with added and removed counts, full-height terminal, Linear Dark
      theme.
- [x] **v6** — Graph panel (`git log --graph` of the local branches; a
      commit unfolds into its files, read like Changes), terminal states
      (working, to read, quiet).
- [x] Terminals that keep running after quitting and across rebuilds (see below).
- [x] Sessions: a repository's checkouts, each with its terminals, files, changes and graph.
- [x] Task sessions (`n s`): a copy of the checkout with its own Docker project, ports and database (see below).
- [x] Reopen where you were; after a reboot, reopen the terminals and resume their Claude sessions.
- [x] A built-in editor that looks exactly like the viewer (micro is gone; `$PULPO_EDITOR` for your own).
- [x] `pulpo hooks install | status | remove` instead of pasting the hooks config.
- [x] Program status (OSC 7501, and OSC 9;4 progress until a program reports it): what a program says beats the heuristics.
- [ ] Terminal: links, scrollback search. (Clipboard, OSC 52: done.)
- [x] Notification (OSC 9 to Ghostty) when an agent waits for input or finishes, at most one per terminal every 10 s.
- [ ] Codex support.
- [ ] Read the theme from the VS Code extension instead of copying it.

## Terminals that keep running

`pulpo serve` owns every terminal: the program on its pseudo-terminal, a
vt100 emulator that answers the program's queries, and what Pulpo shows
about it (title, foreground program, working / to read, program status).
It keeps tracking all of that while no Pulpo is open, so what finished
meanwhile shows as to read. Pulpo connects over a Unix socket
(`~/.local/state/pulpo/server/sock`, 0600 in a 0700 directory; one server
per account, held by a `flock`), and keeps a mirror emulator per terminal
for drawing and for encoding keys and the mouse.

- **Attach.** On connect the server sends each terminal's size and a
  snapshot (`term::snapshot`: its history, then `state_formatted()`, then
  its title), then its raw output. The client resizes it to its own
  window, and full-screen programs redraw. Several Pulpos can be open at
  once; the last resize wins. One that falls behind is dropped and
  reconnects.
- **Upgrade.** The server reports its program's path and when it was
  built. A Pulpo from the same path but built later (`make install`, or pulpo.sh/install) asks
  it to upgrade. The server stops its terminal readers (each waits with
  `select` on its terminal and a wake pipe, so no output is lost), writes
  every terminal down (master fd, pid, size, snapshot, status records) and
  checks that the new build can read that (`serve --check-resume`). Then
  it `execve`s `pulpo serve --resume`. The process, and so the programs'
  parent, and the open terminals stay the same. If the check or the exec
  fails, it carries on as it was. `Hello` and `Upgrade` never change, so
  any two builds can do this. Both must keep installing by rename
  (`cargo install`): overwriting the running binary in place gets it
  killed on macOS.
- **Stop.** Closing a terminal, or `Q`, sends Ctrl-C to the program in
  front and gives it up to 10 s (`docker compose up` stops its
  containers), then hangs up every process group in the terminal, then
  terminates, then kills them. The groups are taken from `ps` at once,
  since they move to launchd when the shell exits.
- **Leaving.** The server exits a minute after it last had a terminal, a
  Pulpo or something being stopped. If it dies anyway, its terminals die
  with it: Pulpo says so, with its log (`server/log`), and starts a new
  one.
- A Pulpo started inside a Pulpo terminal uses its own server
  (`server-nested`), so it doesn't show itself.

- **Where you were** is kept in `server/layout.json`, written whenever it
  changes but only after the server has listed its terminals, so a slow
  start never overwrites it: the session on screen, the session last used
  in each repository, the terminal last on screen in each session (by its
  `PULPO_TAB` tag, unique for the server's life), and every open terminal
  with the Claude session alive in it (from the hooks' log).
- **After a reboot** (or the server dying), the server comes back empty
  while the layout still lists terminals: Pulpo opens each again in its
  folder, as `$SHELL -l -c 'claude --resume <id>; exec $SHELL -l'` when
  Claude was running there (the id is checked to be only letters, digits
  and dashes), else a shell, and points the remembered tags at them. `Q`
  writes an empty list, so stopping everything on purpose stays stopped.

## Task sessions

A task session is a copy of a main checkout made with `clonefile` (APFS,
copy-on-write, the whole folder in one call) into `<repo>.sessions/<name>`,
recorded in `~/.local/state/pulpo/sessions.json` (written under a `flock`).
It is chosen over a git worktree because a worktree starts from the last
commit: `.env`, hand-made settings and dependencies would have to be set up
again. Its `.git/worktrees` is removed, since those are main's.

Isolation is the environment of its terminals: `COMPOSE_PROJECT_NAME`
(`<main project>-<name>`) and each port variable of the compose file, in
blocks of ten from 20000 (free ports, none held by another session).
`Spawn` carries that environment to the server. The compose file is read
with a line scan (`session::guess`): port variables, services built
locally (their images are tagged for the session instead of rebuilt), the
database service and engine, and what can't be isolated. The result is
written once to `~/.config/pulpo/repos/<repo>.toml` for you to adjust.

Setting up and removing are shell scripts run in a terminal, so they
survive Pulpo quitting and you see them work. Setup copies main's database
by streaming a dump between the two containers (`mysqldump` or
`pg_dumpall`), consistent without stopping main. Removal runs from main:
the teardown command, `docker compose down -v`, untagging, then deleting
the copy (only ever a folder inside a `.sessions` directory). Then
`pulpo forget-session` takes it off the registry. If Docker fails, nothing
more is deleted and the terminal stays open.
