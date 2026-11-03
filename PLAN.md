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
├── app.rs      state: focus, the prefix and its keys, the terminals' tabs, files, mouse, sessions
├── ui.rs       ratatui rendering: workspaces and sessions | terminal tabs | files over changes
├── keymap.rs   the prefix (cmd-p, ctrl-b), from keys.toml; `pulpo keytest` and the doctor read it
├── input.rs    typing on the bottom line, with readline keys (ctrl-u, ctrl-w, alt-backspace)
├── review.rs   Changes: what the session changed, as a tree, the selected file's diff
├── doctor.rs   `pulpo doctor` (the prefix, the hooks) and `pulpo keytest` (what a key sends)
├── workspace.rs  repo registry + per-repository state (status, tree)
├── search.rs   the search popup: the files as a tree filtered as you type (nucleo), or lines in files; the repositories to add
├── term.rs     the terminal as Pulpo shows it: a vt100 mirror; snapshots, key and mouse encoding, selections
├── server.rs   `pulpo serve`: owns every terminal (PTY + emulator + state), upgrades in place
├── client.rs   connect to the server, or start it; its messages into the event loop
├── proto.rs    the frames and messages between the two
├── pty.rs      pseudo-terminals with libc: a master fd and a pid, nothing to rebuild
├── procs.rs    process trees (`ps`), to hang up what a terminal started
├── session.rs  task sessions: clone, registry, ports, compose guess, setup and removal scripts
├── layout.rs   where you were, and what was open, across restarts and reboots
├── viewer.rs   read-only file view (syntect), and the inline diffs Changes shows
├── editor.rs   editing a file, highlighted by the viewer's own code (identical by construction)
├── theme.rs    the colors on screen (UI, syntax, terminal palette): Linear Dark, or Ghostty's theme
├── tree.rs     lazy file tree, gitignore-aware (ignore crate), and the tree of a list of paths
├── git.rs      `git status --porcelain=v2` and `--numstat` parsers
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
terminals, files and changes. A terminal belongs to the checkout it
was started in (`info.cwd`), so terminals that kept running fall back into
their session on their own; one started outside every checkout goes to
the session on screen. The app keeps the active terminal by id, remembered
per session, and the session last used in each repository. A session's and
a repository's mark is the most urgent state of their terminals.

### The screen

Three columns and a bottom line, nothing else. Left: the pulpo, then two
panels as tall as their rows: the workspaces, and the sessions of the
current one (two rows each: the name, then the branch). Middle: the session's
terminals as tabs, drawn on the top of its frame (in the order of `terms`,
which is the order they came in; the one on screen is kept by tag per
session). Right: Files over Changes, each a tree and the selected file; in
one of them, ⏎ (or a double click), or editing, gives that panel
everything right of the sidebar, the terminal included, and esc takes it
back. Only a click moves the focus; hovering or scrolling a panel doesn't.
The sidebar, the right side and the trees (one width for both) keep the
width they were dragged to while Pulpo runs. The bottom line is where you type a name, answer a
question (y), or read a message.

Keys go to what has the focus. The prefix (⌘p or ⌃b, then one key) is
taken from anywhere first: w s t f c move the focus, n and x make and
close what has it, [ ] change tabs, a goes to who waits, / opens the search
popup (over everything; a file found opens full screen), d e r
act on files, q and Q quit. < and > hide or show a side; ? shows the keybinds in a popup (any key closes it). The prefix waits 3 s for its key; a click cancels it. ⌃b twice sends one to the program. The editor
saves with ⌘s or ⌃s and stays open; esc leaves it. A panel opens wide with the
keys on its file; esc puts them on its tree (the file follows), ⏎ or tab
back on the file. Esc steps back one level: file (or editor), tree,
the three columns.

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

### Changes

