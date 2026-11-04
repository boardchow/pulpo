---
title: Changes
description: "What the session changed: a tree of the files, the selected one shown whole, its changes in place."
section: Using Pulpo
order: 8
---

## Compared with what

| Session | Changes shows |
| --- | --- |
| On `main` itself | What isn't committed. A commit takes it away. |
| On another branch | What the branch changed since it left `main`, commits included. |
| A task session | Everything since it was copied, commits included, so you see all an agent did. |

New files git doesn't know yet are included. The list follows what agents do as they do it.

## Reading

- The file shows whole: removed lines in red where they were, added lines in green. It opens on its first change.
- Marks down its right edge show where it changed (green added, red removed, blue both); click one to go there.
- :k[⏎] opens it full screen, the same way as in Files.

::note
Pulpo never runs git for you: no staging, committing or pushing. The agents (or you, in a terminal) do that.
::
