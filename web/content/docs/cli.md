---
title: CLI
description: "Every pulpo command and flag. Everything else happens in the app."
section: Reference
order: 16
---

| Command | Does |
| --- | --- |
| `pulpo [path]` | Open Pulpo. The repository at `path` joins the project list. `path` defaults to `.` |
| `pulpo hooks [agent]` | Print the hooks Pulpo adds, as JSON, without changing anything |
| `pulpo hooks install [agent]` | Add the hooks to each agent installed, or only to `claude` or `codex` |
| `pulpo hooks status [agent]` | Show whether each hook is installed |
| `pulpo hooks remove [agent]` | Remove the hooks, leaving yours |
| `pulpo doctor` | Check the prefix and the hooks |
| `pulpo update` | Build and install the latest Pulpo |
| `pulpo uninstall` | Remove Pulpo, its workspaces and its hooks |
| `pulpo keytest` | Show what Pulpo receives for each key you press |
| `pulpo --help`, `pulpo -h` | Show the commands |

Other commands you may see, like `pulpo serve`, are internal and aren't meant to be run by hand.

## pulpo [path]

Opens Pulpo and adds the repository that contains `path`. If `path` is in a worktree, Pulpo adds the repository it belongs to. If `path` is in a workspace Pulpo made, Pulpo opens that workspace and adds nothing.

In a project's folder, Pulpo opens on the workspace of that project you were on last. In a workspace's folder, it opens that workspace. Elsewhere, it opens where you were last. With no project yet, it opens the list of repositories to add.

## pulpo hooks

Without an agent, `install` and `status` cover each agent installed and say which they skipped, and `remove` covers each agent with a settings file. An agent counts as installed when its command is on your `PATH` or its settings folder exists. Name `claude` or `codex` for that one only.

| Agent | The hooks go in | Events |
| --- | --- | --- |
| Claude Code | `~/.claude/settings.json`, or `$CLAUDE_CONFIG_DIR/settings.json` | `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Notification`, `Stop`, `SessionEnd` |
| Codex | `~/.codex/hooks.json`, or `$CODEX_HOME/hooks.json` | `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PermissionRequest`, `Interrupt`, `Stop`, `SessionEnd` |

| Flag | For | Does |
| --- | --- | --- |
| `--dry-run` | `install`, `remove` | Show what would change, without writing |

`install` and `remove` print each event and what changed: `added`, `updated`, `unchanged`, `removed` or `not installed`. `status` prints each event as `current`, `outdated` or `missing`. Run `pulpo hooks install` again after updating Pulpo if a hook is `outdated`.

Only sessions started after installing report to Pulpo. Codex also runs new hooks only once you trust them, with `/hooks` in Codex.

## pulpo doctor

Checks:

- That `keys.toml` reads, and which keys are the prefix.
- Whether macOS switches input sources on :k[⌃␣], when it's a prefix, since Pulpo would never receive it.
- That the hooks are installed, for each agent installed.

It ends with `All good.` when nothing is wrong. A line marked `✗` says what to fix.

## pulpo update

Runs the [install script](/docs/installation#install) again, so it builds the latest Pulpo into `~/.cargo/bin`, updates Rust if it's older than 1.88, and refreshes the hooks of each agent installed. The next time you open Pulpo, its server switches to the new build. Your terminals keep running.

## pulpo uninstall

Stops every terminal, deletes each workspace, removes the hooks and every folder Pulpo writes, then uninstalls the `pulpo` command. It lists all of it and asks first. Run it with Pulpo closed, from a terminal outside Pulpo. See [Uninstall](/docs/installation#uninstall).

## pulpo keytest

Prints each key you press as Pulpo receives it, and marks the prefix. Press :k[q] to stop.
