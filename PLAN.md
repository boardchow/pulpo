# Pulpo — plan

How Pulpo is built and what comes next. For using it, see the README.

## Scope

Pulpo is a VS Code replacement in one Ghostty window: projects (your
repositories), terminals (shells and Claude Code), changes, files, search.
Terminals run in a background server, so quitting or rebuilding Pulpo
leaves them running (see "Terminals that keep running"). Git is the
agents' job; the one place Pulpo does git is making and removing
workspaces (git worktrees, see "Workspaces Pulpo makes"). Worktrees made otherwise
(agents', tools', yours) don't show: only the repository and the workspaces
Pulpo made, as in Conductor.

## Architecture

```
pulpo (one binary)
├── main.rs     terminal setup + event loop (one mpsc channel for everything)
├── app.rs      state: focus, the prefix and its keys, the terminals' tabs, files, mouse, workspaces
├── ui.rs       ratatui rendering: projects and workspaces | terminal tabs | files, changes, history
├── keymap.rs   the prefix (cmd-p, ctrl-b), from keys.toml; `pulpo keytest` and the doctor read it
├── input.rs    typing on the bottom line, with readline keys (ctrl-u, ctrl-w, alt-backspace)
├── review.rs   Changes: what the workspace changed, as a tree, the selected file's diff
├── doctor.rs   `pulpo doctor` (the prefix, the hooks) and `pulpo keytest` (what a key sends)
├── project.rs  the project list, `~/.config/pulpo/projects`
├── workspace.rs  per-checkout state (status, tree)
├── search.rs   the search popup: the files as a tree filtered as you type (nucleo), or lines in files; the repositories to add
├── term.rs     the terminal as Pulpo shows it: a vt100 mirror; snapshots, key and mouse encoding, selections
├── server.rs   `pulpo serve`: owns every terminal (PTY + emulator + state), upgrades in place
├── client.rs   connect to the server, or start it; its messages into the event loop
├── proto.rs    the frames and messages between the two
├── pty.rs      pseudo-terminals with libc: a master fd and a pid, nothing to rebuild
├── procs.rs    process trees (`ps`), to hang up what a terminal started
├── worktree.rs the workspaces Pulpo makes: worktree, includes, registry, ports, compose guess and override, setup and removal scripts
├── layout.rs   where you were, and what was open, across restarts and reboots
├── viewer.rs   read-only file view (syntect), and the inline diffs Changes shows
├── editor.rs   editing a file, highlighted by the viewer's own code (identical by construction)
├── theme.rs    the colors on screen (UI, syntax, terminal palette): Pulpo Dark, or Ghostty's theme
├── tree.rs     lazy file tree, gitignore-aware (ignore crate), and the tree of a list of paths
├── git.rs      `git status --porcelain=v2` and `--numstat` parsers, the commits History shows
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

### Workspaces

The sidebar has repositories (Projects) and every repository's checkouts
(Workspaces, each with its project's name, the ones waiting for you first):
`main` and its git worktrees, each a `Workspace`. Selecting a checkout of another project makes that project the
current one.
The current workspace decides what the right side and the panels show: its
terminals, files and changes. A terminal belongs to the checkout it
was started in (`info.cwd`), so terminals that kept running fall back into
their workspace on their own; one started outside every checkout goes to
the workspace on screen. The app keeps the active terminal by id, remembered
per workspace, and the workspace last used in each project. A workspace's and
a project's mark is the most urgent state of their terminals.

### The screen

Three columns and a bottom line, nothing else. Left: the pulpo, then two
panels as tall as their rows: the projects, and every project's
workspaces, waiting ones first. A project's and a workspace's mark come first on their row. Middle: the workspace's
terminals as tabs, drawn on the top of its frame (in the order of `terms`,
which is the order they came in; the one on screen is kept by tag per
workspace). Right: Files over Changes, each a tree and the selected file, over History; in
one of them, ⏎ (or a double click), or editing, gives that panel
everything right of the sidebar, the terminal included, and esc takes it
back. Only a click moves the focus; hovering or scrolling a panel doesn't.
The sidebar, the right side and the trees (one width for both) keep the
width they were dragged to while Pulpo runs. The bottom line is where you type a name, answer a
question (y), or read a message.

Keys go to what has the focus. The prefix (⌘p or ⌃b, then one key) is
taken from anywhere first: p w t f c h move the focus, n and x make and
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

Changes is what isn't committed: the workspace against HEAD, so a commit
takes its changes away (into History). The list of files comes from `git diff --name-status`
and `ls-files --others`; only the selected file's diff is read, as the whole
file (`--unified=1000000`) with its changes inline, both sides highlighted
(reloads run on a thread: `Request::read`, then `Changes::apply`, dropping
a read that was overtaken or is another workspace's),
and an overview ruler down its right edge (added, removed, both; the part in
view lighter).

### History

Under Changes (the right column in thirds): the commits since the workspace
started, newest first. A workspace Pulpo made starts at the commit it was
copied at (`base`, recorded when it is made), a checkout on another branch
where it left the main one; the main branch shows its last 20. A commit is
pushed when `origin/<branch>` has it; the title says pushed, how many aren't,
or not pushed, and how many origin has that HEAD doesn't. Read with Changes,
on the same thread, so a commit or a push (refs move) updates both.

⏎ or a double click opens a commit full screen, read like Changes (its
files against its parent, the first commit against the empty tree), its
hash, subject, author and age in the title. It takes the place of the
workspace's changes (`shelved`) while it is full screen; leaving it (esc,
or the focus going elsewhere) brings them back, on History. Read-only, like the rest.

## Milestones

- [x] **v0** — single repo: Files, Changes.
- [x] **v1** — agent status via `pulpo emit` + Claude Code hooks.
- [x] **v2** — everything in the app: embedded terminal tabs (Claude, shells),
      read-only file viewer, mouse support.
- [x] **v3** — projects from anywhere: every repo Pulpo was opened in is kept in
      `~/.config/pulpo/projects`.
- [x] **v4** — navigation: go to file, search in files, find in file.
- [x] **v5** — mnemonic keys, full-height viewer, inline and side-by-side
      diffs with added and removed counts, full-height terminal, Pulpo Dark
      theme.
- [x] **v6** — Graph panel (`git log --graph` of the local branches; a
      commit unfolds into its files, read like Changes), terminal states
      (working, to read, quiet).
- [x] Terminals that keep running after quitting and across rebuilds (see below).
- [x] Workspaces: a project's checkouts, each with its terminals, files, changes and graph.
- [x] New workspaces (`n s`): a copy of the checkout with its own Docker project, ports and database (see below).
- [x] Reopen where you were; after a reboot, reopen the terminals and resume their Claude sessions.
- [x] A built-in editor that looks exactly like the viewer (micro and `$PULPO_EDITOR` are gone).
- [x] `pulpo hooks install | status | remove` instead of pasting the hooks config.
- [x] Program status (OSC 7501, and OSC 9;4 progress until a program reports it): what a program says beats the heuristics.
- [x] Terminal: links (ctrl-click: URLs open, `path:line` shows in the viewer), scrollback search, copying lines from it (`v`, `y`; wrapped lines come back whole). Clipboard, OSC 52.
- [x] The review layout: the file pane follows the file the tab's agents last edited (the hooks record `tool_input.file_path`), unless you're reading or editing.
- [x] Notification (OSC 9 to Ghostty) when an agent waits for input or finishes, at most one per terminal every 10 s.
- [x] Two modes, one key (Zellij's unlock-first): in a terminal every key is the program's but ⌘p (Ghostty leaves it free, and sends it with the kitty keyboard protocol); in Pulpo, plain keys, `space` for everything (one palette), `r` to review every change in one scroll. Replaced a first try with a prefix, chords and over a hundred bindings.
- [x] Tabs and panes: each workspace's tabs, each a tree of splits of terminals and the file pane, kept across restarts. The sidebar in one box: the pulpo with who waits and works, the lists, and the workspace's tabs with the modes on (BOARD, SCROLL). A top bar came and went: the sidebar already said it all.
- [x] Go to (every terminal of every repo, by state, with what its agent asks), the next one to read, back to the last place, and a menu of every action (`picker.rs`).
- [x] Themes: following Ghostty's by default, any Ghostty theme, Omarchy's first, tried live in a picker. A Ghostty theme's background, foreground and ANSI colors give the rest (greys faded from the foreground, blue as the accent, syntax from the ANSI colors as Pulpo Dark's are).
- [x] Layouts (`L`: main pane left, side by side, stacked, grid), the board (every terminal of the repository, tiled), swarms (N workspaces, Claude started on a task in each once set up), scrolling back and finding in a terminal, readline keys in every prompt (`input.rs`), right-click menus, dragging pane borders.
- [x] Start again from Herdr's layout, keeping what's under it: projects and workspaces | terminal tabs | files over changes, a prefix from anywhere, the mouse for all of it. The palette, splits, layouts, the board, swarms, the graph, the review scroll, scrollback search and the theme picker are gone. A new workspace is yours at once, set up in the background.
- [x] Workspaces as git worktrees, the industry's pattern (Conductor, Claude Code, Cursor): a branch from the remote's default branch, main's ignored files it needs, a block of ten ports, git in containers (see "Workspaces Pulpo makes").
- [x] Workspace recipes: Conductor's settings read, an install guessed from lockfiles, a trust prompt for committed scripts (see "Workspaces Pulpo makes").
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
  start never overwrites it: the workspace on screen, the workspace last used
  in each project, the terminal last on screen in each workspace (by its
  `PULPO_TAB` tag, unique for the server's life), and every open terminal
  with the Claude session alive in it (from the hooks' log).
- **After a reboot** (or the server dying), the server comes back empty
  while the layout still lists terminals: Pulpo opens each again in its
  folder, as `$SHELL -l -c 'claude --resume <id>; exec $SHELL -l'` when
  Claude was running there (the id is checked to be only letters, digits
  and dashes), else a shell, and points the remembered tags at them. `Q`
  writes an empty list, so stopping everything on purpose stays stopped.

## Workspaces Pulpo makes

A workspace is three things, each with one job:

```
Workspace = checkout (git worktree, own branch)     agents can't touch each other's files or branch
          + runtime  (own ports, Compose project)   processes, containers, databases can't collide
          + recipe   (includes, setup, teardown)    no manual step, the same every time
