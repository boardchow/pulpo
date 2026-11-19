---
title: Keybinds
description: "Every key and mouse action in Pulpo, by where you are."
section: Reference
order: 15
---

Keys written :kp[w] are the [prefix](/docs/prefix-key), :k[⌃b], then the key. In Pulpo, :kp[?] lists them.

## Go to

| Keys | Does |
| --- | --- |
| :kp[p] | Projects |
| :kp[w] | Workspaces |
| :kp[t] | The terminal |
| :kp[f] | Files |
| :kp[c] | Changes |
| :kp[h] | History |
| :kp[F] | The selected file, full screen |
| :kp[C] | Changes full screen, at the first change not approved or commented on |
| :kp[H] | The selected commit, full screen |
| :kp[1] to :kp[9] | The workspace with that number, in its terminal |
| :kp[a] | The terminal that has waited longest for you |
| :kp{k="["}, :kp{k="]"} | The previous, next terminal tab |
| :kp[/] | Search |
| :kp[?] | The list of every key. Any key closes it |

## Show and hide

| Keys | Does |
| --- | --- |
| :kp{k="<"} | Hide or show the left side: Projects, Workspaces, Files, Changes and History |

## Create and close

These act on the panel you're in.

| Keys | Projects | Workspaces | Terminal | Files |
| --- | --- | --- | --- | --- |
| :kp[n] | Add a project | New workspace | New terminal | New file, or a folder if the name ends with `/` |
| :kp[x] | Remove from the list, after asking | Delete the workspace, after asking | Close the terminal, asking if a program runs | Delete, after asking |

These work from any panel:

| Keys | Does |
| --- | --- |
| :kp[N] | New workspace in any project: :k[↑] :k[↓] picks the project, :k[⏎] asks for the name |
| :kp[P] | New project |

## Files only

| Keys | Does |
| --- | --- |
| :kp[e] | Edit the selected file |
| :kp[r] | Rename the selected file or folder |
| :kp[d] | Delete it, after asking |

## Changes only

| Keys | Does |
| --- | --- |
| :kp[n] | Comment on the selected lines, or the selected file |
| :kp[e] | Edit the comment at the cursor |
| :kp[d] | Delete it, after asking |

## Quit

| Keys | Does |
| --- | --- |
| :kp[q] | Quit. The terminals keep running |
| :kp[Q] | Stop every terminal and quit, after asking |

Neither works while the editor has unsaved changes.

## Terminal

Every key goes to the program, except the prefix.

| Keys | Does |
| --- | --- |
| :k[⌃b] :k[⌃b] | Sends :k[⌃b] to the program |

## Projects and Workspaces

| Keys | Does |
| --- | --- |
| :k[↓] or :k[j] | Next row, and on into the other list at the end |
| :k[↑] or :k[k] | Previous row, and on into the other list at the start |
| :k[g] or :k[home] | First row |
| :k[G] or :k[end] | Last row |
| :k[→] | In Projects: go to Workspaces. In Workspaces: go to the terminal |
| :k[←] | In Workspaces: go to Projects |
| :k[⏎] | Go to the terminal |

## Files and Changes

In the tree:

| Keys | Does |
| --- | --- |
| :k[↓] :k[↑] or :k[j] :k[k] | Next, previous row |
| :k[pgdn] :k[pgup] | Ten rows down, up |
| :k[g] :k[G] or :k[home] :k[end] | First, last row (Files only) |
| :k[→] or :k[l] | Unfold a folder |
| :k[←] or :k[h] | Fold a folder, or go to the one above |
| :k[⏎] | Fold or unfold a folder; open a file full screen |
| :k[esc] | Go to the terminal |

Full screen, reading the file:

| Keys | Does |
| --- | --- |
| :k[pgdn] :k[pgup] | Scroll a page down, up, keeping 2 lines in view |
| :k[→] :k[←] or :k[l] :k[h] | Scroll sideways, in Files |
| :k[0] | Back to the left edge, in Files |
| :k[tab] | Go between the file and the tree |
| :k[esc] | To the tree, then to the terminal beside it |

A line cursor moves in the file. In Files, its line ends with who changed it last:

| Keys | Does |
| --- | --- |
| :k[↓] :k[↑] or :k[j] :k[k] | Next, previous line |
| :k[d] or :k[space], :k[u] | Half a page down, up |
| :k[g] :k[G] or :k[home] :k[end] | First, last line |
| :k[shift] :k[↓] :k[↑] or :k[J] :k[K] | Select lines, in Changes |
| :k[esc] | Drop the selection, in Changes |

In Changes:

| Keys | Does |
| --- | --- |
| :k[n] :k[p] | Next, previous change, in the next files too |
| :k[a] | Approve the change and go to the next one not approved or commented on. On an approved change, take it back |
| :k[c] | Comment on the selected lines. On a commented change, open it to edit the comment |

Writing a comment:

| Keys | Does |
| --- | --- |
| :k[⏎] | Keep it. An empty one is deleted |
| :k[⌥⏎] or :k[⌃j] | New line |
| :k[←] :k[→] :k[↑] :k[↓] | Move in the text |
| :k[esc] | Cancel |

## History

| Keys | Does |
| --- | --- |
| :k[↓] :k[↑] or :k[j] :k[k] | Next, previous commit |
| :k[pgdn] :k[pgup] | Ten commits down, up |
| :k[g] :k[G] or :k[home] :k[end] | Newest, oldest commit loaded |
| :k[⏎] | Open the commit full screen. :k[esc] comes back |
| :k[esc] | Go to the terminal |

## Editor

| Keys | Does |
| --- | --- |
| :k[⌃s] | Save |
| :k[esc] | Leave. With unsaved changes, press twice to throw them away |
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

## Search

| Keys | Does |
| --- | --- |
| :k[↓] :k[↑] | Next, previous result |
| :k[⌃j] :k[⌃k], :k[⌃n] :k[⌃p] | Next, previous result |
| :k[pgdn] :k[pgup] | Ten results down, up |
| :k[tab] | Switch between file names and inside files |
| :k[⏎] | Open the file full screen, at the line |
| :k[esc] | Close |

In the new project list, :k[↓] :k[↑], :k[pgdn] :k[pgup] and :k[esc] work the same, and :k[⏎] adds the selected repository, or the path you typed.

## Typing a name or a search

| Keys | Does |
| --- | --- |
| :k[⌫] | Delete a character |
| :k[⌥⌫], :k[⌃w] | Delete a word |
| :k[⌃u] | Clear |
| :k[tab] | In New workspace: switch between the name and the branch |
| :k[⏎] | Confirm |
| :k[esc] | Cancel |

## Questions

| Keys | Does |
| --- | --- |
| :k[y] | Yes |
| Any other key | No |

## Mouse

| Action | Does |
| --- | --- |
| Click | Focus a panel, select a row, go to a tab. A file in Files or Changes, or a commit, opens full screen. `+` opens a terminal |
| Click a selected folder | Fold or unfold it |
| Double-click | A workspace: its terminal |
| Drag in a terminal | Select text and copy it. Hold :k[shift] if the program uses the mouse |
| Drag a border | Resize the left side |
| Wheel | Scroll. With :k[shift], scroll sideways in Files |
| Click the marks beside a change | Jump to that change |
| Click a line in Files or Changes, full screen | Put the line cursor there. In Changes, drag or :k[shift]-click selects lines |
| Click a comment in Changes, full screen | Edit it |
| Ctrl-click in a terminal | Open a link, or a path like `src/app.rs:120` in Files |
| Click outside a list or question | Close it, or answer no |
