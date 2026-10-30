# Pulpo — plan

How Pulpo is built and what comes next. For using it, see the README.

## Scope

Pulpo is a VS Code replacement in one Ghostty window: workspaces (your
repositories), terminals (shells and Claude Code), changes, files, search.
It is not a background multiplexer (yet, see "Later: terminals that keep
running"): quitting Pulpo stops everything in it, and it doesn't create or manage git worktrees (a repository
that already has some lists them under it, read only).

## Architecture

```
pulpo (one binary)
├── main.rs     terminal setup + event loop (one mpsc channel for everything)
├── app.rs      state: focus, selection, keys, mouse, terminal tabs
├── workspace.rs  repo registry + per-repository state (status, tree)
├── search.rs   search files (nucleo fuzzy matcher), search inside files, workspace picker
├── ui.rs       ratatui rendering
├── term.rs     embedded terminals: PTY (portable-pty) + emulator (vt100)
├── viewer.rs   read-only file view (syntect), inline diffs, find in file
├── theme.rs    Linear Dark colors (UI, syntax, terminal palette)
├── tree.rs     lazy file tree, gitignore-aware (ignore crate)
├── git.rs      `git status --porcelain=v2`, `--numstat` and `log --graph` parsers
└── agents.rs   optional hook-based agent status (`pulpo emit`)
```

Event sources feeding the loop:

| Source         | Mechanism                                  | Triggers                    |
|----------------|--------------------------------------------|-----------------------------|
| keyboard/mouse | crossterm reader thread                    | navigation, terminal input  |
| terminals      | one reader thread per PTY                  | emulator update, redraw     |
| files          | `notify` watcher per workspace, throttled  | tree, git, viewer refresh   |
| agents         | `notify` watcher on the event log          | agent status refresh        |
| search         | background thread per query                | search results              |

### Agent status

Works with no setup. A terminal is working while the program prints on its
own: output within 0.4 s of input (typing, pasting, clicks, scrolling,
resizing) is the program answering you, and runs shorter than 0.3 s are
redraws. When a run of at least a second ends, or the bell rings, the
terminal has something to read (magenta) until you go into it. The Claude
Code hooks (`pulpo hooks`) add what Claude is running and what it asks. Each terminal Pulpo starts gets a
`PULPO_TAB` env var; `pulpo emit` records it, so hook events map to the right tab.

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
- [ ] Terminals that keep running after quitting (see below).
- [ ] `pulpo hooks --install` instead of pasting the hooks config.
- [ ] Terminal: clipboard (OSC 52), links, scrollback search.
- [ ] Notification when an agent waits for input; Codex support.
- [ ] Read the theme from the VS Code extension instead of copying it.

## Later: terminals that keep running

Not started. The aim: quit Pulpo while Claude and builds keep running, and
reopen it where you were (same workspace, same tabs, each screen as it was).

Today the Pulpo process owns every PTY and its vt100 screen, so quitting
kills the shells. Something that outlives the window has to own each PTY.
Options considered, cheapest first:

| Option | Cost | Verdict |
|--------|------|---------|
| Save tabs and run `claude --resume <session>` on reopen | ~½ day | Brings the conversation back, but running work still stops. Worth adding on top, since it also survives a reboot. |
| Run each terminal inside a private tmux server | ~1–2 days | tmux sits between Pulpo and the program: it takes keys (prefix, Shift+Enter), breaks wheel scrollback, and hides the foreground program from tab names and the working / to-read indicator. Needs tmux installed. |
| **A small `pulpo hold` process per terminal** (like dtach) | ~2–4 days, 500–800 lines | **Chosen.** Pulpo keeps its own emulator, so tabs, the indicator, mouse and scrollback keep working. |
| One background server holding every terminal | a week or more | Every rebuild of Pulpo would mean restarting the server, and so losing the terminals. |

### How `pulpo hold` would work

- Opening a terminal starts `pulpo hold`, detached from Pulpo. It owns the
  PTY and the program, listens on a Unix socket under
  `~/.local/state/pulpo/`, and keeps the last ~1 MB of output.
- Protocol, kept tiny so holds started by an older build still talk to a
  newer one: Pulpo sends input bytes and resizes; the hold sends output
  bytes, the time of the last output, and when the program exits.
- On start, Pulpo connects to every live hold, replays its saved output
  into a new vt100 parser, then resizes it, so full-screen programs
  (Claude) redraw themselves exactly.
- A state file keeps the rest: current workspace, tab order, focus.

### Decisions

- `q` quits and leaves terminals running; `Q` stops them too. Without a
  real stop, forgotten agents keep running and spending tokens.
- What you missed while Pulpo was closed: a hold reports when its program
  last printed, so a task that ended meanwhile shows as to read.
- The tab name (the foreground program) has to come from the hold, which
  owns the PTY.
- A reboot still ends everything; the resume option above covers that case.
