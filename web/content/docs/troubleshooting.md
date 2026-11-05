---
title: Troubleshooting
description: "What a problem or message means, and how to fix it."
section: Reference
order: 17
---

## Keys

| Problem | Fix |
| --- | --- |
| The prefix does nothing | Run `pulpo keytest` and press it. If :k[⌘p] doesn't show up, use :k[⌃b], or [choose another prefix](/docs/configuration#keys) |
| :k[⌃␣] as the prefix does nothing | macOS uses it to switch input sources. Run `pulpo doctor`: it says how to free it |
| *`⌘p f` first: that's for files* | :kp[e], :kp[r] and :kp[d] work in Files only. Press :kp[f] first |
| *using the default keys* | `keys.toml` has an error. Fix it, or run `pulpo doctor` to see it |
| *unsaved changes in the editor* | Save with :k[⌘s], or press :k[esc] twice to throw the changes away. Until then, Pulpo won't switch workspace or quit |

## Agent status

| Problem | Fix |
| --- | --- |
| No status, or no notifications, for Claude | Run `pulpo hooks status`. If a hook is `missing` or `outdated`, run `pulpo hooks install`, then start a new Claude session |
| *run `pulpo hooks install`* | The hooks aren't installed. Run `pulpo hooks install` |
| *no terminal has anything to read* | No terminal waits for you, so :kp[a] has nowhere to go |

## The server

| Message | Means |
| --- | --- |
| *the Pulpo server stopped* | Its terminals stopped with it. Pulpo started a new server. The reason is in `~/.local/state/pulpo/server/log` |
| *the Pulpo server keeps stopping* | Read `~/.local/state/pulpo/server/log`. :kp[t] tries again |
| *runs another build* | A server from another `pulpo` binary is running, for example one built from a checkout. It stops once its terminals are closed |

## Creating a workspace

| Message | Fix |
| --- | --- |
| *a workspace name is lowercase…* | Pick a name with only those |
| *the repository has no commit yet* | Make a first commit in the project |
| *`<path>` already exists* | A folder is left over from an earlier workspace. Run `git worktree list` in the project, then `git worktree remove --force --force <path>`, or pick another name |
| *branch … exists already* | A deleted workspace left its branch, or the name is taken. Pick another name, or delete the branch |
| *`<branch>` isn't a valid branch name* | Fix the branch you typed after :k[tab] |
| *no free ports from 20000 up* | Delete workspaces you no longer need |
| *not a git repository* | When adding a project: pick a folder inside a git repository |

## Setting a workspace up

The setup log lists what went wrong, on lines starting with `!`. Each line contains:

| Line | Means |
| --- | --- |
| *couldn't fetch origin* | The workspace starts from origin's default branch as last fetched |
| *default branch isn't known* | Run `git remote set-head origin --auto` in the project. Until then, workspaces start from the project's last commit |
| *script isn't trusted* | You didn't press :k[y] when Pulpo asked. Run the script by hand in the workspace's terminal |
| *fixed port*, *fixed container name*, *fixed name* | Two workspaces can't both run it. See [what can't be isolated](/docs/docker#fix-what-cant-be-isolated) |
| *git won't work in its containers* | `docker compose config` failed, so Pulpo couldn't add the project's `.git` to the containers |
| *compose files with -f* | Those `make` commands skip the Compose file Pulpo adds, so git doesn't work in their containers |

If the workspace shows *✗ setup failed*, select it: the bottom line shows the log's path. Fix the cause, then run the remaining steps in the workspace's terminal, or delete the workspace and create it again.

## Deleting a workspace

| Message | Fix |
| --- | --- |
| *could not delete workspace* | Open the log it names, in `~/.local/state/pulpo/logs/`. Fix the cause, then delete the workspace again |
| *this is the project itself* | Projects aren't deleted, only taken off the list: :kp[p], then :kp[x] |

## Display

| Problem | Fix |
| --- | --- |
| Pulpo's mascot shows as boxes or garbled | Your terminal's font lacks the Unicode 16 block characters. Use Ghostty |
| *theme: …; using Pulpo Dark* | Pulpo couldn't load the theme in `config.toml`. Check its name against [Theme](/docs/configuration#theme) |
