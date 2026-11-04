---
title: Configuration
description: Pulpo works without settings. These files change the prefix, the colors and how task sessions are set up.
section: Reference
order: 13
---

## The prefix

`~/.config/pulpo/keys.toml`:

```toml
prefix = ["cmd-j", "ctrl-a"]
```

Keys are written `cmd-`, `ctrl-`, `alt-` (or `opt-`), `shift-`, then a character or `space`, `tab`, `enter`, `esc`, `backspace`, `left`, `right`, `up`, `down`. Run `pulpo keytest` to see what your terminal sends.

## Colors

Pulpo uses Ghostty's theme, so it matches the terminal around it; without one, its own Linear Dark. To choose, `~/.config/pulpo/config.toml`:

```toml
theme = "Linear Dark"   # or "ghostty" (the default), or a Ghostty theme's name or path
```

## Task sessions

Guessed from the Compose file and lockfiles, then Conductor's `.conductor/settings.toml` if there is one, then `~/.config/pulpo/repos/<repo>.toml`. See [Docker and ports](/docs/docker#settings).

## Environment

Every terminal Pulpo starts has `PULPO_TAB`, which the hooks use to match Claude to its terminal. A task session's terminals also have `PULPO_SESSION`, `PULPO_PORT`, `COMPOSE_PROJECT_NAME` and their port variables (see [Docker and ports](/docs/docker#what-a-session-gets)).

`XDG_CONFIG_HOME` and `XDG_STATE_HOME` move Pulpo's folders; `CLAUDE_CONFIG_DIR` moves where the hooks go.
