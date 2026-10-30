# pulpo

A VS Code replacement that runs in one Ghostty window. You open it, work,
and quit. Claude Code and your shells run inside it; files and diffs open
inside it. Nothing opens in another program, and nothing keeps running after
you quit: quitting pulpo stops the agents too.

## What it does

| Feature        | What it means                                                       |
|----------------|---------------------------------------------------------------------|
| **Workspaces** | Your git repositories. Open pulpo inside one and it remembers it. Each workspace is the normal folder you already work in. |
| **Terminals**  | Shells running inside pulpo, in tabs. Type `claude` in one to start an agent; each tab is named after what runs in it, and the sidebar shows which ones are busy. |
| **Changes**    | What changed in the current workspace since the last commit. Open one to see the diff, side by side when the window is wide. |
| **Files**      | The workspace's file tree. Open a file to read it, highlighted. Read only: agents do the editing. |
| **Search**     | `/` finds a file by name; tab switches to searching inside files. In the viewer, `/` searches only the file you are reading. |

Two Claude tabs in the same workspace are fine for asking questions or
reviewing; just don't let both edit code at the same time, since they share
the same folder and branch. pulpo doesn't create git worktrees.

```
┌ Workspaces ─────────┐┌ [1 claude] [2 zsh] ─────────────────────────┐
│ ● api  main ±2      ││                                             │
│   web  feat/x       ││   the selected agent or shell,              │
│ Terminals           ││   running inside pulpo                      │
│  1 ● claude api     ││                                             │
│  2 ○ zsh api        │├ src/auth.rs  +3 −1 ─────────────────────────┤
│ Changes 2           ││  11   fn login(user: &User) {                │
│  M auth.rs +3 −1    ││  12 -     check(user)?;                      │
│ Files               ││  12 +     check(user).await?;                │
│ ▾ src               ││  13   }                                      │
└─────────────────────┘└─────────────────────────────────────────────┘
```

## Install

```sh
cargo build --release
ln -sf "$PWD/target/release/pulpo" ~/.local/bin/pulpo
```

Run `pulpo` in a full Ghostty window. Opening it inside a git repository adds it
to the workspace list; from anywhere else it opens the same list. To add
another workspace, press `n` in Workspaces and pick one of the git
repositories found in your home folder.

## Keys

Letters are mnemonic: a panel by its initial; `n` (new) and `x` (remove) act
on the panel you are in. `?` in the app shows the full list, and the bottom
line always shows the keys that work where you are.

| Sidebar   | Does                                                       |
|-----------|------------------------------------------------------------|
| w t c f v | go to Workspaces, Terminals, Changes, Files, Viewer        |
| s         | into the current shell                                     |
| z         | into the current shell, full height (hides the viewer)     |
| n         | new: a workspace in Workspaces, a shell anywhere else      |
| x x       | remove the workspace from the list (files kept) / close the terminal |
| /  tab    | search files by name / tab: search inside files            |
| 1-9  [ ]  | enter terminal N / previous, next terminal                 |
| q q       | quit                                                       |

| In a terminal  | Does                                                    |
|----------------|---------------------------------------------------------|
| ctrl-s         | back to the sidebar                                     |
| ctrl-n         | new shell                                               |
| ctrl-q         | close this shell; asks again if something is running in it |
| ctrl-1-9       | switch terminal                                         |
| ctrl-f         | hide / show the viewer                                  |
| shift-⏎        | new line in a Claude prompt                             |

| In the viewer | Does                                                   |
|---------------|--------------------------------------------------------|
| ⏎ / esc       | full height or split view / back to where you were     |
| /             | find in this file                                      |
| h l  ← →  0   | scroll sideways / back to the start (also shift + wheel, trackpad swipe) |
| n / p         | next / previous match, or change when showing a diff   |

Not ctrl-space: macOS uses it to switch keyboard layouts. The mouse works
too: click to focus and select, wheel to scroll, shift-drag to select text.
In a terminal the wheel goes to the program when it wants it (Claude, vim),
moves through full-screen programs like `less`, and otherwise scrolls back
through the shell's output.

## Reading changes

The Changes panel lists modified files with their added and removed line
counts. Opening one shows the whole file, syntax highlighted; `n` and `p`
jump between changes. Deleted lines show in red where they were and added
lines in green below them. On a large screen (each side at least 85 columns
wide, e.g. a 16" laptop or an external monitor), the old version is on the
left and the new one on the right, lined up instead.

## Agent status

Without setup, a terminal that printed something in the last second or so
shows as busy. For precise states (running a tool, waiting for you, done),
add the Claude Code hooks: run `pulpo hooks` and merge its output into
`~/.claude/settings.json`.
