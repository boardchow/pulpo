---
title: Command line
description: Everything else is in the app.
section: Reference
order: 10
---

| Command | Does |
| --- | --- |
| `pulpo [path]` | Open Pulpo, adding the repository at `path` as a project (default: `.`) |
| `pulpo hooks install` | Add Pulpo's hooks to Claude Code |
| `pulpo hooks status` | Show which hooks are installed |
| `pulpo hooks remove` | Remove them |
| `pulpo doctor` | Check the prefix and the hooks |
| `pulpo keytest` | Show what Pulpo receives for each key |

`hooks install` and `hooks remove` take `--dry-run`.
