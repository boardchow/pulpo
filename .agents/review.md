# Reviewing

A fresh reviewer catches what the author can't see. Once a code change is built and its tests pass, have it reviewed before you call it done. Never skip it because the change looks simple. Changes to docs text alone don't need it.

1. **Pick the range.** Run `git fetch origin`, then `BASE=$(git merge-base origin/main HEAD)`. If everything is committed, `HEAD_SHA=$(git rev-parse HEAD)`. Otherwise snapshot the working tree without committing: `git add -A`, then `HEAD_SHA=$(git stash create)`, then `git reset -q`. For a fix made after an earlier review, `BASE` is the commit that review ended on.
2. **Gather the context.** The requirements: what the user asked for, in their words; if that's unclear, ask them for two lines. A description: two or three sentences on what was built and the main decisions. The tests: the commands you ran and what they showed.
3. **Start the reviewer.** Give a subagent the prompt below with the placeholders filled, and nothing of your session's history. If you can't start one, review it yourself in a separate pass from the same prompt, and tell the user it wasn't independent.
4. **Act on the findings.** Fix Critical and Important ones now, with the rules in [AGENTS.md](../AGENTS.md), and rerun the related tests. List Minor ones for the user to decide. Push back on wrong ones with the code or test that shows it. Rule on each "Declined to judge" line in one line: in scope or not. Review again only if the fixes weren't trivial, on the fix range only.

Report the review in this shape:

```
Review: <verdict> · <n> fixed · <n> for you
Fixed: 1. <file:line> <what>
For you: 1. [Minor] <file:line> <what>: <suggestion>
Disputed: 1. <finding>: <why not>
```

The reviewer's prompt:

```
You are a senior code reviewer. Review this change to Pulpo against its
requirements and find the issues before they ship. You review; you do not fix.
Follow the repository's AGENTS.md.

## What was built
[DESCRIPTION]

## Requirements
[REQUIREMENTS]

## Tests run by the author
[TEST_RESULTS]

## Range
Base: [BASE]   Head: [HEAD_SHA]

    git diff --stat [BASE]..[HEAD_SHA]
    git diff [BASE]..[HEAD_SHA]

Read the changed files in full where the diff lacks context, and grep for
callers of anything whose behavior changed.

## Rules
- Read-only. Never change the working tree, the index, HEAD or any branch.
  For another revision, use `git worktree add <temp dir>/review-<sha> <sha>`.
- Do it all yourself; never start subagents.
- The requirements are a vision, not a full spec. Where they are silent,
  what a reasonable user would expect is a requirement.
- Only report code you actually read. Cite file:line.

## What to check
1. Requirements: everything required is there; deviations are flagged as
   intentional or not; problems with the requirements themselves are named.
2. Correctness: logic bugs, edge cases (empty, duplicates, limits,
   concurrency, retries), error handling, data loss.
3. Reach: callers, tests, fixtures, config and docs the change should have
   updated but did not.
4. Tests: related tests updated; new non-trivial logic has a test; tests
   check behavior; the reported runs cover the changed code.
5. Security and trust boundaries: validation, injection into shell commands,
   secrets, what reaches other programs' settings files.
6. Simplicity: code that could be deleted, an existing helper or dependency
   that already does it, abstractions, options or config nobody asked for,
   a dependency added for a few lines, comments that only narrate the code.
7. Users' machines: compatibility with files and state written by earlier
   versions, performance on real repositories, failures that would be silent.

## Severity
- Critical: bugs, security issues, data loss, broken functionality.
- Important: missing requirement, missing reach, test gaps, poor error
  handling, needless complexity that will cost later.
- Minor: style, naming, small polish.
Not everything is Critical. If it is fine, say so; do not invent issues.

## Output
### Strengths
(specific, short)

### Issues
#### Critical
#### Important
#### Minor
Each: file:line · what is wrong · why it matters · how to fix (if not obvious)

### Declined to judge
Every behavior you considered and set aside as out of scope, one line each
with the reason. Write "None" if empty.

### Verdict
Ready: Yes | No | With fixes. One or two sentences why.
```
