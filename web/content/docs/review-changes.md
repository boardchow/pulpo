---
title: Review what an agent changed
description: "Read every file an agent changed, each one whole with its changes in place, then look back through its commits."
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

Full screen, the file takes everything right of the left side, whole, with its changes in place. On a wide screen, the old and new versions sit side by side. Marks on its right edge show where it changed: click one to jump there.

| To | Press |
| --- | --- |
| Move the line cursor | :k[↑] :k[↓] or :k[j] :k[k], or click a line |
| Move it half a page | :k[d] or :k[space], and :k[u] |
| Scroll a page | :k[pgdn] :k[pgup] |
| Move it to the top, the bottom | :k[g], :k[G] |
| Scroll sideways | :k[←] :k[→] or :k[h] :k[l], and :k[0] for the left edge |
| Go between the file and the tree | :k[tab] |
| Step back | :k[esc]: to the tree, to the terminal beside it, then into the terminal |

## Comment for the agent

Full screen, select lines with the line cursor, :k[shift] :k[↓] or a drag, press :kp[n] and type your comment below them. In the tree, :kp[n] comments on the whole file. :kp[e] edits a comment and :kp[d] deletes it.

Comments stay until you send them, while Pulpo runs. Press :kp[s] or click *send review* to send them all to the workspace's agent, like Claude Code or Codex, each under its place like `src/app.rs:12-14`.

## Look through the commits

Press :kp[h]. History, under Changes, lists the branch's commits since it left the default branch, newest first. On the default branch itself, it lists all of them. More load as you scroll down.

A `↑` marks a commit that isn't on origin yet. The title says where the branch stands: `pushed`, `2 unpushed` or `not pushed`, and `1 to pull` when origin has commits you don't.

Press :k[⏎], or click a commit, to open it full screen. It shows the commit's files, each whole with that commit's changes in place, and its author and age in the title. :k[esc] goes back to History.

Pulpo doesn't stage, commit or undo changes. Ask the agent, or use the terminal.
