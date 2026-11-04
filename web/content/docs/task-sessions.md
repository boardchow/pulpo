---
title: Task sessions
description: A copy of the repository for one task, so agents working in parallel never touch each other's files, ports or database.
section: Task sessions
order: 10
---

## Make one

:kp[s], then :kp[n], then a name (lowercase letters, digits, `-` and `_`).

1. Pulpo copies the repository to `<repo>.sessions/<name>` next to it. It's an APFS clone: about a second, and no extra disk space until files change.
2. The copy starts from the last commit: uncommitted work stays in `main`. Files git ignores (`.env`, installed dependencies) come along.
3. A shell opens in it right away.
4. Its Docker setup runs in the background (see [Docker and ports](/docs/docker)). The session shows *setting up*, then nothing, or *setup failed* with the path to its log.

## Work in it

The copy is its own git repository, on the branch `main` was on. Make a branch there for the task and push it; other sessions see it after a `git fetch`. Changes shows everything since the copy, commits included.

## Delete one

:kp[x] in Sessions. Pulpo first says what you'd lose: uncommitted changes, commits on no remote, stashes. Then it stops the session's terminals and, in the background, runs its `teardown` command, removes its containers and volumes (its database), and deletes the copy. If Docker fails, nothing more is deleted, and Pulpo points to the log.

::careful
Deleting removes the copy's folder. Push what you want to keep first.
::
