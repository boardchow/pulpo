---
title: Task sessions
description: A git worktree of the repository for one task, on its own branch, so agents working in parallel never touch each other's files, ports or database.
section: Task sessions
order: 10
---

## Make one

:kp[s], then :kp[n], then a name (lowercase letters, digits, `-` and `_`).

1. Pulpo fetches `origin`'s default branch (five seconds at most) and makes a git worktree from it in `<repo>.sessions/<name>`, next to the repository, on a new branch `<you>/<name>`: `you` is the repository's `branch_prefix` setting (Conductor's too) if set, else git's `user.name` for that repository (a name set in the repository wins over your global one), else your GitHub login for its remote (from `gh`). Without a remote, it starts from `main`'s last commit. Uncommitted work stays in `main`.
2. Files git ignores aren't in a worktree, so Pulpo copies the ones a session needs from `main`: its `.env` files, and those your `.worktreeinclude` names (the same file Claude Code reads). See [What it gets from main](#what-it-gets-from-main).
3. A shell opens in it right away.
4. Its setup runs in the background: submodules (one that can't be fetched is said in the log, and the rest goes on), then Docker, then the setup script: yours, Conductor's, or the install its lockfiles call for (see [Docker and ports](/docs/docker#settings)). A script committed to the repository is asked about first. The session shows *setting up*, then nothing, or *setup failed* with the path to its log. The log also says what was copied from `main`.

## What it gets from main

Every session gets `main`'s `.env` and `.env.*` files (not `.example`, `.sample` or `.template`). A `.worktreeinclude` at the repository's root adds more, in `.gitignore` syntax; `!` leaves one out:

```text
config/master.key
certs/
!.env.local
```

Only files git ignores are copied (tracked ones come with the branch). They're APFS clones: instant, and no extra disk space until they change. A folder can be listed too (`node_modules/`), but installing dependencies in the session is safer: a Python `.venv` copied elsewhere still runs `main`'s. The repository's `include` setting adds patterns of your own (see [Docker and ports](/docs/docker#settings)).

## Work in it

It's a git worktree: one repository with `main`, its own files and branch. Commit and push from it as usual; `main` sees its commits at once. Changes shows everything since it left the default branch, commits included.

Its terminals have `PULPO_PORT`, the first of ten ports kept for it (`PORT` too when there's no Compose file), so two dev servers never collide.

## Delete one

:kp[x] in Sessions. Pulpo first says what you'd lose: uncommitted changes, and commits on no branch. Then it stops the session's terminals and, in the background, runs its `teardown` command, removes its containers and volumes (its database), and removes the worktree. If Docker fails, nothing more is deleted, and Pulpo points to the log.

Its branch stays if it has commits, so nothing committed is lost. A branch with nothing added goes with the session.

A session's folder deleted by hand (in Finder, with `rm`), or moved away, counts as deleted: Pulpo notices, and removes the rest the same way, without asking. That's its containers and volumes (its database), its worktree's record in git and its untouched branch, so the name can be used again.

::careful
Deleting removes the session's folder, and what wasn't committed in it.
::
