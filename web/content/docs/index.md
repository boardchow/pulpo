---
title: Introduction
description: A terminal for running several coding agents side by side on a Mac. Each task gets its own checkout of the repository, you see which agent needs you, and the files and what changed sit next to the terminals.
section: Get started
order: 1
---

::app-screen
::

- **Left:** your repositories (workspaces) and their sessions. A mark shows which one is working (spinner) or waiting for you (◆).
- **Middle:** the session's terminals, as tabs. Claude Code, a dev server, a shell.
- **Right:** the session's files, and what it changed.

Everything is one prefix away, from anywhere, terminals included: :kp then a key, or the mouse. Terminals keep running when you quit Pulpo, and come back after a reboot.

::cards
  :::card{title="Installation" to="/docs/installation"}
  One command, macOS.
  :::

  :::card{title="Quick start" to="/docs/quick-start"}
  From the first run to reviewing an agent's work.
  :::

  :::card{title="Task sessions" to="/docs/task-sessions"}
  A git worktree per task, with its own branch, ports and database.
  :::

  :::card{title="Keybinds" to="/docs/keybinds"}
  Every key and what the mouse does.
  :::
::
