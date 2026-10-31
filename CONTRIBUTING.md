# Contributing

Hey, thanks for wanting to help. Pulpo is tiny and young, so every bit counts.

## Found a bug?

Open an issue. Tell me:

- what you did
- what you expected
- what happened instead
- your macOS version and your terminal (Ghostty, kitty, whatever)

A screenshot helps a lot. It's a terminal app, half the bugs are visual lol.

## Got an idea?

Open an issue first, before writing the code. Saves us both a PR that doesn't fit.

A few things Pulpo won't do, on purpose:

- Run git for you. The agents do git, Pulpo just shows it.
- Listen on the network. The server stays on its private Unix socket.

## Sending a PR

- Keep it small. One thing per PR.
- `make test` passes.
- Write it like the code around it. Same names, same style, same amount of comments.
- If it changes what the user sees or the keys, update the README too.

[PLAN.md](PLAN.md) explains how it's built. Read it first, it'll save you some digging.

That's it. Have fun :)
