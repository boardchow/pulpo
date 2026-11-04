---
title: Files on disk
description: Where Pulpo keeps its settings and state.
section: Reference
order: 15
---

## Settings

| Path | Holds |
| --- | --- |
| `~/.config/pulpo/workspaces` | Your repositories, one path per line |
| `~/.config/pulpo/keys.toml` | The prefix |
| `~/.config/pulpo/config.toml` | The theme |
| `~/.config/pulpo/repos/<repo>.toml` | How the repository's task sessions are set up |

## State

| Path | Holds |
| --- | --- |
| `~/.local/state/pulpo/server/sock` | The server's socket (private to you; Pulpo never listens on the network) |
| `~/.local/state/pulpo/server/log` | The server's log |
| `~/.local/state/pulpo/server/layout.json` | Where you were and what was open, to come back to it |
| `~/.local/state/pulpo/sessions.json` | The task sessions |
| `~/.local/state/pulpo/events.jsonl` | What the Claude hooks reported |
| `~/.local/state/pulpo/logs/` | Task session setup and removal logs |

A Pulpo started inside a Pulpo terminal uses `server-nested/` instead, so it doesn't show itself.

## Task sessions

`<repo>.sessions/<name>`, next to the repository.
