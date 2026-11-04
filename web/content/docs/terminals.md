---
title: Terminals
description: Each workspace has its own terminals, as tabs, run by a background server.
section: Using Pulpo
order: 5
---

## Tabs

| Action | Keys | Mouse |
| --- | --- | --- |
| New terminal | :kp[n] | Click `+` |
| Previous, next | :kp{k="["}, :kp{k="]"} | Click the tab |
| Close | :kp[x] | |

Every key goes to the program, except the prefix. To send :k[⌃b], press it twice.

## Mouse

- **Wheel:** scrolls back through the output.
- **Drag:** selects text and copies it. Hold :k[shift] if the program takes the mouse.
- **Ctrl-click:** opens a link, or a path like `src/app.rs:120` in Files.

## Agent status

| State | When |
| --- | --- |
| Working | The program prints on its own |
| Waiting (◆) | A long run ended, the bell rang, or Claude asks something. Clears once you look. |

With the [hooks](/docs/installation#claude-code-hooks), Claude reports exactly when it waits. Other programs can report their own state with [OSC 7501](https://mitchellh.com/writing/program-status-osc7501).

When an agent asks something or finishes, Ghostty shows a notification. :kp[a] goes to the terminal that has waited longest.

## Quit and restart

- :kp[q] quits Pulpo. Terminals keep running; `pulpo` brings them back.
- After a reboot, Pulpo reopens them and resumes their Claude sessions (needs the hooks).
- :kp[Q] stops every terminal, then quits.
