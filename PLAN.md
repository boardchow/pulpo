# pulpo — plan

How pulpo is built and what comes next. For using it, see the README.

## Scope

pulpo is a VS Code replacement in one Ghostty window: workspaces (your
repositories), terminals (shells and Claude Code), changes, files, search.
It is deliberately not a background multiplexer: quitting pulpo stops
everything in it, and it doesn't create or manage git worktrees (a repository
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
├── git.rs      `git status --porcelain=v2` and `--numstat` parsers
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

Works with no setup: a terminal that printed output in the last ~1.5 s shows
as busy. For precise states (running a tool, waiting for permission, done),
install the Claude Code hooks (`pulpo hooks`). Each terminal pulpo starts gets a
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
- [x] **v3** — workspaces from anywhere: every repo pulpo was opened in is kept in
      `~/.config/pulpo/workspaces`.
- [x] **v4** — navigation: go to file, search in files, find in file.
- [x] **v5** — mnemonic keys, full-height viewer, inline and side-by-side
      diffs with added and removed counts, full-height terminal, Linear Dark
      theme.
- [ ] `pulpo hooks --install` instead of pasting the hooks config.
- [ ] Terminal: clipboard (OSC 52), links, scrollback search.
- [ ] Notification when an agent waits for input; Codex support.
- [ ] Read the theme from the VS Code extension instead of copying it.
