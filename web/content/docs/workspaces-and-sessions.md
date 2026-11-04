---
title: Workspaces and sessions
description: A workspace is a repository; a session is one checkout of it, with its own terminals, files and changes.
section: Using Pulpo
order: 5
---

## Workspaces

Opening `pulpo` inside a repository adds it. To add another, :kp[w] then :kp[n]: pick from the git repositories in your home folder, or type a path.

:kp[x] in Workspaces takes a repository off the list. Its files stay.

## Sessions

The current workspace's sessions:

| Session | What it is |
| --- | --- |
| `main` | The repository itself. |
| A git worktree | Shown if the repository has worktrees (an agent's, say). Pulpo leaves them to git. |
| A task session | A worktree Pulpo made for one task, with its own ports and containers. See [Task sessions](/docs/task-sessions). |

Each row shows the session's state, its name, its number of uncommitted changes (`±3`), and *setting up* / *setup failed* while its setup runs or after it fails.

Selecting a session (click, or arrows in Sessions) puts its terminals, files and changes on screen. Pulpo remembers the last session of each workspace.

## States

| Mark | Meaning |
| --- | --- |
| spinner | A terminal is working. |
| ◆ | A terminal waits for you, or finished something you haven't seen. |
| ○ | Quiet. |

A session shows its most urgent terminal; a workspace its most urgent session. See [Agents](/docs/agents).
