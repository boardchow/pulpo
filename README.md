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

Run it from anywhere: it opens your workspaces. Started inside a git repository, it adds that one and opens it; `n` in Workspaces adds others. The bottom line shows the keys that work where you are, and `?` lists them all.

| Key | Does |
| --- | --- |
| `w` `t` `c` `g` `f` | go to Workspaces, Terminals, Changes, Graph, Files |
| `n` | new shell (type `claude` in it for an agent) |
| `ctrl-s` | switch between the sidebar and the terminal |
| `/` | find a file; `tab` searches inside files |
| `q` | quit (stops everything running inside) |

To see what Claude is running and asking, add its hooks: run `pulpo hooks` and merge the output into `~/.claude/settings.json`.

## Development

```sh
make run    # build and run from the checkout
make test   # unit tests
```

[PLAN.md](PLAN.md) explains how it is built.
