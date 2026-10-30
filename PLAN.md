# ws — a terminal workspace for Ghostty

One full-screen app in a Ghostty window that replaces VS Code for agent-driven
work. Nothing opens in another program.

```
┌ sidebar ───────────┐┌ [1 claude] [2 zsh] ────────────────────┐
│ api  main ↑2       ││                                        │
│ Agents & terminals ││   the selected agent or shell,         │
│  1 ● claude  Bash  ││   running inside ws                    │
│  2 ○ zsh           ││                                        │
│ Changes 2          │├ src/auth.rs ───────────────────────────┤
│  M auth.rs  src    ││  12  fn login(...)                     │
│ Files              ││  13      ...    (view only, live)      │
│ ▾ src              ││                                        │
└────────────────────┘└────────────────────────────────────────┘
```

## Architecture

```
ws (one binary)
├── main.rs     terminal setup + event loop (one mpsc channel for everything)
├── app.rs      state: focus, selection, keys, mouse, terminal tabs
├── workspace.rs  repo registry + per-checkout state (status, tree)
├── search.rs   go to file (nucleo fuzzy matcher), search in files
├── ui.rs       ratatui rendering
├── term.rs     embedded terminals: PTY (portable-pty) + emulator (vt100)
├── viewer.rs   read-only file view (syntect) and inline diffs
├── theme.rs    Linear Dark colors (UI, syntax, terminal palette)
├── tree.rs     lazy file tree, gitignore-aware (ignore crate)
├── git.rs      `git status --porcelain=v2` parser
└── agents.rs   optional hook-based agent status (`ws emit`)
```

Event sources feeding the loop:

| Source       | Mechanism                               | Triggers                    |
|--------------|-----------------------------------------|-----------------------------|
| keyboard/mouse | crossterm reader thread               | navigation, terminal input  |
| terminals    | one reader thread per PTY               | emulator update, redraw     |
| files        | `notify` watcher on the repo, debounced | tree, git, viewer refresh   |
| agents       | `notify` watcher on the event log       | agent status refresh        |

### Agent status

Works with no setup: a terminal that printed output in the last ~1.5 s shows
as busy. For precise states (running a tool, waiting for permission, done),
install the Claude Code hooks (`ws hooks`). Each terminal ws starts gets a
`WS_TAB` env var; `ws emit` records it, so hook events map to the right tab.

## Milestones

- [x] **v0** — single repo: header, Files, Changes.
- [x] **v1** — agent status via `ws emit` + Claude Code hooks.
- [x] **v2** — everything in the app: embedded terminal tabs (Claude, shells),
      read-only file viewer and diffs, mouse support.
- [x] **v3** — workspaces from anywhere: every repo ws was opened in is kept in
      `~/.config/ws/workspaces`, with its git worktrees. `w` creates a worktree
      on a new branch and starts Claude in it; `D` deletes it.
- [x] **v4** — navigation: `p` go to file (fuzzy), `/` search in files.
- [ ] later — per-worktree setup (copy `.env`, install deps), terminal
      scrollback search, Codex support, notification when an agent waits.

## Keys

Letters are mnemonic: a panel by its initial, `n` + a letter to create.
`?` in the app shows the full list.

| Sidebar   | Does                                               |
|-----------|----------------------------------------------------|
| w a c f v | go to Workspaces, Agents, Changes, Files, Viewer   |
| s         | enter the Shell (current terminal)                 |
| n c / n s | new Claude / new shell in the current workspace    |
| n w       | new worktree on a new branch, with Claude in it    |
| n r       | add a repository (picked from a scan of ~)         |
| p  /      | go to file / search in files (viewer: find in file, n/p) |
| 1-9  [ ]  | enter terminal N / previous, next terminal         |
| x x       | close the current terminal                         |
| D D       | delete worktree / remove repo from the list        |
| q q       | quit                                               |

| In a terminal  | Does                                          |
|----------------|-----------------------------------------------|
| ctrl-s         | back to the sidebar                           |
| ctrl-n         | new… (then c, s, w or r)                      |
| ctrl-q ctrl-q  | close this terminal                           |
| ctrl-1-9       | switch terminal                               |
| shift-⏎        | new line in a Claude prompt                   |

Not ctrl-space: macOS uses it to switch keyboard layouts. The mouse works
too: click to focus and select, wheel to scroll, shift-drag to select text.

## Setup

```sh
cargo build --release
ln -sf "$PWD/target/release/ws" ~/.local/bin/ws
ws              # from inside a repo, in a full Ghostty window
ws hooks        # optional: merge into ~/.claude/settings.json
```
