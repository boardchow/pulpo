---
title: CLI
description: "Every pulpo command and flag. Everything else happens in the app."
section: Reference
order: 16
---

| Command | Does |
| --- | --- |
| `pulpo [path]` | Open Pulpo. The repository at `path` joins the project list. `path` defaults to `.` |
| `pulpo hooks` | Print the hooks Pulpo adds to Claude Code, as JSON, without changing anything |
| `pulpo hooks install` | Add the hooks to Claude Code's settings |
| `pulpo hooks status` | Show whether each hook is installed |
| `pulpo hooks remove` | Remove the hooks, leaving yours |
| `pulpo doctor` | Check the prefix and the hooks |
| `pulpo keytest` | Show what Pulpo receives for each key you press |
| `pulpo --help`, `pulpo -h` | Show the commands |

Other commands you may see, like `pulpo serve`, are internal and aren't meant to be run by hand.

## pulpo [path]

Opens Pulpo and adds the repository that contains `path`. If `path` is in a worktree, Pulpo adds the repository it belongs to. If `path` is in a workspace Pulpo made, Pulpo opens that workspace and adds nothing.

In a project's folder, Pulpo opens on the workspace of that project you were on last. In a workspace's folder, it opens that workspace. Elsewhere, it opens where you were last. With no project yet, it opens the list of repositories to add.

## pulpo hooks

The hooks go in `~/.claude/settings.json`, or `$CLAUDE_CONFIG_DIR/settings.json`. They report these Claude Code events: `SessionStart`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `Notification`, `Stop` and `SessionEnd`.

| Flag | For | Does |
| --- | --- | --- |
| `--dry-run` | `install`, `remove` | Show what would change, without writing |

`install` and `remove` print each event and what changed: `added`, `updated`, `unchanged`, `removed` or `not installed`. `status` prints each event as `current`, `outdated` or `missing`. Run `pulpo hooks install` again after updating Pulpo if a hook is `outdated`.

Only Claude sessions started after installing report to Pulpo.

## pulpo doctor

Checks:

- That `keys.toml` reads, and which keys are the prefix.
- Whether macOS switches input sources on :k[⌃␣], when it's a prefix, since Pulpo would never receive it.
- That the hooks are installed.

It ends with `All good.` when nothing is wrong. A line marked `✗` says what to fix.

## pulpo keytest

Prints each key you press as Pulpo receives it, and marks the prefix. Press :k[q] to stop.
