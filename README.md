<p align="center">
  <img src="assets/pulpo.svg" width="128" alt="Pulpo">
</p>

<h1 align="center">Pulpo</h1>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS-black?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/built%20with-Rust-black?style=flat-square" alt="Rust">
  <img src="https://img.shields.io/badge/terminal-Ghostty-black?style=flat-square" alt="Ghostty">
  <img src="https://img.shields.io/badge/agent-Claude%20Code-black?style=flat-square" alt="Claude Code">
</p>

<p align="center"><b>Many arms. One brain.</b><br>You orchestrate. Your agents execute. One terminal to coordinate work, review diffs, and manage every shell.</p>

<p align="center"><a href="https://pulpo.sh">pulpo.sh</a></p>

## Install

```sh
curl -fsSL https://pulpo.sh/install | sh
```

On a Mac, that sets up everything: the Xcode Command Line Tools if they are missing, [Rust](https://rustup.rs) 1.88+ through rustup, then it builds Pulpo into `~/.cargo/bin` and adds its hooks to Claude Code. Run it again to update; terminals that are running keep running across the update. The script is [web/install](web/install).

From a checkout instead: `make install`, then `pulpo hooks install` once.

### What it works with

- **Terminal:** a modern one. Pulpo is built and tested in [Ghostty](https://ghostty.org), which is the one to use; kitty, WezTerm and iTerm2 should work but are untested. Apple's Terminal lacks some of the characters it draws.
- **Agent:** [Claude Code](https://claude.com/claude-code), for now. It is the agent Pulpo knows: its hooks say when Claude works, waits for you or is done, and Pulpo resumes its sessions after a reboot. Other agents run fine in Pulpo's terminals, but it can only tell what they are doing if they report it (see [OSC 7501](#claude-code-and-other-programs) below).
- **System:** macOS. Linux should work but is untested.

## Usage

```sh
pulpo
```

Run it from anywhere: it opens your workspaces. Started inside a git repository, it adds that one and opens it; `n` then `w` adds others. The bottom line shows the keys that work where you are, and `?` lists them all.

| Key | Does |
| --- | --- |
| `w` `t` `c` `g` `f` | go to Workspaces, Sessions, Changes, Graph, Files |
| `n s` | new session, with its own copy, Docker, ports and database |
| `n t` / `n w` | new terminal in this session (type `claude` in it for an agent) / new workspace |
| `ctrl-n` | in a terminal: new shell in the same session |
| `ctrl-[` `ctrl-]` | in a terminal: previous / next terminal of the session |
| `ctrl-s` | switch between the sidebar and the terminal |
| `/` | find a file; `tab` searches inside files |
| `e` | edit the selected file, right here: it looks exactly as in the viewer |
| `q` | quit; terminals keep running and come back next time |
| `Q` | quit and stop every terminal |

Workspaces lists your repositories. Sessions lists the checkouts of the selected one: `main`, the repository itself, then its git worktrees. Picking a session brings up everything of it: its terminals (a session can hold several, say `claude`, `make run` and a shell, shown as tabs above the terminal), its files, changes and graph. Each session comes back on the terminal you left it on, and a mark beside each session and repository shows when something in it works (spinner) or waits for you (◆).

### Task sessions

To work on several tasks at once, each with its own agent, `n s` makes a new session: a full copy of the repository's checkout, next to it in `<repo>.sessions/<name>`. It is an APFS clone, so it takes a second and no extra disk until files change, and it is exact: uncommitted work, `.env` and other settings you made by hand, installed dependencies. Pulpo runs no git command for it; the copy has main's history, and the agent makes its branch there.

If the repository has a Docker Compose file, the session gets its own Compose project and its own value for each port variable the file uses (`${DB_PORT:-3306}`), from 20000 up, so its containers and database never meet main's or another session's. Its first terminal sets it up while you watch:

1. It reuses main's built images instead of building them again.
2. It copies main's database into its own (MySQL or PostgreSQL), or prepares a fresh one when main's isn't running.
3. It starts its containers, then becomes the session's shell.

Every terminal of the session gets those variables (`COMPOSE_PROJECT_NAME`, the ports), so `docker compose` there always means the session's. The ports of its app show beside it in Sessions.

How sessions of a repository are set up is guessed from its compose file the first time, into `~/.config/pulpo/repos/<repo>.toml`, which you can edit (extra setup and teardown commands, the command for a fresh database). The setup says what sessions can't keep apart: a fixed host port (`3000:3000`), a fixed container name, a volume with a fixed name.

`x x` on a session in Sessions removes it. It first says what would be lost (uncommitted changes, commits not pushed). Then it stops its terminals, removes its containers and volumes (its database) with the image tags made for it, and deletes the copy. Main and other sessions are left alone. What lives outside the copy and Docker is shared by all sessions: remote services, `~/.config` credentials, global caches. Claude asks to trust each new session's folder once.

Terminals live in a small background process, `pulpo serve`, that Pulpo starts when it first needs it. Quitting Pulpo, or updating it, leaves Claude, dev servers and long jobs running; the next Pulpo shows them as they are. A rebuilt Pulpo hands the running terminals to its new build without restarting them.

So nothing keeps running unnoticed:

- Pulpo reopens where you were: the session you had on screen (or, started inside a repository, the session you last used in it), and in each session the terminal you left it on.
- After a reboot the server starts with nothing in it; Pulpo opens your terminals again, each in its session, and resumes the Claude sessions that were running in them (`claude --resume`), then leaves a shell when Claude exits. `Q` means you stopped them on purpose: nothing comes back after it.
- `q` asks first and lists what keeps running, with the ports it listens on (`npm :3000`). A Docker CLI is named, since Docker's own processes hold its ports.
- `Q`, or closing a terminal, stops what runs in it the way you would: Ctrl-C to the program first, so `docker compose up` stops its containers and a dev server frees its port, then a hang-up, then a kill for what remains.
- The server listens only on a Unix socket in `~/.local/state/pulpo/server/` that your account alone can open, never on a network port. It exits by itself a minute after its last terminal and Pulpo are gone.
- Processes deliberately detached from the terminal (`docker run -d`, `nohup … &`) outlive it, as they do when you close any terminal window.

### Claude Code and other programs

To know exactly when Claude waits for you or is done, Pulpo uses Claude Code's hooks. The install script adds them; from a checkout, or if you installed Claude Code after Pulpo, run `pulpo hooks install` once. It adds Pulpo's hooks to `~/.claude/settings.json` and leaves the rest of the file as it was; `pulpo hooks status` shows what is installed and `pulpo hooks remove` takes them out again. When an agent asks you something or finishes while you look elsewhere, Ghostty shows a notification saying what it asks. The sidebar itself only shows each terminal working or waiting.

Any program can also say what it is doing with the [program status escape sequence](https://www.superlogical.com/rex/docs/build/program-status) (OSC 7501): Pulpo shows it working or waiting on you, and its message goes in the notification.

### Editing

`e` opens the file in Pulpo's own editor, on the right, colored exactly as the viewer shows it: the same syntax definitions, theme and highlighter. The keys are the usual ones: arrows, shift to select, alt or ctrl + arrows by word, ctrl-c / x / v (a whole line without a selection), ctrl-z / y, tab and shift-tab to indent, ctrl-f to find, ctrl-s to save, esc to close (it asks again if something isn't saved). When an agent changes the file while it is open and you haven't changed it, the editor takes the new version. To use your own terminal editor instead, set `PULPO_EDITOR` (for example `PULPO_EDITOR=nvim`).

## Development

```sh
make run    # build and run from the checkout
make test   # unit tests, and the server's end-to-end test
```

[PLAN.md](PLAN.md) explains how it is built. [web/](web) is [pulpo.sh](https://pulpo.sh): one static page and the install script, served as they are.
