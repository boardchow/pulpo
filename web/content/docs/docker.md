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
| Each port variable | Its own free port, in blocks of ten from 20000 |
| `PULPO_SESSION` | The session's name |

So `docker compose up` in a session never collides with `main` or another session, as long as the Compose file reads its host ports from variables:

```yaml
services:
  app:
    ports:
      - ${APP_PORT:-8080}:8080
```

## Setup

In the background, with that environment:

1. tags the images `main` built for the session, instead of building them again;
2. starts the database and copies `main`'s into it (MySQL or Postgres, by streaming a dump), or runs the `fresh` command if `main`'s database isn't running;
3. `docker compose up -d`;
4. runs the `setup` command.

## Settings

The first time, Pulpo writes `~/.config/pulpo/repos/<repo>.toml` from the Compose file. Edit it freely.

```toml
ports = ["APP_PORT", "DB_PORT"]   # port variables; each session gets its own
images = ["app"]                  # services main builds, reused
db = "mysql"                      # mysql, postgres, or "" for no copy
db_service = "db"
fresh = ""                        # run when main's database isn't running
setup = ""                        # run at the end of the setup
teardown = ""                     # run before the session is deleted
```

## What can't be isolated

The setup log lists what sessions would still share: a fixed host port (`3000:3000`), a `container_name`, a volume with a fixed `name`. Make those come from variables.
