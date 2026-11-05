---
title: Run agents on separate branches
description: "Give each agent its own workspace, so several tasks move forward at once and none touches another's files."
section: Guides
order: 4
---

## Create a workspace

A [workspace](/docs/projects-and-workspaces) is a copy of your project in its own folder, on its own branch.

1. Press :kp[w] to go to Workspaces, then :kp[n].
2. Type a name, like `fix-login`. Names use lowercase letters, digits, `-` and `_`. Capitals turn lowercase as you type.
3. Press :k[⏎].

The workspace shows in Workspaces, and a shell opens in it. Pulpo makes it in the current project, from origin's default branch, fetched first, on the branch `<you>/fix-login`. `<you>` is your git user name in lowercase, unless you [set a branch prefix](/docs/configuration).

## Name the branch yourself

To use a branch name of your own, for example a ticket's, press :k[tab] after typing the workspace's name. Type or paste the branch, then press :k[⏎]. The workspace's name stays the folder's name.

Pulpo creates the branch. If a branch of that name already has commits of its own, Pulpo refuses: pick another name.

## Start an agent

Type `claude`, or any agent's command, in the workspace's shell. To run something next to it, like tests or a dev server, press :kp[n] from the terminal to open another tab. :kp{k="["} and :kp{k="]"} switch between tabs.

## Move between workspaces

Each of these goes to the workspace's terminal:

| To | Press |
| --- | --- |
| Go to workspace 1 to 9 | :kp[1] to :kp[9] |
| Go to the agent that waits for you, or a workspace whose setup ended | :kp[a] |
| Pick from the list | :kp[w], then :k[↑] :k[↓] and :k[⏎] |
| Find a workspace by name | :kp[/], type part of its name, then :k[⏎] |

The numbers follow the list. A workspace moves to the top when its agent starts waiting for you, and above the quiet ones when it starts working. It stays where it is once you've read it, so rows only move up.

Moving through the list shows each workspace right away: its terminal, its files and its changes.

## Add another repository

Press :kp[p], then :kp[n]. Pick one of the git repositories Pulpo finds in your home folder, or type a path like `~/code/api`, then press :k[⏎]. Its new workspaces join the same list, with the project's name on the right.
