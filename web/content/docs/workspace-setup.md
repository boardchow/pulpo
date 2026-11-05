---
title: Set up new workspaces automatically
description: "Make every new workspace ready to run: copy the ignored files it needs, and run your own setup commands."
section: Guides
order: 9
---

## Copy ignored files

A worktree only has the files git tracks. Pulpo also copies the project's `.env` and `.env.*` files, except `.env.example`, `.env.sample` and `.env.template`. In a Rails app, it also copies `config/master.key` and `config/credentials/*.key`.

To copy more, list them in a `.worktreeinclude` file at the repository's root, in `.gitignore` syntax:

```text
config/local.yml
certs/
!.env.local
```

A line starting with `!` leaves a file out. Only ignored files are copied. The copies are APFS clones: instant, and they take no extra disk space until they change.

## Run a setup script

The first time you create a workspace in a repository, Pulpo writes `~/.config/pulpo/repos/<repo>.toml`, with every setting commented out and set to what it guessed. Uncomment `setup` and give it your command:

```toml
setup = "npm ci && npm run build"
```

The setup script runs in the workspace's folder, in the background, after the [containers](/docs/docker) start. It has the [workspace's variables](/docs/docker#use-the-workspaces-ports). Until you set it, Pulpo guesses one:

- Without a Compose file, it installs dependencies from the lockfiles it finds: pnpm, yarn, bun, npm, Bundler or uv.
- In a Rails app with an `app` service, it runs `bin/rails db:prepare` in that container.

`teardown` works the same way, and runs before a workspace is deleted. `fresh` runs when the project's database isn't running to copy from.

While setup runs, the workspace shows *setting up*. You can work in its terminal meanwhile.

## Trust a committed script

A setup or teardown script that comes from a file committed to the repository asks before its first run. Pulpo shows its first line. Press :k[y] to trust that exact script for this repository; any other key skips it. If the script changes, Pulpo asks again.

Scripts in your own `repos/<repo>.toml` never ask.

## Use Conductor's settings

If the repository has `.conductor/settings.toml`, Pulpo reads it, then `.conductor/settings.local.toml`:

| Conductor | Pulpo |
| --- | --- |
| `scripts.setup` | `setup` |
| `scripts.archive` | `teardown` |
| `file_include_globs` | `include` |
| `git.branch_prefix` | `branch_prefix` |

Your `repos/<repo>.toml` overrides them.

## When setup fails

The workspace shows *✗ setup failed*. Select it, and the bottom line shows the log's path, in `~/.local/state/pulpo/logs/`. Fix the cause, then run the remaining steps in the workspace's terminal, or delete the workspace and create it again.
