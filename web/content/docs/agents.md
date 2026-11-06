---
title: Agents
description: "Pulpo runs any coding agent that works in a terminal. With its hooks, it knows exactly what Claude Code and Codex are doing."
section: Concepts
order: 12
---

An agent is a program you give a coding task to in a terminal, like Claude Code or Codex. You start it yourself, in a [workspace](/docs/projects-and-workspaces)'s terminal. Pulpo doesn't pick, install or configure agents.

## Status from what the terminal prints

For every agent, Pulpo watches what its terminal prints:

- A program that prints on its own is working.
- A terminal waits for you when its bell rings, or when a program stops after running for a second or more.

This works with any agent, but it's a guess from the output.

## Status from the hooks

Hooks are commands an agent runs at each step of its work. Pulpo's hooks pass each step to Pulpo. Claude Code and Codex support them.

| With the hooks | Without them |
| --- | --- |
| Pulpo knows when the agent works, asks something or finishes | Pulpo guesses from the output |
| Your terminal app shows a notification when the agent asks something or finishes | No notification |
| After a reboot, the agent's session resumes in its terminal | The terminal reopens with a shell |

To add them, run `pulpo hooks install`. See [Connect your agents](/docs/installation#connect-your-agents) for each agent.

## Status from other programs

Any program can report its status with [OSC 7501](https://mitchellh.com/writing/program-status-osc7501). Pulpo shows `working` as working, and `blocked` as waiting for you.
