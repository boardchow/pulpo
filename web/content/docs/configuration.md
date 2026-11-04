---
title: Configuration
description: Pulpo works without settings. These files change it.
section: Reference
order: 9
---

## Prefix

`~/.config/pulpo/keys.toml`:

```toml
prefix = ["cmd-j", "ctrl-a"]
```

Modifiers are `cmd-`, `ctrl-`, `alt-` and `shift-`. Run `pulpo keytest` to see what your terminal sends.

## Theme

Pulpo follows Ghostty's theme, or uses Linear Dark without one. To choose, `~/.config/pulpo/config.toml`:

```toml
theme = "Linear Dark"   # or "ghostty", or any Ghostty theme's name
```

## Sessions

How a repository's sessions are set up comes in layers, each overriding the one before:

1. What Pulpo guesses from the Compose file and lockfiles.
2. Conductor's `.conductor/settings.toml`, then `settings.local.toml`.
3. `~/.config/pulpo/repos/<repo>.toml`.

Pulpo writes the last file the first time, with every setting commented out. Uncomment one to set it; an empty value turns it off.

```toml
# include = []                      # extra ignored files to copy, gitignore syntax
# ports = ["APP_PORT", "DB_PORT"]   # port variables; each session gets its own
# images = ["app"]                  # images main builds, reused
# db = "mysql"                      # mysql, postgres, or "" for no copy
# db_service = "db"
# fresh = ""                        # run when main's database isn't running
# setup = ""                        # run at the end of the setup
# teardown = ""                     # run before the session is deleted
# branch_prefix = ""                # branches are <prefix>/<name>
```

A setup or teardown script committed to the repository runs only after you trust it. Creating the session shows it and asks: :k[y] runs it and trusts it, anything else skips it. A changed script asks again.

## Files on disk

| Path | Holds |
| --- | --- |
| `~/.config/pulpo/` | Settings, and the list of workspaces |
| `~/.local/state/pulpo/` | State: the server, sessions, hook events |
| `~/.local/state/pulpo/logs/` | Session setup and removal logs |
| `~/.local/state/pulpo/server/log` | The server's log |
| `<repo>.sessions/<name>` | Sessions, next to their repository |

`XDG_CONFIG_HOME` and `XDG_STATE_HOME` move these folders. `CLAUDE_CONFIG_DIR` moves where the hooks go.
