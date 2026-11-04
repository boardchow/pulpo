---
title: Projects and workspaces
description: A project is a repository. A workspace is one checkout of it, so agents working in parallel never touch each other's work.
section: Using Pulpo
order: 4
---

## Projects

Running `pulpo` in a repository adds it. To add another, :kp[p] then :kp[n].

Selecting a project makes it the current one and goes back to the workspace you last used in it.

:kp[x] in Projects removes a project from the list. Its files stay.

## Workspaces

A workspace is one checkout of a project. Selecting one shows its terminals, files and changes.

Workspaces lists every project's workspaces, each with its project's name on the right, so you can go from one to another without going through Projects. The ones waiting for you come first, then the ones working, then the rest. Selecting a workspace in another project makes that project the current one.

Every project starts with `main`, the repository itself. Each workspace you create is a git worktree on its own branch, with its own ports and Docker containers. Worktrees made outside Pulpo, by agents or by hand, don't show.

Each row shows the workspace's state, name, uncommitted changes (`±3`) and project.

## Create a workspace

:kp[w], then :kp[n], then a name. The workspace is made in the current project.

1. Pulpo fetches the remote's default branch and makes a worktree from it in `~/.pulpo/workspaces/<repo>/<name>`, on the branch `<you>/<name>`.
2. It copies `main`'s `.env` files, and the files listed in `.worktreeinclude`.
3. A shell opens in it right away.
4. Setup runs in the background: submodules, [Docker](/docs/docker), then the setup script. The workspace shows *setting up* meanwhile, or *setup failed* with its log.

`<you>` is the `branch_prefix` setting, else git's `user.name`, else your GitHub login.

To pick the branch yourself, say for a Linear ticket, press `Tab` after the name and type or paste it. The name stays the folder's.

Commit and push from a workspace as usual; `main` sees the commits at once.

## Copy ignored files

A worktree only has tracked files. List the ignored ones a workspace needs in `.worktreeinclude`, at the repository's root, in `.gitignore` syntax:

```text
config/master.key
certs/
!.env.local
```

Copies are APFS clones: instant, and no extra disk space.

## Delete a workspace

:kp[x] in Workspaces. Pulpo first says what you'd lose. Then it stops the terminals, runs the teardown script, and removes the containers and the worktree.

The branch stays if it has commits. Deleting the folder by hand has the same effect.

::careful
Deleting removes everything that wasn't committed in the workspace.
::

## States

| Mark | Meaning |
| --- | --- |
| spinner | A terminal is working |
| ◆ | A terminal waits for you |
| ○ | Quiet |

A workspace shows its most urgent terminal, a project its most urgent workspace. See [Agent status](/docs/terminals#agent-status).
