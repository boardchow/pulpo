---
title: Introduction
description: Pulpo runs several coding agents side by side on your Mac, each in its own copy of your repository.
section: Get started
order: 1
---

Each copy is a [workspace](/docs/projects-and-workspaces): a git worktree on its own branch, so no agent touches another's files.

## Who it's for

Developers who give coding agents more than one task at a time. Pulpo runs any agent that works in a terminal, and shows exactly what [Claude Code and Codex](/docs/agent-status) are doing.

## Why not tmux and worktrees by hand

| To | With tmux | With Pulpo |
| --- | --- | --- |
| Start a task | Make a branch and a worktree, copy `.env`, open a pane | Two keys and a name |
| Run the app in each task | Change ports by hand | Each workspace gets its own ports, and with Docker Compose its own containers and database |
| See who needs you | Check every pane | A ◆ marks the agent that waits, and one key goes to it |
| Review the work | Run `git diff` | Changes shows each edited file whole, with its changes in place |
| Restart your Mac | Start every session again | Terminals reopen and agent sessions resume |

Pulpo never commits, pushes or merges. Your agents, or you, do that in the terminal.
