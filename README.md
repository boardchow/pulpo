<p align="center">
  <img src="assets/pulpo.svg" width="128" alt="Pulpo">
</p>

<h1 align="center">Pulpo</h1>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS-black?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/built%20with-Rust-black?style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/terminal-Ghostty-black?style=flat-square" alt="Ghostty">
  <img src="https://img.shields.io/badge/agent-Claude%20Code-black?style=flat-square" alt="Claude Code">
  <img src="https://img.shields.io/badge/license-MIT-black?style=flat-square" alt="MIT license">
</p>

<p align="center"><b>Many agents. One terminal.</b><br>Spin up a few coding agents, give each its own copy of your repo,<br>and see who needs you. One terminal. Have fun :)</p>

<p align="center"><a href="https://pulpo.sh">pulpo.sh</a></p>

## Hey

So here's the thing.

I was running Claude Code in like five terminal tabs. One fixing a bug, one on a feature, one writing tests.

And tbh it was chaos.

Which tab is waiting on me? Why are two of them fighting over port 3000? Who changed this file??

So I built Pulpo.

It's a terminal multiplexer for coding agents. Think tmux, but it actually knows what your agents are up to.

- Every agent gets its own copy of your repo. Nobody steps on anybody.
- You see who's working and who's waiting on you.
- Files and what each session changed sit right next to the terminals, so reviewing is easy.
- Quit whenever. The terminals keep running. Reboot, and they come back.

That's it. That's the app.

<p align="center">
  <img src="assets/screenshot.png" alt="Pulpo: workspaces and sessions on the left, a terminal in the middle, files and changes on the right (an older layout)">
</p>

## Install

```sh
curl -fsSL https://pulpo.sh/install | sh
```

One line. It installs whatever your Mac is missing (Xcode Command Line Tools, Rust), builds Pulpo and hooks it into Claude Code.

Run it again to update. Your running terminals won't even notice.

Building from a checkout? `make install`, then `pulpo hooks install` once.

## What you need

