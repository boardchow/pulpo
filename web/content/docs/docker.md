---
title: Run your app in each workspace
description: "Run your app in several workspaces at once, each on its own ports and, with Docker Compose, its own containers and database."
section: Guides
order: 8
---

## Use the workspace's ports

Pulpo keeps ten free ports for each workspace, from 20000 up. The first one is in `PULPO_PORT`. Without a Compose file, `PORT` has it too, which many dev servers read. To start a server on it:

```sh
python3 -m http.server "$PULPO_PORT"
```

Every terminal of the workspace also has `PULPO_WORKSPACE_NAME`, `PULPO_ROOT_PATH` (the project's folder) and `PULPO_WORKSPACE_PATH`. See [every variable](/docs/configuration#environment-variables).

## Use Docker Compose

Read host ports from variables, with the project's own port as the default:

```yaml
services:
  app:
    build: .
    ports:
      - ${APP_PORT:-8080}:8080
```

Pulpo finds each variable in a `ports` entry and gives each workspace its own value. When you create a workspace, its setup then:

1. Reuses the images the project built, for services with `build` and no `image`.
2. Copies the project's database, if a service runs a MySQL, MariaDB, Postgres or PostGIS image. With Docker Desktop, that takes about a second whatever its size. Without a database to copy, the workspace starts from a fresh one.
3. Runs `docker compose up -d --build`, which only builds again what changed since the project's images, like a new dependency.

Each workspace is its own Compose project, so containers, networks and volumes never collide. Containers that mount the workspace's folder also get the project's `.git`, read only, so git works inside them.

The setup log ends with each port, like `APP_PORT: localhost:20010`.

## Fix what can't be isolated

The setup log warns about what two workspaces would share:

| Warning | Fix |
| --- | --- |
| A fixed host port, like `3000:3000` | Read it from a variable, like `${WEB_PORT:-3000}:3000` |
| A fixed `container_name` | Remove it |
| A volume with a fixed `name` | Remove the `name` |

## Change what Pulpo guessed

Pulpo guesses the ports, images and database from the Compose file. To change them, edit `~/.config/pulpo/repos/<repo>.toml`, where `<repo>` is the repository's folder name. See [Configuration](/docs/configuration).

Deleting a workspace removes its containers, its volumes and its images.
