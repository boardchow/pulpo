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
- In a Rails app with an `app` service, it runs `bin/rails db:prepare` in a new `app` container.

`teardown` works the same way, and runs before a workspace is deleted. `fresh` runs when the project has no database to copy.

The workspace's *setup* tab shows each step as it runs, and ends with *✓ Workspace … is ready* and its ports. A terminal then opens beside it and takes you there, unless you had moved to another tab. If you were in another workspace, this one shows ✓ until you go there, which opens that terminal. Closing the *setup* tab doesn't stop the setup.

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

The *setup* tab ends with what went wrong, and you stay on it, with a terminal beside it to debug in. The workspace shows ✗ until you go there, which opens the *setup* tab. A container that stops at boot fails the setup too, with its last logs. The log is also in `~/.local/state/pulpo/logs/`. Fix the cause, then run the remaining steps in the workspace's terminal, or delete the workspace and create it again.
