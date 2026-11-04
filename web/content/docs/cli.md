---
title: Command line
description: Everything else is in the app.
section: Reference
order: 14
---

| Command | Does |
| --- | --- |
| `pulpo [path]` | Opens Pulpo. A repository at `path` (the current folder by default) joins the workspaces. |
| `pulpo hooks install` | Adds Pulpo's hooks to Claude Code. `--dry-run` shows what would change. |
| `pulpo hooks status` | Shows which hooks are installed, outdated or missing. |
| `pulpo hooks remove` | Takes them out. `--dry-run` works here too. |
| `pulpo hooks` | Prints the hooks, to add by hand. |
| `pulpo doctor` | Checks that the prefix reaches Pulpo and that the hooks are installed. |
| `pulpo keytest` | Shows what Pulpo receives for each key you press; :k[q] stops. |
| `pulpo serve` | The server that keeps terminals running. Pulpo starts it. |
| `pulpo emit` | Records a hook event. The hooks call it. |
