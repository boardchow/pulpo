# Committing

Commit and push once a change is reviewed and its tests pass; the user doesn't need to ask. Turn the work into small commits a reader can follow one by one.

1. **Start clean.** Run `git reset -q` so staging starts empty, then `git status --short` and `git log --oneline -20` to see the message style.
2. **Plan.** Group the changes by intent, in an order where each commit builds and passes `make test` on its own: preparation first (refactors, renames, moves the change depends on), then the change with its tests, then follow-ups like docs.
3. **Commit.** Stage by file with `git add <paths>`. For a file with two intents, stage only some hunks: write `git diff -- <file>` to a patch in a temp folder, remove the hunks that belong elsewhere, then `git apply --cached` it.
4. **Check.** `git status --short` must be empty unless you left something out on purpose. Rerun the related tests on the last commit, and report them with the list of commits.
5. **Push.** `git push origin main`.

Rules:

- One intent per commit. Tests go in the same commit as the code they cover. A small change is one commit: don't split for the sake of it.
- Mechanical changes, like renames, formatting or `Cargo.lock`, get their own commit, so a reader can skip them.
- The subject is one plain sentence in the imperative, under 72 characters, that makes sense on its own and says what changes for the user, like "Add hooks for Codex, and install them for each agent found". No prefixes, no "fix" or "wip". Add a body only when the why isn't obvious: the reason, not a list of files.
- Never `--no-verify`. A failing hook is a finding: fix its cause.
- Never amend or rebase commits that are already pushed without asking.
- A fix for review feedback is a new commit on top, saying what changed, not "address review".
