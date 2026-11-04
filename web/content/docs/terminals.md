---
title: Terminals
description: Each session has its terminals, as tabs. They run in a background server, so quitting Pulpo doesn't stop them.
section: Using Pulpo
order: 6
---

## Tabs

| Do | Keys | Mouse |
| --- | --- | --- |
| New terminal (a shell) | :kp[n] in the terminal, or :kp[t] in a session without one | Click `+` |
| Next, previous tab | :kp{k="]"}, :kp{k="["} | Click the tab |
| Close | :kp[x] | |

Closing asks first when a program is running in it. A terminal belongs to the session it was started in.

## Keys

Every key goes to the program, except the prefix. Claude Code uses :k[⌃b] to background a command: press :k[⌃b ⌃b].

## Mouse

- **Wheel:** scrolls back through the output. In full-screen programs (`less`, `vim`), it sends arrow keys.
- **Drag:** selects text; it's copied when you let go.
- **Ctrl-click:** opens a link in the browser, or a path like `src/app.rs:120` in Files at that line.
- A program that takes the mouse itself gets the clicks; hold :k[shift] to select anyway.

## Quitting, rebooting, updating

- :kp[q] quits Pulpo. Terminals keep running; `pulpo` brings them back as you left them.
- After a reboot, Pulpo opens them again in their folders, and resumes the Claude sessions that were running (with `claude --resume`; this needs the hooks).
- Updating Pulpo doesn't stop them either.
- :kp[Q] stops them all: Ctrl-C first, so dev servers free their ports and `docker compose up` stops its containers, then, after up to 10 seconds, a hang-up.
