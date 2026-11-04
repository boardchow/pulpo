---
title: Introduction
description: A terminal for running several coding agents side by side on a Mac.
section: Get started
order: 1
---

::app-screen
::

| Column | Shows | Go there |
| --- | --- | --- |
| Left | Your projects and their workspaces | :kp[p], :kp[w] |
| Middle | The workspace's terminals, as tabs | :kp[t] |
| Right | The workspace's files, and what it changed | :kp[f], :kp[c] |

Every action is the prefix, :k[⌘p], then one key. It works from anywhere, terminals included. The mouse works too.

## Concepts

- **Project:** a git repository.
- **Workspace:** one checkout of a project: `main`, or a git worktree with its own branch, ports and database. The screen shows its terminals, files and changes.
- **Terminals keep running** when you quit Pulpo, and come back after a reboot.
