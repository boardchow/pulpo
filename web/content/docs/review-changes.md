---
title: Review what an agent changed
description: "Go through every change an agent made, under the code it sits in, approve each one, then look back through its commits."
section: Guides
order: 6
---

## See what isn't committed

Press :kp[c]. Changes, under Files, lists every file in the workspace that changed since the last commit, new files included. Green is new, blue is changed, and red is deleted.

| To | Press |
| --- | --- |
| Move | :k[↑] :k[↓] or :k[j] :k[k] |
| Fold or unfold a folder | :k[←] :k[→] or :k[h] :k[l] |
| Open a file full screen | :k[⏎] or click it |

Once an agent commits a file, it leaves Changes and shows in History.

## Read a change full screen

Full screen, the file takes everything right of the left side, its changes inline with the old and new line numbers. Each change shows with 3 lines around it, back to where its function starts when that's 10 lines up or less. The lines between fold into one row, which names the code they hide that the change sits in, like `impl App { › fn set_current(...) {`. In a changed line, the words that changed stand out. Long lines wrap. Marks on its right edge show where it changed: click one to jump there.

| To | Press |
| --- | --- |
| Move the line cursor | :k[↑] :k[↓] or :k[j] :k[k], or click a line |
| Move it half a page | :k[d] or :k[space], and :k[u] |
| Scroll a page | :k[pgdn] :k[pgup] |
| Move it to the top, the bottom | :k[g], :k[G] |
| Go between the file and the tree | :k[tab] |
| Step back | :k[esc]: to the tree, to the terminal beside it, then into the terminal |

## Go change by change

Press :kp[C] to open Changes full screen at the first change not approved yet. :k[n] and :k[p] go to the next and previous change, across files. :k[a] approves one: it shrinks to a line and the next comes up, and :k[a] again takes it back. The title counts them, like `4 of 10 approved`.

An approval follows what the change says. When the agent edits the file again, changes it didn't come near stay approved, so you only read what's new. Approvals are kept when Pulpo quits.

## Comment for the agent

Full screen, select lines with the line cursor, :k[shift] :k[↓] or a drag, press :kp[n] and type your comment below them. In the tree, :kp[n] comments on the whole file. :kp[e] edits a comment and :kp[d] deletes it.

Comments stay until you send them, while Pulpo runs. Press :kp[s] or click *send review* to send them all to the workspace's agent, like Claude Code or Codex, each under its place like `src/app.rs:12-14`.

## Look through the commits

Press :kp[h]. History, under Changes, lists the branch's commits since it left the default branch, newest first. On the default branch itself, it lists all of them. More load as you scroll down.

A `↑` marks a commit that isn't on origin yet. The title says where the branch stands: `pushed`, `2 unpushed` or `not pushed`, and `1 to pull` when origin has commits you don't.

Press :k[⏎], or click a commit, to open it full screen. It shows the commit's files and that commit's changes, read the same way, and its author and age in the title. :k[esc] goes back to History.

Pulpo doesn't stage, commit or undo changes. Ask the agent, or use the terminal.
