---
title: Finish a workspace
description: "Ship what an agent made, then delete its workspace along with its containers and folder."
section: Guides
order: 10
---

## Commit and push

Pulpo doesn't run git for you. Ask the agent to commit and push, or run this in the workspace's terminal:

```sh
git add -A
git commit -m "Fix the login check"
git push -u origin HEAD
```

History, at :kp[h], says `pushed` once origin has every commit. Open a pull request the way you usually do, for example with `gh pr create`.

## Merge without a pull request

The project's folder has the default branch checked out, so a workspace can't switch to it. Merge from the project's folder instead. In the workspace's terminal, run:

```sh
git -C "$PULPO_ROOT_PATH" merge "$(git branch --show-current)"
```

This merges the workspace's branch into the branch checked out in the project's folder.

## Delete the workspace

1. Press :kp[w] and select the workspace.
2. Press :kp[x]. Pulpo asks first, and says what you'd lose: uncommitted changes, and commits on no branch.
3. Press :k[y].

Pulpo closes the workspace's terminals, runs your teardown script, and removes its containers, volumes, images and worktree.

The branch stays if it has commits, so you can still push it or open a pull request. A branch without commits of its own goes with the workspace.

::careful
Deleting a workspace removes everything in it that wasn't committed.
::

## Delete it by hand

If you delete a workspace's folder yourself, Pulpo notices and removes the rest: its containers and its worktree.
