---
title: Keybinds
description: "Every key and mouse action in Pulpo, by where you are."
section: Reference
order: 14
---

Keys written :kp[w] are the [prefix](/docs/prefix-key), :k[⌘p] or :k[⌃b], then the key. In Pulpo, :kp[?] lists them.

## Go to

| Keys | Does |
| --- | --- |
| :kp[p] | Projects |
| :kp[w] | Workspaces |
| :kp[t] | The terminal |
| :kp[f] | Files |
| :kp[c] | Changes |
| :kp[h] | History |
| :kp[1] to :kp[9] | The workspace with that number, in its terminal |
| :kp[a] | The terminal that has waited longest for you |
| :kp{k="["}, :kp{k="]"} | The previous, next terminal tab |
| :kp[/] | Search |
| :kp[?] | The list of every key. Any key closes it |

## Show and hide

| Keys | Does |
| --- | --- |
| :kp{k="<"} | Hide or show the left side: Projects and Workspaces |
| :kp{k=">"} | Hide or show the right side: Files, Changes and History |

## Create and close

These act on the panel you're in.

| Keys | Projects | Workspaces | Terminal | Files |
| --- | --- | --- | --- | --- |
| :kp[n] | Add a project | New workspace | New terminal | New file, or a folder if the name ends with `/` |
| :kp[x] | Remove from the list, after asking | Delete the workspace, after asking | Close the terminal, asking if a program runs | Delete, after asking |

## Files only

| Keys | Does |
| --- | --- |
| :kp[e] | Edit the selected file |
| :kp[r] | Rename the selected file or folder |
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
| :k[↓] :k[↑] or :k[j] :k[k] | Scroll a line |
| :k[d] or :k[space], :k[u] | Scroll half a page down, up |
| :k[pgdn] :k[pgup] | Scroll half a page down, up |
| :k[g] :k[G] or :k[home] :k[end] | Top, bottom |
| :k[→] :k[←] or :k[l] :k[h] | Scroll sideways |
| :k[0] | Back to the left edge |
| :k[tab] | Go between the file and the tree |
| :k[esc] | To the tree, then to the three columns |

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
| :k[⌘s] | Save |
| :k[esc] | Leave. With unsaved changes, press twice to throw them away |
| :k[⌘z] | Undo |
| :k[⌘⇧z] or :k[⌃y] | Redo |
| :k[⌘a] | Select all |
| :k[⌘c] :k[⌘x] :k[⌘v] | Copy, cut, paste |
| :k[⌘f] | Find. Then :k[⏎] next, :k[⇧⏎] previous, :k[esc] done |
| :k[⌘g], :k[⌘⇧g] | Next, previous match |
| :k[⌥←] :k[⌥→] | A word left, right |
| :k[⌘←] :k[⌘→] or :k[home] :k[end] | Start, end of the line |
| :k[⌘↑] :k[⌘↓] | Start, end of the file |
| :k[pgup] :k[pgdn] | A page up, down |
| :k[tab], :k[⇧tab] | Indent, unindent |
| :k[⌥⌫] | Delete the word before the cursor |

Hold :k[shift] with a move to select. :k[⌃] works in place of :k[⌘] for save, undo, redo, select all, copy, cut, paste and find. :k[⌃←] and :k[⌃→] move by word.

## Search

| Keys | Does |
| --- | --- |
| :k[↓] :k[↑] | Next, previous result |
| :k[⌃j] :k[⌃k], :k[⌃n] :k[⌃p] | Next, previous result |
| :k[pgdn] :k[pgup] | Ten results down, up |
| :k[⏎] | Open: a file full screen at the line, a project or workspace in its terminal |
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

A question Pulpo asks on its own, like trusting a setup script, ignores keys until you've stopped typing for a second.

## Mouse

| Action | Does |
| --- | --- |
| Click | Focus a panel, select a row, go to a tab. `+` opens a terminal |
| Click a selected folder | Fold or unfold it |
| Double-click | A file: full screen. A workspace: its terminal. A commit: open it |
| Drag in a terminal | Select text and copy it. Hold :k[shift] if the program uses the mouse |
| Drag a border | Resize the columns, or the tree |
| Wheel | Scroll. With :k[shift], scroll sideways in files |
| Click the marks beside a change | Jump to that change |
| Ctrl-click in a terminal | Open a link, or a path like `src/app.rs:120` in Files |
| Click outside a list or question | Close it, or answer no |
