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
| `~/.config/pulpo/repos/<repo>.toml` | [How a repository's workspaces are set up](#workspaces) |
| `.worktreeinclude`, at a repository's root | [Ignored files to copy into workspaces](#worktreeinclude) |

`XDG_CONFIG_HOME` moves `~/.config`.

## Keys

`keys.toml` has one setting:

| Setting | Default | Does |
| --- | --- | --- |
| `prefix` | `"ctrl-b"` | The [prefix](/docs/prefix-key): one key, or a list of keys |

```toml
prefix = ["ctrl-a", "cmd-j"]
```

A key is a name, after any modifiers:

| Part | Values |
| --- | --- |
| Modifiers | `cmd-` or `super-`, `ctrl-`, `alt-` or `opt-`, `shift-` |
| Names | `space`, `tab`, `enter` or `return`, `esc` or `escape`, `backspace`, `left`, `right`, `up`, `down`, or one character |

An uppercase letter means it with :k[shift]: `"N"` is :k[⇧n]. If the file has an error, Pulpo uses the default prefix and says why when it starts. `pulpo keytest` shows what Pulpo receives for each key.

## Workspaces

`repos/<repo>.toml` sets how workspaces of one repository are made, where `<repo>` is the repository's folder name. Pulpo writes it the first time you create a workspace there, with every setting commented out and set to what it guessed. Uncomment a setting to set it.

| Setting | Guessed from | Does |
| --- | --- | --- |
| `include` | Rails: `config/master.key`, `config/credentials/*.key` | More ignored files to copy, in `.gitignore` syntax. Adds to the `.env` files and `.worktreeinclude` |
| `ports` | Each `${VAR}` in the Compose file's `ports` | Port variables. Each workspace gets its own port for each |
| `images` | Compose services with `build` and no `image` | The project's images to reuse instead of building them again |
| `db` | The first MySQL, MariaDB, Postgres or PostGIS image | How to copy the project's database: `"mysql"`, `"postgres"`, or `""` for no copy |
| `db_service` | The service with that image | The Compose service that runs the database |
| `fresh` | Nothing | Runs when the project has no database to copy, after the containers start |
| `setup` | Without Compose: install from the lockfiles (pnpm, yarn, bun, npm, Bundler, uv). Rails with an `app` service: `bin/rails db:prepare` in a new `app` container | Runs at the end of the setup, in the workspace's folder |
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

## .worktreeinclude

A file at the repository's root listing ignored files every workspace gets, in `.gitignore` syntax. Pulpo reads it from the new workspace, or else from the project's folder. Every workspace also gets `.env` and `.env.*`, except `.env.example`, `.env.sample` and `.env.template`.

```text
config/local.yml
certs/
!.env.local
```

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

Pulpo reads these:

| Variable | Changes |
| --- | --- |
| `XDG_CONFIG_HOME` | Where `pulpo/` settings are, instead of `~/.config` |
| `XDG_STATE_HOME` | Where `pulpo/` state is, instead of `~/.local/state` |
| `CLAUDE_CONFIG_DIR` | Where the Claude Code hooks go, instead of `~/.claude` |
| `CODEX_HOME` | Where the Codex hooks go, instead of `~/.codex` |
| `SHELL` | The shell new terminals run, `/bin/zsh` without it |

## Files Pulpo writes

| Path | Holds |
| --- | --- |
| `~/.config/pulpo/projects` | The list of projects |
| `~/.config/pulpo/repos/` | Workspace settings, one file per repository |
| `~/.local/state/pulpo/workspaces.json` | The workspaces Pulpo made |
| `~/.local/state/pulpo/events.jsonl` | What the agents' hooks reported |
| `~/.local/state/pulpo/logs/` | Workspace setup and removal logs |
| `~/.local/state/pulpo/server/` | The server's socket, its `log`, and which terminals were open |
| `~/.local/state/pulpo/compose/` | The Compose files Pulpo adds to workspaces |
| `~/.pulpo/workspaces/<repo>/<name>` | The workspaces |
| `~/.claude/settings.json` | The Claude Code hooks, next to yours |
| `~/.codex/hooks.json` | The Codex hooks, next to yours |
