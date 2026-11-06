# Pulpo

Pulpo is a Mac terminal app, written in Rust, that runs coding agents side by side, each in its own workspace. [PLAN.md](PLAN.md) explains how it's built.

## Writing code

Work like a lazy senior developer: the best code is the code never written. Solve the whole problem with the least new code.

### Before you write

Read the task and the code it touches. List every place the change must reach: callers, tests, fixtures, config, docs. Check what it could break for users: data it would destroy or expose, callers that stop working. That is the scope. Extra features are not.

### The smallest complete change

Take the first option that fully works:

1. Does it need to exist? Skip features, options and flexibility nobody asked for, and name them in one line. A vague request gets the smallest version that does the core job.
2. Is it already in this codebase, as a helper or a pattern? Use it the way the surrounding code does.
3. Does the standard library or the platform do it? Use that.
4. Does a dependency in `Cargo.toml` do it? Use that. Never add a dependency for a few lines.
5. Can it be one line a reader gets at a glance? Write one line.
6. Otherwise, write the minimum code that works.

- Be lazy about the solution, never about the change: finish every part the task needs, including the callers, tests and fixtures it breaks.
- No abstraction, wrapper, type conversion, option, config, boilerplate or "for later" code nobody asked for. Keep values in the form the platform already gives you. Deletion beats addition. Keep the structure the code already has.
- The shortest working diff wins, once you know everything it must touch. A one-liner that needs decoding is not short.
- Fixing a bug: find every caller of the code you touch, then fix the cause once, in the shared code.
- Code you move keeps its error handling and validation.
- Between two options of the same size, take the one that is right on edge cases.
- New logic that isn't trivial (a branch, a loop, a parser, anything about security) gets one small test. Trivial changes need none.
- A shortcut with a known limit gets a comment in this form: `shortcut: <the limit>, <when to upgrade>`.

Never cut validation at trust boundaries, error handling that prevents data loss, security, accessibility, or anything the user asked for.

### Comments

The code speaks for itself. A comment is the exception, not the default.

- Default to zero comments. Before writing one, try a clearer name, a smaller function or a named constant.
- Comment only what the code cannot say: a non-obvious why, a workaround for another program's bug, a hidden constraint (order, timing, a spec), or a `shortcut:` note.
- One line, one short sentence. If it needs more, simplify the code, or put the explanation in the commit message.
- Never write comments that describe what the next line does, restate a name, narrate your steps, or record your edit ("added", "fixed", "updated to", "changed from"). No section banners, no commented-out code.
- Write `///` doc comments only where the code around it has them on that kind of item, in the shortest form.
- Leave existing comments alone unless your change makes them wrong. Then fix or delete them.

Bad:

```rust
// Loop through the terminals and keep the ones that are running
// This makes sure we only count live terminals
let running = terms.iter().filter(|t| t.running).count(); // Updated to use filter
```

Good:

```rust
let running = terms.iter().filter(|t| t.running).count();
```

Good, when the why isn't visible:

```rust
// micro only uses 24-bit color when told.
("MICRO_TRUECOLOR", "1"),
```

### Style

- Write it like the code around it: same names, same idioms.
- Use Pulpo's words in code, UI and docs: project, workspace, terminal, files, changes. "Session" only means an agent's session.
- Never use the em dash, in code, UI text, docs or commit messages.

### Before you finish

- `make test` passes.
- If the change alters what users see, type or configure, update the docs too, following [web/content/AGENTS.md](web/content/AGENTS.md).
- For a code change, once its tests pass: read [.agents/review.md](.agents/review.md) and have it reviewed as it says.
- End your reply with one or two lines: what you skipped or didn't check, and any risk the user must know.

## Committing

Commit only when the user asks, and never push without asking. To commit, read [.agents/commit.md](.agents/commit.md) and follow it.

## Releasing

Release only when the user asks. To release, read [.agents/release.md](.agents/release.md) and follow it.
