---
title: Agents
description: How Pulpo knows that a terminal works or waits for you, and tells you.
section: Using Pulpo
order: 9
---

## States

- **Working** (spinner): the program is printing on its own. Output right after you type, paste or resize is the program answering you, and short bursts are redraws: neither counts.
- **Waiting** (◆): a run of at least a second ended, the terminal rang its bell, or Claude asks something or stopped. It clears once you look at that terminal.

## Claude Code

With the hooks installed (`pulpo hooks install`), Claude says exactly when it waits and what it asks. Pulpo also learns each terminal's Claude session, to resume it after a reboot.

## Other programs

A program that reports its own state with [OSC 7501](https://mitchellh.com/writing/program-status-osc7501) is taken at its word: *blocked* shows as waiting, *working* as working. Progress reports (OSC 9;4) count as working until then.

## Notifications

When an agent asks something or finishes, Ghostty shows a macOS notification with what it said: at most one per terminal every 10 seconds.

## Go to it

:kp[a] goes to the terminal that has waited longest: those asking before those that finished.
