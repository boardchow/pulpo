---
title: Installation
description: "Install Pulpo and connect it to your agents, so it knows when each one works or waits for you."
section: Get started
order: 2
---

## Requirements

- macOS, with git.
- Any terminal app.
- [Claude Code](https://claude.com/claude-code) or [Codex](https://developers.openai.com/codex). Optional: without them, Pulpo still runs any agent, and guesses its status from the terminal's output.

## Install

Run the install script:

```sh
curl -fsSL https://pulpo.sh/install | sh
```

It downloads the latest Pulpo into `~/.local/bin`. If Claude Code or Codex is installed, it also adds Pulpo's [hooks](#connect-your-agents) to it.

Then open a new terminal window, so your shell finds `pulpo`, and check the install:

```sh
pulpo doctor
```

The last line says `All good.` If something is missing, the line marked `✗` says how to fix it.

### From source

With Rust 1.88 or later:

```sh
git clone https://github.com/boardchow/pulpo
cd pulpo
make install
pulpo hooks install
```

## Connect your agents

The [hooks](/docs/agents) tell Pulpo when an agent works, waits for you or stops. They also let Pulpo resume agent sessions after a reboot. The install script adds them to each agent it finds; to add them yourself, run:

```sh
pulpo hooks install
```

It adds them to Claude Code and Codex, whichever you have, and says which it skipped. Each hook shows as `added`. To add them to one agent only, name it, as below.

Other agents run in Pulpo too, without hooks: Pulpo guesses their status from what they print.

### Claude Code

```sh
pulpo hooks install claude
```

The hooks go in `~/.claude/settings.json`, next to any hooks of your own. Only Claude sessions started after this report to Pulpo: start a new one to see its status.

### Codex

```sh
pulpo hooks install codex
```

The hooks go in `~/.codex/hooks.json`, next to any hooks of your own. Codex runs new hooks only once you trust them: start Codex, type `/hooks`, and trust Pulpo's. Only Codex sessions started after this report to Pulpo.

To check or remove the hooks, see the [CLI reference](/docs/cli).

## Update

```sh
pulpo update
```

It runs the install script again: it downloads the latest Pulpo and refreshes the hooks. The next time you open Pulpo, its server switches to the new build. Your terminals keep running.

## Uninstall

Quit Pulpo, then run:

```sh
pulpo uninstall
```

It lists what it removes and asks before changing anything. Then it:

1. Stops every terminal, as :kp[Q] does.
2. Deletes each workspace, as deleting it in Pulpo does: its folder or worktree, its containers and its untouched branch.
3. Removes the hooks from Claude Code and Codex.
4. Deletes every folder Pulpo writes: `~/.pulpo`, the settings in `~/.config/pulpo` and the state in `~/.local/state/pulpo`. Those two follow `XDG_CONFIG_HOME` and `XDG_STATE_HOME` if you set them.
5. Uninstalls the `pulpo` command.

If a workspace can't be deleted, it stops there and leaves the rest. Fix what it shows, then run it again.

::careful
Each workspace goes with everything in it that wasn't committed. The list says which ones have changes you'd lose. Commits stay on their branches in your repository.
::