- A Mac. macOS only for now.
- [Ghostty](https://ghostty.org). It's what Pulpo is built and tested on. kitty, WezTerm and iTerm2 prob work too, no promises.
- [Claude Code](https://claude.com/claude-code). Other agents run fine inside, but Pulpo can't tell what they're doing unless they report it with [OSC 7501](https://mitchellh.com/writing/program-status-osc7501).

It's v0.1. Beta. Things will break. Tell me when they do.

## Using it

```sh
pulpo
```

Run it anywhere and you get your workspaces. Run it inside a git repo and it adds that one.

Three columns, nothing else:

- **Left:** the pulpo (who's working, who's waiting), then two small panels: your workspaces (repos), and the sessions of the one you're on, each with its branch under its name.
- **Middle:** the session's terminals, as tabs. Claude, `make run`, a shell, whatever.
- **Right:** Files over Changes. Each is a tree on the left and the file on the right.

One prefix, from anywhere, terminals included: `⌘p` (P for Pulpo) or `ctrl-b` (like Herdr and tmux). Press it, let go, then a key (within 3 seconds). `⌘p ?` shows the keybinds:

| Prefix, then | Does |
| --- | --- |
| `w` `s` `t` `f` `c` | go to workspaces, sessions, the terminal, files, changes |
| `n` | new: a workspace, a session, a terminal, a file (ending with `/`: a folder), depending on where you are |
| `x` | close it: a workspace leaves the list (files kept), a session is deleted, a terminal stops (asks first when something would be lost) |
| `[` `]` | previous, next terminal tab |
| `<` `>` | hide or show the left side, the right side (both show by default) |
| `a` | the next terminal waiting for you |
| `/` | search, over whatever is on screen: the files as a tree, typing filters it, `tab` searches inside them; a file opens full screen |
| `d` `e` `r` | in Files: delete, edit, rename |
| `q` `Q` | quit (terminals keep running), quit and stop them all |
| `?` | the keybinds, in a popup |

Everything else goes to what you're in. In a terminal that's every key, so Claude gets Esc, ctrl-f, all of it. Claude backgrounds a command with `ctrl-b`: press it twice.

In the lists and trees: arrows (or `j` `k`) move, `←` `→` fold. `⏎` on a file (or a change) opens it over everything right of the sidebar, terminal included, the arrows on the file. `esc` goes back one step: to the tree, still full screen (moving in it, the file follows; `⏎` or `tab` goes back into the file), then to the three columns. Page up and down scroll the file from either. Editing opens it the same way: `⌘s` (or `ctrl-s`) saves and stays, `esc` leaves the editor for the tree (twice with unsaved changes, which throws them away).

The mouse does all of it too. Click anything to go there (hovering changes nothing), including a row of the search popup or yes when Pulpo asks. Double-click a file to open it wide. Click `+` for a new tab. Scroll a terminal back with the wheel, drag to copy, ctrl-click a link to open it, or a path (`src/app.rs:120`, as agents print them) to see that line. Drag a border to resize: the sidebar's, the right side's, and the line between a tree and its file. A long line scrolls sideways with a trackpad swipe or shift + wheel (`←` `→` with the file open).

### Changes

Changes shows what this session did and nothing else: for a task session, everything since it was copied, commits included (agents commit as they go). On main itself, what isn't committed yet: a commit takes it away. On another branch, what the branch did since it left main. Pick a file and you see the whole file with its changes in place, with marks down its right edge for where they are (click one to go there).

### The prefix

`⌘p` needs nothing set up: Ghostty leaves it free (its own palette is `⌘⇧P`), and no program inside ever sees `⌘`. `ctrl-b` works in any terminal. Not sure a key gets through? `pulpo keytest` shows what Pulpo receives. Another one? `~/.config/pulpo/keys.toml`:

```toml
prefix = ["cmd-j", "ctrl-a"]
```

## Sandboxes

This is the good part.

`⌘p s` then `⌘p n` makes a task session: a full copy of your repo, in about a second. It's an APFS clone, so it takes no extra disk until files change. Kinda magic. You get a terminal in it right away, while Docker and the rest get set up in the background.

It has your `.env` and your installed dependencies, and it starts from your last commit: uncommitted work stays where it was. Want to keep going on it instead? Make a branch right there.

Each session is its own git repo on the same branch main was on. Make a branch in it for the task. That branch lives only in that session until you push it, and the other sessions catch up with `git fetch` once it's merged.

Start `claude` in it. Give it a job. `make run` in another tab. Do it again.

One Claude on the bug. One on the feature. One on the tests. None of them can mess with the others.

If you use Docker Compose, each session also gets its own containers, its own ports and its own copy of the database. No more port 3000 fights lol. (The setup for each repo lives in `~/.config/pulpo/repos/<repo>.toml` if you wanna tweak it.)

Besides starting a session clean, Pulpo never runs git for you. The agents do the branches and commits, you just review.

Done with a task? `⌘p x` on it in Sessions deletes the session. It tells you first if you'd lose anything (uncommitted changes, unpushed commits).

## Close it. They keep going.

Hit `⌘p q` and go do something irl. Your agents, dev servers and long builds keep running.

The terminals live in a tiny background server. It only listens on a private Unix socket, never on the network.

Open Pulpo again and you're right where you left off. Reboot your Mac and Pulpo brings your terminals back and resumes your Claude sessions. Even updating Pulpo doesn't kill them.

When you actually want everything stopped: `⌘p Q`. It does it the polite way: Ctrl-C first, so dev servers free their ports and `docker compose up` stops its containers.

## Who needs you

Spinner next to a tab, a session or a workspace: it's working.

`◆`: it's waiting for you.

Next to the pulpo, the sidebar counts them for all your repos. `⌘p a` (or a click on that count) goes straight to the one that has waited longest.

And Ghostty pings you when an agent asks something or finishes. No more clicking through tabs to find the one that's been stuck for ten minutes.

This works through Claude Code's hooks. The installer sets them up. Built from a checkout, or installed Claude Code after Pulpo? Run:

```sh
pulpo hooks install   # adds them to ~/.claude/settings.json, rest of the file untouched
pulpo hooks status    # shows what's installed
pulpo hooks remove    # takes them out
```

## Colors

Pulpo wears your Ghostty theme, so it looks like the terminal around it. No theme in Ghostty: Linear Dark, Pulpo's own.

## Editing

`⌘p e` on a file edits it right inside Pulpo, over everything right of the sidebar. Same colors as the viewer, so what you see is literally what you edit.

Normal keys: shift to select, ctrl-c / x / v, ctrl-z, ctrl-f. `⌘s` saves, `esc` leaves (twice if you'd lose changes).

## Hacking on it

```sh
make run    # build and run from the checkout
make test   # unit tests, and the server's end-to-end test
```

[PLAN.md](PLAN.md) explains how it's built and what's next. [web/](web) is [pulpo.sh](https://pulpo.sh) and the installer.

Wanna help? [CONTRIBUTING.md](CONTRIBUTING.md) has the short version.

## License

[MIT](LICENSE). Use it, fork it, ship it. Just keep the notice.
