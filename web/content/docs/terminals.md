---
title: Persistent terminals
description: "Terminals run in a background server, so agents keep working when you quit Pulpo, and come back after a reboot."
section: Concepts
order: 12
---

## Terminals outlive Pulpo

Pulpo's terminals run in a background server, not in Pulpo's window. Pulpo starts the server the first time you open it.

- :kp[q] quits Pulpo. The terminals keep running, and so do the agents in them.
- Running `pulpo` again brings them back, with what's on their screens.
- :kp[Q] asks, then stops every terminal and quits. Nothing reopens the next time.

When you install a new build, the server switches to it the next time you open Pulpo. The terminals keep running.

## After a reboot

A reboot stops the server and its terminals. The next time you open Pulpo, it reopens each terminal in its folder. Terminals that ran Claude Code or Codex resume that session, with `claude --resume` or `codex resume`, then leave you a shell. This needs the [hooks](/docs/installation#connect-your-agents).

The bottom line says how many terminals came back, and how many agent sessions resumed. What they printed before the reboot doesn't come back.

## Tabs

Each project and workspace has its own terminals, as tabs. A tab shows the program running in it, like `claude` or `codex`, and its [mark](/docs/agent-status#read-the-marks).

| To | Press | Or |
| --- | --- | --- |
| Open a terminal | :kp[n] in the terminal | Click `+` |
| Go to the previous, next tab | :kp{k="["}, :kp{k="]"} | Click the tab |
| Close the terminal | :kp[x]. Pulpo asks if a program still runs in it | |

A project or workspace always has a terminal. A shell opens, in your `$SHELL`, when you go to one without any, and when you close or exit its last one.

Every key goes to the program in the terminal, except the [prefix](/docs/prefix-key). Terminals in a workspace have [its variables](/docs/docker#use-the-workspaces-ports), like `PULPO_PORT`.

## Mouse

| Action | Does |
| --- | --- |
| Wheel | Scrolls back through the output, up to 5,000 lines. In full-screen programs like `less`, it sends the arrow keys |
| Drag | Selects text and copies it. Hold :k[shift] when the program uses the mouse |
| Ctrl-click | Opens a link in your browser, or a path like `src/app.rs:120` in Files |
