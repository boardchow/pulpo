---
title: Know when an agent needs you
description: "See at a glance which agents are working and which wait for you, then go straight to the one that needs you."
section: Guides
order: 5
---

## Read the marks

| Mark | Means |
| --- | --- |
| Spinner | The terminal is working: its program prints on its own |
| ◆ | The terminal waits for you: the agent asked something or finished, the bell rang, or a program stopped after running for a second or more |
| ○ | Quiet |

Each terminal tab shows its mark. A workspace or project shows the mark of its most urgent terminal. A ◆ clears once you look at that terminal.

A workspace being [set up](/docs/workspace-setup) shows the spinner, then ✓ or ✗ until you go there.

## Go to the agent that waits

Press :kp[a]. Pulpo goes first to a workspace whose setup ended, then to the terminal that has waited longest, in any workspace. Terminals that asked something come before those that finished. Press :kp[a] again for the next one.

## Get exact status from Claude Code

Without help, Pulpo guesses from what the terminal prints. With Pulpo's hooks installed, Claude reports exactly when it works, asks something, or finishes:

```sh
pulpo hooks install
```

Then start a new Claude session; sessions started before don't report. To check the hooks, run `pulpo hooks status`: every hook should say `current`.

## Get notified

With the hooks installed, Ghostty shows a notification when an agent asks something or finishes. It names the terminal, its workspace, and what the agent asked. Each terminal notifies at most once every 10 seconds.

## Report status from other programs

Other programs can report their status with [OSC 7501](https://mitchellh.com/writing/program-status-osc7501). Pulpo shows `working` as working and `blocked` as waiting for you.
