---
title: Run agents on separate branches
description: "Give each agent its own workspace, so several tasks move forward at once and none touches another's files."
section: Guides
order: 4
---

## Create a workspace

A [workspace](/docs/projects-and-workspaces) is a copy of your project in its own folder, on its own branch.

1. Press :kp[n], the [prefix](/docs/prefix-key) then n. The list of repositories opens: your projects first, the one you're in on top, then the other git repositories in your home folder. Type to filter, or type a path like `~/code/api`.
2. Press :k[⏎] on the repository. The popup asks for the name.
3. Type a name, like `fix-login`, and press :k[⏎]. Names use lowercase letters, digits, `-` and `_`: spaces turn into `-`.

A shell opens in the workspace. Pulpo makes it from origin's default branch, fetched first, on the branch `<you>/fix-login`. `<you>` is your git user name in lowercase, unless you [set a branch prefix](/docs/configuration).

## Name the branch yourself

To use a branch name of your own, for example a ticket's, press :k[tab] after typing the workspace's name. Type or paste the branch, then press :k[⏎]. The workspace's name stays the folder's name.

If a branch of that name already has commits of its own, Pulpo refuses: pick another name.

## Start from a Linear issue

Press :kp[l] to read your open Linear issues full screen. Type to search all of Linear, including descriptions and comments, and add `@name`, `#project` or `is:state` to narrow the results. Issues are open ones unless you ask for a state: `@riley is:done` lists what Riley finished. Press :kp[n] on an issue, pick its repository, and Pulpo makes the workspace named after the issue, like `app-52`, on the issue's own branch, and starts `claude` in it with the prompt `Work on Linear issue APP-52`. Set `PULPO_LINEAR_PROMPT` to use another, where `{id}` is the issue's number.

Pulpo only reads Linear, with a personal API key from `LINEAR_API_KEY` or from a file called `linear` in `~/.config/pulpo`. Create one in Linear's settings, under Security.

## Start an agent

Type your agent's command, like `claude` or `codex`, in the workspace's shell. To run something next to it, like tests or a dev server, press :kp[t] to open another terminal. :kp{k="["} and :kp{k="]"} switch between them.

## Move between workspaces

| To | Press |
| --- | --- |
| Go to the first to ninth workspace in the list | :kp[1] to :kp[9] |
| Go to the previous, next workspace | :kp[⇧↑], :kp[⇧↓] |
| Go to the agent that waits for you, or a workspace whose setup ended | :kp[a] |
| Pick from the list | :kp[w], then type to filter, :k[↑] :k[↓] and :k[⏎] |

The numbers follow [the workspace list](/docs/projects-and-workspaces#the-workspace-list), which moves the workspaces that wait for you to the top.