```

This is the pattern Conductor, Orca, Claude Code (`--worktree`) and Cursor
share: a worktree on its own branch, the ignored files it needs copied, a
setup script, per-workspace ports. None of them isolates Docker; Pulpo does.
Workspaces were APFS copies of the whole checkout before (`Kind::Copy` in the
registry): no setup again, but commits stayed trapped in the copy until
pushed, and nobody else works that way. Those are still removed the way
they were made.

**Checkout.** `worktree::create`, in a background thread, every git call
with `GIT_TERMINAL_PROMPT=0` and in its own Unix session (`setsid`), so
neither git nor ssh can ask on Pulpo's raw-mode terminal:

1. The default branch: `origin/HEAD`, else `main` or `master` on origin.
   `git fetch origin <it>`, killed after 5 s (then as last fetched). No
   remote: main's HEAD. No commit at all: refused.
2. The branch `<user>/<name>` (the way Conductor and Orca name them),
   `user` being git's `user.name`, so the repository's own name before the
   global one (a name chosen for a repository wins); without any, gh's login
   for the remote's host (`gh config get -h <host> user`, no network), else
   git's `github.user`; slugged. Or the branch typed after the name (Tab
   in the prompt), as is if `git check-ref-format --branch` takes it. An
   existing one is reused only if it sits at the base; with commits of its
   own, refused.
3. `git worktree add --lock` into `~/.pulpo/workspaces/<repo>/<name>` (out of
   the folder the repository is in, the way Cursor and Codex keep theirs).
   The lock keeps
   `git worktree prune` and other tools' cleanups off it. Hooks
   (post-checkout, LFS) run; what they say goes to the setup log.
4. Includes, the Compose override, then the registry entry
   (`~/.local/state/pulpo/workspaces.json`, under a `flock`) with its ports.
   A failure undoes what this call made: the worktree, and the branch if it
   made it (git won't delete a branch checked out elsewhere).

Changes compares with the merge-base with the default branch (`base` is
None for worktrees), so a rebase doesn't show main's commits as the
workspace's. The file watcher sends `.git/worktrees/<id>/{HEAD,index}` to
that worktree only; refs concern everyone. The sidebar lists a workspace's
worktree once, as the workspace. A registry entry whose folder is gone
(deleted by hand) is forgotten, freeing its ports.

**Includes.** A worktree has only tracked files. Main's ignored files that
`.worktreeinclude` (Claude Code's file, gitignore syntax) and the
repository's `include` setting name are cloned in (`clonefile`, links as
links, `cp -RP` where it can't clone), on top of `.env` and `.env.*` but
`.example`, `.sample`, `.template`, always (a `!pattern` takes one away).
For a Rails app the guess adds `config/master.key` and
`config/credentials/*.key`, and `bin/rails db:prepare` as `setup`: the
copied database is at main's migrations, the code at the default
branch's. Candidates come from `git ls-files
--others --ignored --directory`, so `node_modules` isn't walked; a folder
ignored whole is looked into only when a pattern starts with it. Only
ignored files, never out of the repository. Dependencies aren't copied by
default: a `.venv` copied elsewhere still runs main's interpreter.

**Runtime.** Each workspace holds a block of ten ports from 20000:
`PULPO_PORT` to `PULPO_PORT+9` for the user (`PORT` too without a compose
file), the compose file's port variables after the block. All of it counts
as taken. The terminals' environment also has `COMPOSE_PROJECT_NAME`
(`<main project>-<name>`), `PULPO_WORKSPACE_NAME`, `PULPO_ROOT_PATH`,
`PULPO_WORKSPACE_PATH`; `Spawn` carries it to the server. The compose file
is read with a line scan (`worktree::guess`): port variables, services built
locally (their images are tagged for the workspace instead of rebuilt), the
database service and engine, and what can't be isolated. That's the first
layer of the settings (see "Recipes").

**Git in containers.** A worktree's `.git` is a file pointing at
`<repo>/.git/worktrees/<id>`, absent in a container that mounts the
workspace's folder. `docker compose --profile '*' config --format json`
(Compose's own resolution: `extends`, every profile, `COMPOSE_FILE` from
`.env`) finds the services bind-mounting the folder; an override in
`~/.local/state/pulpo/compose/<project>-<hash of the repository's path>.yaml`
mounts main's `.git` at the same path, read-only, in each, and the
workspace's `COMPOSE_FILE` is its files then the override, read again each
time a terminal starts (a compose file added since counts; a missing
override is left out rather than failing Compose). Read-only plus the lock: a container can read
history (`merge-base`, `diff`, `log`) but can't prune or rewrite the shared
repository. Checked in a container: `log`, `status`, `merge-base` work,
`worktree prune` removes nothing, a commit is refused. Docker's short
`-v src:dst:ro` form mangled that target on OrbStack; Compose's long form
(what the override uses) is fine. Makefiles passing `-f` skip the
override; the setup log says so.

**Setup and removal** are shell scripts run in the background in their own
process group, so they survive Pulpo quitting (Q stops a setup still
running); their output goes to a log in `~/.local/state/pulpo/logs`, which
starts with what creation noted, and the workspace shows "setting up"
meanwhile, then "setup failed" if it did, until the workspace goes. Setup:
submodules (a failure is said and the setup goes on), main's images tagged, main's database copied by streaming a
dump between the two containers (`mysqldump` or `pg_dumpall`, consistent
without stopping main), `docker compose up -d`, the `setup` command.
Removal, from main: the teardown command, `docker compose -p <project>
down -v` (by project, so it works without the folder), untagging, `git
worktree remove --force --force` (it's locked), the branch
Pulpo made only if still at its start commit (else kept: squash merges
never look merged to `git branch -d`), the override, then `pulpo
forget-workspace`. Only a registered worktree of the repository in
`~/.pulpo/workspaces` is ever removed. If Docker fails, nothing more is
deleted and Pulpo points at the log. Before, Pulpo says what would be
lost (uncommitted changes, commits on a detached HEAD) and which branch
stays. A workspace whose folder was deleted by hand is removed the same way,
once, as soon as the watcher sees it gone (its locked worktree would
otherwise block the name, and its containers stay).

**Known limits**, shared by every worktree tool: one `git stash` per
repository; concurrent fetches can fail with "cannot lock ref" (retry); a
branch checked out elsewhere can't be a workspace's; an agent's `git branch
-D` or `gc` affects every workspace; `COMPOSE_PROJECT_NAME` and
`COMPOSE_FILE` follow a `cd` to another project in a workspace's terminal;
cookies aren't port-scoped, so localhost logins clash between workspaces.

**Recipes.** `worktree::config(repo, checkout)` stacks layers, a later one
winning where it says something: the guess (compose
file; without one, the install the lockfiles call for: `npm ci`, `pnpm`,
`yarn`, `bun`, `bundle install`, `uv sync`; with one, installing is the
containers' business), Conductor's `.conductor/settings.toml` as the
workspace's checkout has it, `.conductor/settings.local.toml` in main, then
Pulpo's file. Of Conductor's: `scripts.setup`, `scripts.archive` (as
`teardown`), `file_include_globs` (one string, comma or line separated),
`git.branch_prefix`. Its scripts run with `CONDUCTOR_PORT`,
`_ROOT_PATH`, `_WORKSPACE_NAME`, `_WORKSPACE_PATH` exported, in Pulpo's
scripts only (in terminals, tools would think they run in Conductor).
An empty Conductor value says nothing. Pulpo's file is written the first
time with every setting commented out as found, so it never freezes a
guess or hides a later Conductor change; it's read as a `Layer` of
`Option`s, so an uncommented empty value turns a setting off (`db = ""`:
no database copy). A file written by the first worktree release (every
line set, its header "Guessed from its compose file") keeps its meaning:
its empty values say nothing. Includes add up across layers (`!pattern` takes one
away). `create` reads main's settings (includes, branch prefix: the
checkout doesn't exist yet); setup reads the workspace's checkout. Scripts
run under `set -e -o pipefail`, stricter than Conductor's shell.

**Trust.** A setup or teardown script whose winning layer is the
repository's file is run only once trusted: the workspace starts from the
remote, so anyone who can push could otherwise run code here. A file is
yours only when it's there by exactly that name, a real folder and file
(on a case-insensitive disk a committed `.Conductor/Settings.toml` opens
as `.conductor/settings.toml`; a committed link leads to a committed file;
git, asked about the name, sees nothing in either case), and `git status
--porcelain --ignored` answers `??` or `!!` for it (it says nothing for a
submodule's file); anything unsure is the repository's. Making the
workspace asks (each script, how it starts, how many lines); the question carries the exact scripts
shown, so yes trusts and runs those, recorded as (repository, exact text)
in `~/.local/state/pulpo/trusted-scripts.json` (no hash to forge), and
anything else (another key, a click elsewhere, the prefix, a newer
question) sets up without them, said in the log. The question comes on
its own, while you may be typing in the new workspace's shell, so keys go on
to the terminal instead of answering it until a second passes without
typing. A changed
script is asked again. Removal skips an untrusted one the same way. `git
status` runs with `core.ignorecase=true`, as the disk is: with it off, a
committed `.Conductor/settings.local.toml` pulled over your ignored file
would leave that file looking ignored. One gap remains, by your own hand:
untracking a committed settings file (`git rm --cached`, committed) makes
it yours.

Consent covers explicit scripts only. A guessed install (`npm ci` runs
`package.json`'s scripts, `bundle install` reads the `Gemfile`) and
Compose builds run code from the remote without asking, as in Conductor
and as after any `git pull && npm i`: asking about the text `npm ci` would
say nothing about what it runs.

### Workspaces: next

- **A workspace from an existing branch or pull request**
, like Conductor's
  ⌘⇧N.
- Dropped: a "setting up" marker for agents. A terminal's environment is
  fixed when it starts, and an agent wouldn't know to look for a file; the
  sidebar already says it.
