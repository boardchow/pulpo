---
title: Installation
description: One command builds Pulpo and connects it to Claude Code.
section: Get started
order: 2
---

## Requirements

- **macOS**, with git. On an APFS disk (the default), the files task sessions get from `main` are copied in an instant.
- **[Ghostty](https://ghostty.org).** Pulpo is built and tested in it. Other terminals may work; Apple's Terminal can't draw the pulpo.
- **[Claude Code](https://claude.com/claude-code)**, to see what each agent is doing. Any other program runs fine, but its state only shows if it reports it (see [Agents](/docs/agents)).

## Install

```sh
curl -fsSL https://pulpo.sh/install | sh
```

The script:

1. installs the Xcode Command Line Tools if they're missing (accept the dialog, then run the script again);
2. installs Rust with rustup if there's no Rust 1.88 or newer;
3. builds Pulpo into `~/.cargo/bin` (a minute or two);
4. adds Pulpo's hooks to Claude Code, if Claude Code is installed.

Then, in a new terminal:

```sh
pulpo
```

## From a checkout

```sh
git clone https://github.com/boardchow/pulpo
cd pulpo
make install
pulpo hooks install
```

## Update

Run the install command again, or `make install` in a checkout. Your terminals keep running: the next time you open Pulpo, its server switches to the new build in place.

## Claude Code hooks

The hooks tell Pulpo when Claude works, waits for you or stops, and what it asks. They go in `~/.claude/settings.json` (or `$CLAUDE_CONFIG_DIR`), next to your own hooks, which stay untouched.

```sh
pulpo hooks install   # add them (again after installing Claude Code)
pulpo hooks status    # what's installed
pulpo hooks remove    # take them out
```

Only Claude sessions started after installing them report to Pulpo.

## Check

```sh
pulpo doctor
```

It checks that the prefix reaches Pulpo and that the hooks are installed.

## Uninstall

In Pulpo, :kp[Q] stops every terminal. Then:

```sh
pulpo hooks remove
cargo uninstall pulpo
rm -rf ~/.config/pulpo ~/.local/state/pulpo   # settings and state, if you want them gone
```

Task sessions are folders next to their repository (`<repo>.sessions/`); delete them in Pulpo first, or by hand.
