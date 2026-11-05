---
title: Installation
description: "Install Pulpo and connect it to Claude Code, so it knows when each agent works or waits for you."
section: Get started
order: 2
---

## Requirements

- macOS, with git.
- [Ghostty](https://ghostty.org). Other terminals may work, but aren't tested.
- [Claude Code](https://claude.com/claude-code). Optional: without it, Pulpo still runs any agent, and guesses its status from the terminal's output.

## Install

1. Run the install script:

   ```sh
   curl -fsSL https://pulpo.sh/install | sh
   ```

   It builds Pulpo into `~/.cargo/bin`, which takes a minute or two. It installs Rust if you don't have it, and updates it if yours is older than 1.88. If Claude Code is installed, it also adds Pulpo's [hooks](#connect-claude-code). If the Xcode Command Line Tools are missing, the script opens their installer and stops: accept the dialog, wait for the install to finish, then run the command again.

2. Open a new terminal window, so your shell finds `pulpo`.

3. Check the install:

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

## Connect Claude Code

The hooks tell Pulpo when Claude works, waits for you or stops. They also let Pulpo resume Claude sessions after a reboot. The install script adds them; to add them yourself, run:

```sh
pulpo hooks install
```

Each hook shows as `added`, and they go in `~/.claude/settings.json`, next to any hooks of your own. Only Claude sessions started after this report to Pulpo: start a new one to see its status.

To check or remove the hooks, see the [CLI reference](/docs/cli).

## Update

```sh
pulpo update
```

It runs the install script again: it builds the latest Pulpo, updates Rust if it needs to, and refreshes the hooks. The next time you open Pulpo, its server switches to the new build. Your terminals keep running.

## Uninstall

Quit Pulpo, then run:

```sh
pulpo uninstall
```

It lists what it removes and asks before changing anything. Then it:

1. Stops every terminal, as :kp[Q] does.
2. Deletes each workspace, as deleting it in Pulpo does: its folder or worktree, its containers and its untouched branch.
3. Removes the hooks from Claude Code.
4. Deletes every folder Pulpo writes: `~/.pulpo`, the settings in `~/.config/pulpo` and the state in `~/.local/state/pulpo`. Those two follow `XDG_CONFIG_HOME` and `XDG_STATE_HOME` if you set them.
5. Uninstalls the `pulpo` command.

If a workspace can't be deleted, it stops there and leaves the rest. Fix what it shows, then run it again.

::careful
Each workspace goes with everything in it that wasn't committed. The list says which ones have changes you'd lose. Commits stay on their branches in your repository.
::