Changes compares the session with where it started: a task session with the
commit it was copied at (`base`, recorded when it is made), so the
agents' commits are included; a checkout on another branch with where it
left the main one; the main branch itself with its last commit, so a
commit takes its changes away. Only a task session's base is kept: every
other read finds it again, so a commit or a push moves it. The list of files comes from `git diff --name-status`
and `ls-files --others`; only the selected file's diff is read, as the whole
file (`--unified=1000000`) with its changes inline, both sides highlighted
(reloads run on a thread: `Request::read`, then `Changes::apply`, dropping
a read that was overtaken or is another session's),
and an overview ruler down its right edge (added, removed, both; the part in
view lighter).

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
- [x] A built-in editor that looks exactly like the viewer (micro and `$PULPO_EDITOR` are gone).
- [x] `pulpo hooks install | status | remove` instead of pasting the hooks config.
- [x] Program status (OSC 7501, and OSC 9;4 progress until a program reports it): what a program says beats the heuristics.
- [x] Terminal: links (ctrl-click: URLs open, `path:line` shows in the viewer), scrollback search, copying lines from it (`v`, `y`; wrapped lines come back whole). Clipboard, OSC 52.
- [x] The review layout: the file pane follows the file the tab's agents last edited (the hooks record `tool_input.file_path`), unless you're reading or editing.
- [x] Notification (OSC 9 to Ghostty) when an agent waits for input or finishes, at most one per terminal every 10 s.
- [x] Two modes, one key (Zellij's unlock-first): in a terminal every key is the program's but ⌘p (Ghostty leaves it free, and sends it with the kitty keyboard protocol); in Pulpo, plain keys, `space` for everything (one palette), `r` to review every change in one scroll. Replaced a first try with a prefix, chords and over a hundred bindings.
- [x] Tabs and panes: each session's tabs, each a tree of splits of terminals and the file pane, kept across restarts. The sidebar in one box: the pulpo with who waits and works, the lists, and the session's tabs with the modes on (BOARD, SCROLL). A top bar came and went: the sidebar already said it all.
- [x] Go to (every terminal of every repo, by state, with what its agent asks), the next one to read, back to the last place, and a menu of every action (`picker.rs`).
- [x] Themes: following Ghostty's by default, any Ghostty theme, Omarchy's first, tried live in a picker. A Ghostty theme's background, foreground and ANSI colors give the rest (greys faded from the foreground, blue as the accent, syntax from the ANSI colors as Linear Dark's are).
- [x] Layouts (`L`: main pane left, side by side, stacked, grid), the board (every terminal of the repository, tiled), swarms (N task sessions, Claude started on a task in each once set up), scrolling back and finding in a terminal, readline keys in every prompt (`input.rs`), right-click menus, dragging pane borders.
- [x] Start again from Herdr's layout, keeping what's under it: workspaces and sessions | terminal tabs | files over changes, a prefix from anywhere, the mouse for all of it. The palette, splits, layouts, the board, swarms, the graph, the review scroll, scrollback search and the theme picker are gone. A new session is yours at once, set up in the background.
- [ ] Codex support.

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
  containers; Claude gets Ctrl-C again every half second, since it quits
  on the second), then hangs up every process group in the terminal, then
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

The copy starts from main's last commit (`git reset --hard`, `git clean`,
done before it shows), and that commit is kept as its `base`. Its first
terminal is a plain shell, yours at once. Setting up and removing are
shell scripts run in the background in their own process group, so they
survive Pulpo quitting (Q stops a setup still running); their output goes to
a log in `~/.local/state/pulpo/logs` and the session shows "setting up"
meanwhile, then "setup failed" if it did, until the session goes. A copy
left half made by quitting mid-copy is no session's, and making one by that
name again removes it first. Setup copies main's database
by streaming a dump between the two containers (`mysqldump` or
`pg_dumpall`), consistent without stopping main. Removal runs from main:
the teardown command, `docker compose down -v`, untagging, then deleting
the copy (only ever a folder inside a `.sessions` directory). Then
`pulpo forget-session` takes it off the registry. If Docker fails, nothing
more is deleted and Pulpo points at the log.
