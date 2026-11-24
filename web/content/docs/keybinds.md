---
title: Keybinds
description: "Every key and mouse action in Pulpo, by where you are."
section: Reference
order: 15
---

Keys written :kp[w] are the [prefix](/docs/prefix-key), :k[⌃b], then the key. In Pulpo, :kp[?] lists them.

## Workspaces

| Keys | Does |
| --- | --- |
| :kp[w] | The workspace list |
| :kp[n] | New workspace: pick its repository, then type its name |
| :kp[l] | Linear, full screen: read the issues, and press :kp[n] to start a workspace on one |
| :kp[X] | Delete the workspace you're in, after asking. In a project's own folder: hide the project, its folder stays |
| :kp[1] to :kp[9] | The first to ninth workspace in the list, in its terminal |
| :kp[⇧↑], :kp[⇧↓] | The previous, next workspace. For a second after, the keys work without the prefix |
| :kp[a] | The terminal that has waited longest for you |
| :kp[?] | The list of every key. Any key closes it |

## Terminals

Every key goes to the program, except the prefix.

| Keys | Does |
| --- | --- |
| :kp[t] | New terminal, in a split of the focused pane |
| :kp[x] | Close the terminal, asking if a program runs |
| :kp{k="["}, :kp{k="]"} | The previous, next pane |
| :kp[\\], :kp[-] | Split the pane right, down |
| :kp[←] :kp[↑] :kp[↓] :kp[→] | Move between panes. For a second after, the arrows keep moving |
| :kp[z] | Zoom the pane, or show them all |
| :k[⌃b] :k[⌃b] | Sends :k[⌃b] to the program |

## Files

| Keys | Does |
| --- | --- |
| :kp[f] | Open Files full screen, or close it |
| :kp[/] | In Files: find a file by name. :k[tab] searches inside files, :k[⏎] opens it |
| :kp[n] | In Files: new file in the selected folder, or a folder if the name ends with `/` |
| :kp[e] :kp[r] :kp[d] | In Files: edit, rename, delete, after asking |

In the tree, :k[↓] :k[↑] or :k[j] :k[k] move, :k[pgdn] :k[pgup] go ten rows, :k[g] :k[G] the first and last, :k[→] :k[←] or :k[l] :k[h] fold, :k[⏎] reads the file and :k[esc] closes Files. Reading the file, the review's line keys work, :k[h] :k[l] scroll sideways, :k[0] goes back to the left edge, and :k[tab] or :k[esc] go back to the tree.

## Review

| Keys | Does |
| --- | --- |
| :kp[r] | Open the review full screen, or close it |
| :kp[n] | In the review: comment on the selected lines, or the selected file |
| :kp[d] | In the review: delete the comment at the cursor, after asking |
| :kp[e] | In the review: edit the file at the cursor's line |

In the list of files:

| Keys | Does |
| --- | --- |
| :k[↓] :k[↑] or :k[j] :k[k] | Next, previous row |
| :k[→] or :k[l] | Unfold a folder |
| :k[←] or :k[h] | Fold a folder, or go to the one above |
| :k[⏎] | Fold or unfold a folder; go into a file |
| :k[tab] | Go into the file |
| :k[esc] | Close the review |

In the file, a line cursor moves:

| Keys | Does |
| --- | --- |
| :k[↓] :k[↑] or :k[j] :k[k] | Next, previous line |
| :k[d] or :k[space], :k[u] | Half a page down, up |
| :k[g] :k[G] or :k[home] :k[end] | First, last line |
| :k[pgdn] :k[pgup] | Scroll a page down, up, keeping 2 lines in view |
| :k[shift] :k[↓] :k[↑] or :k[J] :k[K] | Select lines |
| :k[n] :k[p] | Next, previous change, in the next files too |
| :k[a] | Approve the change and go to the next one not approved or commented on. On an approved change, take it back |
| :k[c] | Comment on the selected lines. On a commented change, open it to edit the comment |
| :k[tab] | Go back to the list of files |
| :k[esc] | Drop the selection, then go back to the list of files |

Writing a comment:

| Keys | Does |
| --- | --- |
| :k[⏎] | Keep it. An empty one is deleted |
| :k[⌥⏎] or :k[⌃j] | New line |
| :k[←] :k[→] :k[↑] :k[↓] | Move in the text |
| :k[esc] | Cancel |

## Editor

| Keys | Does |
| --- | --- |
| :k[⌃s] | Save |
| :k[esc] | Leave the editor. With unsaved changes, press twice to throw them away |
| :k[⌃z] | Undo |
| :k[⌃y] | Redo |
| :k[⌃a] | Select all |
| :k[⌃c] :k[⌃x] :k[⌃v] | Copy, cut, paste |
| :k[⌃f] | Find. Then :k[⏎] next, :k[⇧⏎] previous, :k[esc] done |
| :k[⌃g] | Next match |
| :k[⌃←] :k[⌃→] or :k[⌥←] :k[⌥→] | A word left, right |
| :k[home] :k[end] | Start, end of the line |
| :k[⌃home] :k[⌃end] | Start, end of the file |
| :k[pgup] :k[pgdn] | A page up, down |
| :k[tab], :k[⇧tab] | Indent, unindent |
| :k[⌥⌫] | Delete the word before the cursor |

Hold :k[shift] with a move to select.

## Quit

| Keys | Does |
| --- | --- |
| :kp[q] | Quit. The terminals keep running |
| :kp[Q] | Stop every terminal and quit, after asking |

Neither works while the editor has unsaved changes.

## The workspace list

| Keys | Does |
| --- | --- |
| Typing | Filter the list |
| :k[↓] :k[↑], :k[⌃n] :k[⌃p] or :k[⌃j] :k[⌃k] | Next, previous row |
| :k[⏎] | Go to the workspace. On `+ new workspace`: make one |
| :k[⌃x] | Delete the selected workspace, after asking. On a project: hide it, its folder stays |
| :k[⌫], :k[⌥⌫] or :k[⌃w], :k[⌃u] | Delete a character, a word, everything |
| :k[esc] | Close |

In the list of repositories, typing filters, :k[↓] :k[↑] and :k[pgdn] :k[pgup] move, :k[⏎] picks the selected repository, or the path you typed, and :k[esc] closes it.

Naming a new workspace:

| Keys | Does |
| --- | --- |
| :k[tab] | Switch between the name and the branch |
| :k[⏎] | Make the workspace. With no name or branch: work in the project's own folder |
| :k[esc] | Cancel |

## Questions

| Keys | Does |
| --- | --- |
| :k[y] | Yes |
| Any other key | No |

## Mouse

| Action | Does |
| --- | --- |
| Click a workspace in the sidebar | Go there. `+ new` makes one |
| Click the workspace's name | Open the workspace list (when the window is too narrow for the sidebar) |
| Click `◆ 2 waiting` | Go to the terminal that has waited longest |
| Click a pane | Focus it |
| Click `3 to review` | Open the review |
| Click a row in a list | Select it. In the workspace list, go there |
| Drag in a terminal | Select text and copy it. Hold :k[shift] if the program uses the mouse |
| Wheel | Scroll. With :k[shift], scroll sideways in Files and the editor |
| Click the marks beside a change | Jump to that change |
| Click a line in the review | Put the line cursor there. Drag or :k[shift]-click selects lines |
| Click a comment in the review | Edit it |
| Ctrl-click in a terminal | Open a link, or a path like `src/app.rs:120` in the editor, at that line |
| Click outside a list or question | Close it, or answer no |
