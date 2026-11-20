---
title: Find and edit files
description: "Find any file or line in a workspace, read it, and make a quick edit without leaving Pulpo."
section: Guides
order: 7
---

## Open Files

Press :kp[f], the [prefix](/docs/prefix-key) then f. Files takes the whole screen: the workspace as a tree on the left, the selected file on the right. Names are colored by what the workspace changed: green new, blue changed, red deleted. :kp[f] again, or :k[esc] from the tree, goes back to the terminal.

| To | Press |
| --- | --- |
| Move, fold a folder | :k[↑] :k[↓], :k[←] :k[→] |
| Read the file | :k[⏎] or click it, then :k[tab] back to the tree |
| Scroll it | :k[j] :k[k], :k[d] :k[u], :k[pgdn] :k[pgup], and sideways :k[h] :k[l] |

The line cursor's line ends with who changed it last, when, and the commit's subject.

## Search

In Files, press :kp[/] and type part of a file's name. Press :k[tab] to search inside files instead, and :k[tab] again to go back to names. :k[⏎] opens the file, at the line.

Search covers the files of the workspace, except the ones git ignores. Text in lowercase matches any case; type a capital letter to match case exactly. Files over 1 MB and binary files are skipped.

## Create, rename and delete

In Files, :kp[n] makes a file in the selected folder and opens it in the editor; end the name with `/` to make a folder. :kp[r] renames the selected file or folder, and :kp[d] deletes it, after asking whether git can restore it. Names stay inside the workspace.

## Edit a file

Press :kp[e] on a file in Files, or in the [review](/docs/review-changes) on the line to fix. From a terminal, Ctrl-click a path like `src/app.rs:120`. The editor opens at that line.

| To | Press |
| --- | --- |
| Save | :k[⌃s] |
| Undo, redo | :k[⌃z], :k[⌃y] |
| Find | :k[⌃f], then :k[⏎] for the next match and :k[⇧⏎] for the previous one |
| Leave | :k[esc]. With unsaved changes, press it twice to throw them away |

See [Keybinds](/docs/keybinds#editor) for every key. While you have unsaved changes, Pulpo won't switch workspace or quit.
