---
title: Workspaces and sessions
description: A workspace is a repository. A session is one checkout of it, so agents working in parallel never touch each other's work.
section: Using Pulpo
order: 4
---

## Workspaces

Running `pulpo` in a repository adds it. To add another, :kp[w] then :kp[n].

:kp[x] in Workspaces removes a repository from the list. Its files stay.

## Sessions

A session is one checkout of a workspace. Selecting one shows its terminals, files and changes.

Every workspace starts with `main`, the repository itself. Each session you create is a git worktree on its own branch, with its own ports and Docker containers. Worktrees made outside Pulpo show up as sessions too, read only.

Each row shows the session's state, name and uncommitted changes (`±3`).

## Create a session

:kp[s], then :kp[n], then a name.

1. Pulpo fetches the remote's default branch and makes a worktree from it in `~/.pulpo/sessions/<repo>/<name>`, on the branch `<you>/<name>`.
2. It copies `main`'s `.env` files, and the files listed in `.worktreeinclude`.
3. A shell opens in it right away.
4. Setup runs in the background: submodules, [Docker](/docs/docker), then the setup script. The session shows *setting up* meanwhile, or *setup failed* with its log.

`<you>` is the `branch_prefix` setting, else git's `user.name`, else your GitHub login.

To pick the branch yourself, say for a Linear ticket, press `Tab` after the name and type or paste it. The name stays the folder's.

Commit and push from a session as usual; `main` sees the commits at once.

## Copy ignored files

A worktree only has tracked files. List the ignored ones a session needs in `.worktreeinclude`, at the repository's root, in `.gitignore` syntax:

```text
config/master.key
certs/
!.env.local
```

Copies are APFS clones: instant, and no extra disk space.

## Delete a session

:kp[x] in Sessions. Pulpo first says what you'd lose. Then it stops the terminals, runs the teardown script, and removes the containers and the worktree.

The branch stays if it has commits. Deleting the folder by hand has the same effect.

::careful
Deleting removes everything that wasn't committed in the session.
::

## States

| Mark | Meaning |
| --- | --- |
| spinner | A terminal is working |
| ◆ | A terminal waits for you |
| ○ | Quiet |

A session shows its most urgent terminal, a workspace its most urgent session. See [Agent status](/docs/terminals#agent-status).
