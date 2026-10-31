<p align="center">
  <img src="assets/pulpo.svg" width="128" alt="Pulpo">
</p>

<h1 align="center">Pulpo</h1>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS-black?style=flat-square" alt="Platform">
  <img src="https://img.shields.io/badge/built%20with-Rust-black?style=flat-square" alt="Rust">
</p>

<p align="center"><b>Many arms. One brain.</b><br>You orchestrate. Your agents execute. One terminal to coordinate work, review diffs, and manage every shell.</p>

## Install

Requires [Rust](https://rustup.rs) 1.88+. Built and tested in [Ghostty](https://ghostty.org); other modern terminals (kitty, WezTerm, iTerm2) and Linux should work but are untested.

```sh
git clone https://github.com/boardchow/pulpo.git
cd pulpo
make install
```

`make install` builds Pulpo and puts `pulpo` in `~/.cargo/bin`, which rustup already adds to your `PATH`. Run it again after pulling to update.

## Usage

```sh
pulpo
```

Run it from anywhere: it opens your workspaces. Started inside a git repository, it adds that one and opens it; `n` then `w` adds others. The bottom line shows the keys that work where you are, and `?` lists them all.

| Key | Does |
| --- | --- |
| `w` `t` `c` `g` `f` | go to Workspaces, Terminals, Changes, Graph, Files |
| `n w` / `n s` | new workspace / new shell in this workspace (type `claude` in it for an agent) |
| `ctrl-n` | in a terminal: new shell in the same workspace |
| `ctrl-s` | switch between the sidebar and the terminal |
| `/` | find a file; `tab` searches inside files |
| `q` | quit; terminals keep running and come back next time |
| `Q` | quit and stop every terminal |

Terminals live in a small background process, `pulpo serve`, that Pulpo starts when it first needs it. Quitting Pulpo, or rebuilding it with `make install`, leaves Claude, dev servers and long jobs running; the next Pulpo shows them as they are. A rebuilt Pulpo hands the running terminals to its new build without restarting them.

So nothing keeps running unnoticed:

- `q` asks first and lists what keeps running, with the ports it listens on (`npm :3000`). A Docker CLI is named, since Docker's own processes hold its ports.
- `Q`, or closing a terminal, stops what runs in it the way you would: Ctrl-C to the program first, so `docker compose up` stops its containers and a dev server frees its port, then a hang-up, then a kill for what remains.
- The server listens only on a Unix socket in `~/.local/state/pulpo/server/` that your account alone can open, never on a network port. It exits by itself a minute after its last terminal and Pulpo are gone.
- Processes deliberately detached from the terminal (`docker run -d`, `nohup … &`) outlive it, as they do when you close any terminal window.

To know exactly when Claude waits for you or is done, run `pulpo hooks install` once. It adds Pulpo's hooks to `~/.claude/settings.json` and leaves the rest of the file as it was; `pulpo hooks status` shows what is installed and `pulpo hooks remove` takes them out again. When an agent asks you something or finishes while you look elsewhere, Ghostty shows a notification saying what it asks. The sidebar itself only shows each terminal working or waiting.

Any program can also say what it is doing with the [program status escape sequence](https://www.superlogical.com/rex/docs/build/program-status) (OSC 7501): Pulpo shows it working or waiting on you, and its message goes in the notification.

## Development

```sh
make run    # build and run from the checkout
make test   # unit tests, and the server's end-to-end test
```

[PLAN.md](PLAN.md) explains how it is built.
