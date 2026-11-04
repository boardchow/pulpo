---
title: Installation
description: One command builds Pulpo and connects it to Claude Code.
section: Get started
order: 2
---

## Requirements

- macOS, with git.
- [Ghostty](https://ghostty.org). Other terminals may work, but aren't tested.
- [Claude Code](https://claude.com/claude-code), to see what each agent is doing. Optional.

## Install

```sh
curl -fsSL https://pulpo.sh/install | sh
```

The script installs the Xcode Command Line Tools and Rust if they're missing, builds Pulpo into `~/.cargo/bin`, and adds its hooks to Claude Code if it's installed.

From a checkout instead:

```sh
git clone https://github.com/boardchow/pulpo
cd pulpo
make install
pulpo hooks install
```

Check the result with `pulpo doctor`.

## Claude Code hooks

The hooks tell Pulpo when Claude works, waits or stops. They go in `~/.claude/settings.json`, next to your own hooks.

```sh
pulpo hooks install
pulpo hooks status
pulpo hooks remove
```

Only Claude sessions started after installing them report to Pulpo.

## Update

Run the install command again. Running terminals keep running: the server switches to the new build in place.

## Uninstall

Press :kp[Q] in Pulpo to stop every terminal, then:

```sh
pulpo hooks remove
cargo uninstall pulpo
rm -rf ~/.config/pulpo ~/.local/state/pulpo
```

Delete workspaces in Pulpo first, or remove `~/.pulpo/workspaces/` by hand.
