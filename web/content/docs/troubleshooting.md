---
title: Troubleshooting
description: The usual problems, and what to do.
section: Reference
order: 16
---

## The prefix does nothing

Run `pulpo keytest` and press it. If :k[⌘p] doesn't show up, your terminal keeps :k[⌘] for itself: use :k[⌃b], or pick another prefix in [keys.toml](/docs/configuration). macOS takes :k[⌃space] to switch keyboard layouts, so it's a poor choice.

## No working or waiting marks for Claude

Run `pulpo hooks status`. If hooks are missing or outdated, `pulpo hooks install`, then start a new Claude session: running ones don't pick them up.

## "The Pulpo server stopped"

Its terminals stopped with it. Pulpo starts a new one; the reason is in `~/.local/state/pulpo/server/log`. If it keeps stopping, Pulpo stops trying: :kp[t] tries again.

## A task session can't be made

- *"it has to be on an APFS disk"*: the copy is an APFS clone; move the repository to an APFS volume.
- *".git isn't a folder"*: the repository is itself a worktree or a submodule; open its main checkout.
- *"could not start the copy from the last commit"*: git was busy in `main` (a lock file); try again.

## "setup failed" on a session

Going to the session shows the path to its log, in `~/.local/state/pulpo/logs/`. Fix the cause, then run the remaining steps in the session's terminal, or delete it and make it again.

## The pulpo is drawn as boxes

It's made of Unicode 16 block characters, which Ghostty draws itself. Other terminals need a font that has them.

## "no Ghostty theme named …"

Pulpo couldn't find the theme Ghostty uses, and uses Linear Dark. Set `theme` in `~/.config/pulpo/config.toml` (see [Colors](/docs/configuration#colors)).
