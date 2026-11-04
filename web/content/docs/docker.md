---
title: Docker and ports
description: Each session gets its own ports and, with a Compose file, its own containers and database.
section: Using Pulpo
order: 7
---

## Environment

Every terminal of a session you created has:

| Variable | Value |
| --- | --- |
| `PULPO_PORT` | The first of ten ports kept for the session, from 20000 |
| `PORT` | `PULPO_PORT`, without a Compose file |
| `COMPOSE_PROJECT_NAME` | `<project>-<session>` |
| `PULPO_SESSION` | The session's name |
| `PULPO_ROOT_PATH`, `PULPO_WORKSPACE_PATH` | Where `main` and the session are |

## Docker

Read host ports from variables so sessions don't collide:

```yaml
services:
  app:
    ports:
      - ${APP_PORT:-8080}:8080
```

When a session is created, Pulpo reuses the images `main` built, copies `main`'s MySQL or Postgres database, runs `docker compose up -d`, then the setup script.

Containers that mount the session's folder also get `main`'s `.git`, read only, so git works inside them.

The setup log lists what can't be isolated: fixed host ports, `container_name`, named volumes.

To change ports, images, the database or the scripts, see [Configuration](/docs/configuration#sessions).
