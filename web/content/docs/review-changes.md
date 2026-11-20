---
title: Review what an agent changed
description: "Read every change an agent made under the code it sits in, approve each one, and send your comments back to it."
section: Guides
order: 6
---

## Open the review

When a workspace has changes neither approved nor commented on, the terminal's frame says so on the right, like `3 to review`. Press :kp[r], the [prefix](/docs/prefix-key) then r, or click it, to open the review full screen: the list of files on the left, the file on the right.

The review shows everything the workspace's branch changed since it left the default branch, committed or not. Green is new, blue is changed, and red is deleted.

## Read the changes

The file shows its changes inline, between the old and new line numbers. Each change shows with 3 lines around it; the lines between fold into one row that names the code they hide, like `impl App { › fn set_current(...) {`. Marks on the right edge show where the file changed: click one to jump there.

| To | Press |
| --- | --- |
| Move the line cursor | :k[↑] :k[↓] or :k[j] :k[k], or click a line |
| Go between the file and the list | :k[tab] |
| Close the review | :k[esc] from the list, or :kp[r] |

## Go change by change

:k[n] and :k[p] go to the next and previous change, across files. :k[a] approves one: it shrinks to a line and the next comes up, and :k[a] again takes it back. Approvals are kept when Pulpo quits, and a change the agent didn't touch since stays approved.

## Comment for the agent

Select lines with :k[shift] :k[↓] or a drag, press :k[c] and type your comment. :k[⏎] keeps it, and :k[c] on that change opens it again. In the list of files, :kp[n] comments on the whole file, and :kp[d] deletes a comment.

Once every change is approved or commented on, the review closes, and Pulpo types your comments into the agent's input, each under its place like `src/app.rs:12-14`. Press :k[⏎] to send them.

To fix a line yourself, [edit the file](/docs/edit-files) from the review. Pulpo doesn't stage, commit or undo changes: ask the agent, or use the terminal.
