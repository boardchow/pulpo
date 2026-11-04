---
title: Docker and ports
description: With a Compose file, each task session gets its own containers, ports and copy of the database.
section: Task sessions
order: 11
---

## What a session gets

Every terminal of a task session has these in its environment:

| Variable | Value |
| --- | --- |
| `COMPOSE_PROJECT_NAME` | `<main's project>-<session>`: its own containers and volumes |
| `PULPO_PORT` | The first of ten ports kept for the session, from 20000 |
| Each port variable | Its own free port, after those ten |
| `COMPOSE_FILE` | When a service mounts the session's folder: its compose files, and one that lets git work in it (below) |
| `PULPO_SESSION` | The session's name |
| `PULPO_ROOT_PATH`, `PULPO_WORKSPACE_PATH` | Where `main` and the session are |

So `docker compose up` in a session never collides with `main` or another session, as long as the Compose file reads its host ports from variables:

```yaml
services:
  app:
    ports:
      - ${APP_PORT:-8080}:8080
```

## Git in containers

A worktree's `.git` is a file pointing into `main`'s `.git`, which a container that mounts the session's folder (`.:/app`) doesn't have, so git fails in it. When a service mounts the folder, Pulpo adds a Compose file that also mounts `main`'s `.git` at the same path, read-only: `git log`, `diff` and `merge-base` work in the container, and nothing in it can rewrite the repository (commit from the session's terminal instead). It's in `~/.local/state/pulpo/compose/`, never in your repository. Commands that name their compose files with `-f` (in a Makefile, say) skip it; the setup log says so.

## Setup

In the background, with that environment:

1. tags the images `main` built for the session, instead of building them again;
2. starts the database and copies `main`'s into it (MySQL or Postgres, by streaming a dump), or runs the `fresh` command if `main`'s database isn't running;
3. `docker compose up -d`;
4. runs the `setup` command.

## Settings

A repository's settings come in layers, a later one winning where it says something:

1. What Pulpo guesses: from the Compose file (ports, images, database), and without one, the install the lockfiles call for (`npm ci`, `pnpm`, `yarn`, `bun`, `bundle install`, `uv sync`).
2. Conductor's settings, if the repository has them: `.conductor/settings.toml`, then `.conductor/settings.local.toml`. Pulpo reads `scripts.setup`, `scripts.archive` (run as `teardown`), `file_include_globs` and `git.branch_prefix`, and runs those scripts with Conductor's variables (`CONDUCTOR_PORT`, `CONDUCTOR_ROOT_PATH`, `CONDUCTOR_WORKSPACE_NAME`, `CONDUCTOR_WORKSPACE_PATH`), so a repository set up for Conductor needs nothing more.
3. `~/.config/pulpo/repos/<repo>.toml`, written the first time with each setting as found, commented out. Uncomment one to set it; an empty value (`""`, `[]`) turns it off. `include` adds to the others instead (`!pattern` leaves one out).

An empty value in Conductor's files says nothing. Scripts run with `set -e`: a failing command stops the setup, where Conductor might go on.

```toml
# include = []                      # main's ignored files each session gets, besides .env and .worktreeinclude
# ports = ["APP_PORT", "DB_PORT"]   # port variables; each session gets its own
# images = ["app"]                  # services main builds, reused
# db = "mysql"                      # mysql, postgres, or "" for no copy
# db_service = "db"
# fresh = ""                        # run when main's database isn't running
# setup = ""                        # run at the end of the setup
# teardown = ""                     # run before the session is deleted
# branch_prefix = ""                # branches are <prefix>/<name>; empty for git's name
```

A setup or teardown script from a file committed to the repository (Conductor's `settings.toml`) runs only once you've trusted it: making the session asks, showing which scripts and how they start. *y* runs them and trusts exactly those for the repository; anything else sets the session up without them. A session starts from the remote, so without asking, anyone who can push could run code on your machine. A changed script is asked about again. Keys you type as it appears still go to the terminal; it waits for a second without typing before a key answers it.

That covers scripts only. The install Pulpo guesses (`npm ci` runs the scripts in `package.json`) and Docker builds run what the repository says without asking, as after any `git pull && npm install`.

For a Rails app, the guess runs `bin/rails db:prepare` as `setup` and copies `config/master.key`. The copied database is at `main`'s migrations while the session's code is at the default branch's, so it's migrated after the copy. If `main` has migrations the default branch doesn't, `db/schema.rb` in the session shows them as changes: leave them out of its commits.

## What can't be isolated

The setup log lists what sessions would still share: a fixed host port (`3000:3000`), a `container_name`, a volume with a fixed `name`. Make those come from variables.
