---
title: Troubleshooting
description: Common problems and their fixes.
section: Reference
order: 11
---

## The prefix does nothing

Run `pulpo keytest` and press it. If :k[⌘p] doesn't show up, use :k[⌃b], or [choose another prefix](/docs/configuration#prefix).

## No status for Claude

Run `pulpo hooks status`. If hooks are missing, run `pulpo hooks install` and start a new Claude session.

## "The Pulpo server stopped"

Its terminals stopped with it. Pulpo starts a new server; the reason is in `~/.local/state/pulpo/server/log`. If it keeps stopping, :kp[t] tries again.

## A workspace can't be created

- *"the repository has no commit yet"*: make a first commit.
- *"branch … exists already"*: a deleted workspace left its branch. Pick another name, or delete the branch.
- *"already exists"*: the folder is left over. Run `git worktree list`, then `git worktree remove --force --force <path>`.

## "setup failed"

Select the workspace to see its log path. Fix the cause, then run the remaining steps in its terminal, or delete the workspace and create it again.

## The pulpo shows as boxes

Your terminal's font lacks Unicode 16 block characters. Use Ghostty.
