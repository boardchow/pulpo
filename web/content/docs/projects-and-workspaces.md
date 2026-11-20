---
title: Projects and workspaces
description: "A project is a repository you work on. A workspace is a copy of it, on its own branch, for one task."
section: Concepts
order: 11
---

## Projects

A project is a git repository you work on with Pulpo. Running `pulpo` in a repository adds it, and so does making a workspace in it. Your projects come first when :kp[n] (the [prefix](/docs/prefix-key), then n) asks for a new workspace's repository.

Running `pulpo` in a project's folder with no workspace yet opens on the project's own folder, on the branch it has checked out. While terminals run there, the workspace list shows it too, marked `project`.

## Workspaces

A workspace is a copy of a project for one task: a [git worktree](#workspaces-are-worktrees) in `~/.pulpo/workspaces/<repo>/<name>`, on its own branch. Each workspace has its own:

| Its own | Means |
| --- | --- |
| Folder and branch | Agents never edit each other's files |
| Terminals | Each workspace keeps its own tabs |
| Ports | Ten ports from 20000 up, the first in `PULPO_PORT` |
| Compose project | Containers, volumes and a copy of the database, with [Docker Compose](/docs/docker) |

To create one, see [Run agents on separate branches](/docs/run-agents-in-parallel). A workspace shows its terminals, and opens its [review](/docs/review-changes) full screen.

## Workspaces are worktrees

A worktree is a second checkout of the same repository. It shares the project's `.git`, so:

- A commit made in a workspace is in the project at once, on the workspace's branch.
- A branch can be checked out in one place only. Since the project has the default branch checked out, a workspace can't switch to it.
- Stashes are shared between the project and its workspaces.
- A worktree only has the files git tracks. Pulpo copies the ignored ones a workspace needs, like `.env`: see [Set up new workspaces automatically](/docs/workspace-setup).

Pulpo locks each worktree, so `git worktree prune` leaves it alone. Worktrees made outside Pulpo, by agents or by hand, don't show in the list.

## The workspace list

Press :kp[w], or click the workspace's name on the terminal's frame. The list shows every project's workspaces, with the project's name on the right, and a last row that makes a new one. Type to filter it.

Each workspace row starts with its mark: a spinner while it works or sets up, ✓ or ✗ once its setup worked or failed, ◆ when it waits for you, ○ when it's quiet. :kp[1] to :kp[9] go to the first to ninth workspace in the list.

A workspace moves to the top when its setup ends or it starts waiting for you, and above the quiet ones when it starts working. Finished setups come first. Once you've read it, it stays where it is: rows only move up. When another workspace waits for you, the terminal's frame names it on the right, like `◆ fix-login waiting`.

Pulpo opens on the workspace you were on last. Running `pulpo` in a project's folder opens that project's last workspace.
