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

## Get exact status

Without [hooks](/docs/agents), Pulpo guesses from what the terminal prints. Install them, so Claude Code and Codex report exactly when they work, ask something, or finish:

```sh
pulpo hooks install
```

Each hook shows as `added`. Start a new session to see its status: sessions started before don't report. In Codex, also type `/hooks` and trust Pulpo's.

## Get notified

With the hooks installed, Pulpo asks your terminal app to show a notification when an agent asks something or finishes. Not every terminal app shows them. It names the terminal, its workspace, and what the agent asked. Each terminal notifies at most once every 10 seconds.
