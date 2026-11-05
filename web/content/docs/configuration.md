---
title: Configuration
description: "Every file Pulpo reads, every setting in them, and their defaults. Pulpo works without any of them."
section: Reference
order: 15
---

## Files

| File | Sets |
| --- | --- |
| `~/.config/pulpo/keys.toml` | [The prefix](#keys) |
| `~/.config/pulpo/config.toml` | [The theme](#theme) |
| `~/.config/pulpo/repos/<repo>.toml` | [How a repository's workspaces are set up](#workspaces) |
| `.worktreeinclude`, at a repository's root | [Ignored files to copy into workspaces](#worktreeinclude) |
| `.conductor/settings.toml`, `.conductor/settings.local.toml` | [Conductor's settings](#conductor), read as workspace settings |

`XDG_CONFIG_HOME` moves `~/.config`.

## Keys

`keys.toml` has one setting:

| Setting | Default | Does |
| --- | --- | --- |
| `prefix` | `["cmd-p", "ctrl-b"]` | The [prefix](/docs/prefix-key): one key, or a list of keys |

```toml
prefix = ["cmd-j", "ctrl-a"]
```

A key is a name, after any modifiers:

| Part | Values |
| --- | --- |
| Modifiers | `cmd-` or `super-`, `ctrl-`, `alt-` or `opt-`, `shift-` |
| Names | `space`, `tab`, `enter` or `return`, `esc` or `escape`, `backspace`, `left`, `right`, `up`, `down`, or one character |

An uppercase letter means it with :k[shift]: `"N"` is :k[⇧n]. If the file has an error, Pulpo uses the default prefix and says why when it starts. `pulpo keytest` shows what Pulpo receives for each key.

## Theme

`config.toml` has one setting:

| Setting | Default | Values |
| --- | --- | --- |
| `theme` | `"ghostty"` | `"ghostty"`, `"Pulpo Dark"`, a Ghostty theme's name, or the path to a Ghostty theme file |

```toml
theme = "Pulpo Dark"
```

With `"ghostty"`, Pulpo uses Ghostty's `theme` setting, including a `light:…,dark:…` pair, which follows macOS's appearance. Without one, it uses Pulpo Dark.

Pulpo looks for a theme by name in `~/.config/ghostty/themes`, `~/Library/Application Support/com.mitchellh.ghostty/themes`, then Ghostty's own themes. If it can't load the theme, it uses Pulpo Dark and says why when it starts.

## Workspaces

`repos/<repo>.toml` sets how workspaces of one repository are made, where `<repo>` is the repository's folder name. Pulpo writes it the first time you create a workspace there, with every setting commented out and set to what it guessed. Uncomment a setting to set it.

| Setting | Guessed from | Does |
| --- | --- | --- |
| `include` | Rails: `config/master.key`, `config/credentials/*.key` | More ignored files to copy, in `.gitignore` syntax. Adds to the `.env` files and `.worktreeinclude` |
| `ports` | Each `${VAR}` in the Compose file's `ports` | Port variables. Each workspace gets its own port for each |
| `images` | Compose services with `build` and no `image` | The project's images to reuse instead of building them again |
| `db` | The first MySQL, MariaDB, Postgres or PostGIS image | How to copy the project's database: `"mysql"`, `"postgres"`, or `""` for no copy |
| `db_service` | The service with that image | The Compose service that runs the database |
| `fresh` | Nothing | Runs when the project's database isn't running, after the containers start |
| `setup` | Without Compose: install from the lockfiles (pnpm, yarn, bun, npm, Bundler, uv). Rails with an `app` service: `bin/rails db:prepare` in it | Runs at the end of the setup, in the workspace's folder |
| `teardown` | Nothing | Runs in the workspace's folder before it's deleted |
| `branch_prefix` | Nothing | Branches are `<branch_prefix>/<name>`. Empty: git's `user.name`, else your GitHub login, in lowercase |

An empty value, `""` or `[]`, turns a guess off. Two settings are exceptions:

- `include` only adds files. To leave one out, add it with `!` in front, like `"!.env.local"`.
- An empty `branch_prefix` goes back to your git user name.

```toml
ports = ["APP_PORT", "DB_PORT"]
setup = "npm ci && npm run build"
branch_prefix = "ada"
```

### Layers

Each layer overrides the one before:

1. What Pulpo guessed from the Compose file and the lockfiles.
2. `.conductor/settings.toml`, from the new workspace, then `.conductor/settings.local.toml`, from the project's folder.
3. `repos/<repo>.toml`.

`include` patterns add up across the layers instead.

## .worktreeinclude

A file at the repository's root listing ignored files every workspace gets, in `.gitignore` syntax. Pulpo reads it from the new workspace, or else from the project's folder. Every workspace also gets `.env` and `.env.*`, except `.env.example`, `.env.sample` and `.env.template`.

```text
config/local.yml
certs/
!.env.local
```

## Conductor

| Conductor setting | Read as |
| --- | --- |
| `scripts.setup` | `setup` |
| `scripts.archive` | `teardown` |
| `file_include_globs` | `include` |
| `git.branch_prefix` | `branch_prefix` |

A setup or teardown script from a file committed to the repository runs only after you trust it, when a workspace is created. Pulpo remembers the exact script it trusted, and asks again when it changes.

## Environment variables

Pulpo sets these in every terminal of a workspace it made. The project's own folder gets none of them.

| Variable | Value |
| --- | --- |
| `PULPO_PORT` | The first of the workspace's ten ports, from 20000 up |
| `PORT` | `PULPO_PORT`, when the project has no Compose file |
| Each variable in `ports` | The workspace's port for it |
| `COMPOSE_PROJECT_NAME` | `<project>-<workspace>` |
| `COMPOSE_FILE`, `COMPOSE_PATH_SEPARATOR` | The Compose files, plus the one Pulpo adds so git works in containers. Set only when Pulpo adds one |
| `PULPO_WORKSPACE_NAME` | The workspace's name |
| `PULPO_ROOT_PATH` | The project's folder |
| `PULPO_WORKSPACE_PATH` | The workspace's folder |

Setup, `fresh` and teardown scripts also get `CONDUCTOR_PORT`, `CONDUCTOR_ROOT_PATH`, `CONDUCTOR_WORKSPACE_NAME` and `CONDUCTOR_WORKSPACE_PATH`, with the same values.

Pulpo reads these:

| Variable | Changes |
| --- | --- |
| `XDG_CONFIG_HOME` | Where `pulpo/` settings are, instead of `~/.config` |
| `XDG_STATE_HOME` | Where `pulpo/` state is, instead of `~/.local/state` |
| `CLAUDE_CONFIG_DIR` | Where the hooks go, instead of `~/.claude` |
| `SHELL` | The shell new terminals run, `/bin/zsh` without it |
| `GHOSTTY_RESOURCES_DIR` | Where Ghostty's own themes are |

## Files Pulpo writes

| Path | Holds |
| --- | --- |
| `~/.config/pulpo/projects` | The list of projects |
| `~/.config/pulpo/repos/` | Workspace settings, one file per repository |
| `~/.local/state/pulpo/workspaces.json` | The workspaces Pulpo made |
| `~/.local/state/pulpo/trusted-scripts.json` | The committed scripts you trusted |
| `~/.local/state/pulpo/events.jsonl` | What the Claude Code hooks reported |
| `~/.local/state/pulpo/logs/` | Workspace setup and removal logs |
| `~/.local/state/pulpo/server/` | The server's socket, its `log`, and which terminals were open |
| `~/.local/state/pulpo/compose/` | The Compose files Pulpo adds to workspaces |
| `~/.pulpo/workspaces/<repo>/<name>` | The workspaces |
| `~/.claude/settings.json` | The hooks, next to yours |
