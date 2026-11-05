---
title: Projects and workspaces
description: "A project is a repository you work on. A workspace is a copy of it, on its own branch, for one task."
section: Concepts
order: 11
---

## Projects

A project is a git repository you added to Pulpo. Running `pulpo` in a repository adds it, and so does :kp[p], then :kp[n].

Selecting a project shows the repository's own folder: its terminals, files and changes. Use it for quick work on the branch it has checked out, to pull, or to run the app. The branch shows in the bottom right corner.

:kp[x] in Projects takes a project off the list. Its files stay where they are.

## Workspaces

A workspace is a copy of a project for one task: a [git worktree](#workspaces-are-worktrees) in `~/.pulpo/workspaces/<repo>/<name>`, on its own branch. Each workspace has its own:

| Its own | Means |
| --- | --- |
| Folder and branch | Agents never edit each other's files |
| Terminals | Each workspace keeps its own tabs |
| Ports | Ten ports from 20000 up, the first in `PULPO_PORT` |
| Compose project | Containers, volumes and a copy of the database, with [Docker Compose](/docs/docker) |

To create one, see [Run agents on separate branches](/docs/run-agents-in-parallel). Selecting a workspace shows its terminals, files and changes.

## Workspaces are worktrees

A worktree is a second checkout of the same repository. It shares the project's `.git`, so:

- A commit made in a workspace is in the project at once, on the workspace's branch.
- A branch can be checked out in one place only. Since the project has the default branch checked out, a workspace can't switch to it.
- Stashes are shared between the project and its workspaces.
- A worktree only has the files git tracks. Pulpo copies the ignored ones a workspace needs, like `.env`: see [Set up new workspaces automatically](/docs/workspace-setup).

Pulpo locks each worktree, so `git worktree prune` leaves it alone. Worktrees made outside Pulpo, by agents or by hand, don't show in the list.

## The two lists

Projects lists your repositories by name. Workspaces lists the workspaces of every project, with the project's name on the right, so you go from one to another without going through Projects.

Each workspace row shows:

| Part | Shows |
| --- | --- |
| Number | :kp[1] to :kp[9] go to that workspace |
| Mark | Spinner: working. ◆: waits for you. ○: quiet |
| `±3` | Files changed and not committed |
| *setting up*, *✗ setup failed* | Its setup, while it runs or if it fails |

A workspace moves to the top when it starts waiting for you, and above the quiet ones when it starts working. Once you've read it, it stays where it is: rows only move up.

| To | Press |
| --- | --- |
| Move | :k[↑] :k[↓] or :k[j] :k[k]; past the end of one list into the other |
| Go to the top, the bottom | :k[g], :k[G] |
| Go from Projects to Workspaces, and back | :k[→], :k[←] |
| Go to the terminal | :k[⏎], :k[→] in Workspaces, or double-click a workspace |

Pulpo opens on the workspace you were on last. Running `pulpo` in a project's folder opens that project's last workspace.
