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
| A git worktree | Shown if the repository has worktrees. Pulpo doesn't create or delete them. |
| A task session | A copy of the repository for one task. See [Task sessions](/docs/task-sessions). |

Each row shows the session's state, its name, its number of uncommitted changes (`±3`), the web port of a task session (`:20000`), and under the name its branch, or *setting up* / *setup failed*.

Selecting a session (click, or arrows in Sessions) puts its terminals, files and changes on screen. Pulpo remembers the last session of each workspace.

## States

| Mark | Meaning |
| --- | --- |
| spinner | A terminal is working. |
| ◆ | A terminal waits for you, or finished something you haven't seen. |
| ○ | Quiet. |

A session shows its most urgent terminal; a workspace its most urgent session. See [Agents](/docs/agents).
