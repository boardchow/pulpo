---
title: Find and edit files
description: "Find any file or line in a workspace, read it, and make a quick edit without leaving Pulpo."
section: Guides
order: 7
---

## Search

Press :kp[/] and type. One list shows, in this order:

- **Projects** and **workspaces** whose names match, the best three of each.
- **Files** whose names match, the best ten.
- **Text** inside files, once you've typed two characters.

Search covers the files of the workspace on screen, except the ones git ignores. Text in lowercase matches any case; type a capital letter to match case exactly. Files over 1 MB and binary files are skipped.

| To | Press |
| --- | --- |
| Move | :k[↑] :k[↓], or :k[⌃k] :k[⌃j], or :k[⌃p] :k[⌃n] |
| Open | :k[⏎]: a file full screen at the line, a project or workspace in its terminal |
| Close | :k[esc] |

## Browse files

Press :kp[f]. Files shows the workspace as a tree, with names colored by git status: green new, blue changed, red deleted. The selected file shows on the right.

Move with :k[↑] :k[↓], fold with :k[←] :k[→], and press :k[⏎] to read a file full screen. Full screen, the keys are the same as in [Changes](/docs/review-changes#read-a-change-full-screen).

From a terminal, Ctrl-click a path like `src/app.rs:120` to open it in Files at that line.

## Edit a file

Select the file in Files and press :kp[e]. The editor opens at the line you were reading.

| To | Press |
| --- | --- |
| Save | :k[⌘s] |
| Undo, redo | :k[⌘z], :k[⌘⇧z] |
| Find | :k[⌘f], then :k[⏎] for the next match and :k[⇧⏎] for the previous one |
| Leave | :k[esc]. With unsaved changes, press it twice to throw them away |

Copy, cut, paste and select all use the usual keys, and :k[⌃] works wherever :k[⌘] does. See [Keybinds](/docs/keybinds) for every key.

While you have unsaved changes, Pulpo won't switch workspace or quit. If the file changes on disk and you haven't edited it, the editor reloads it.

## Create, rename and delete

In Files:

- :kp[n] makes a file in the selected folder and opens it in the editor. End the name with `/` to make a folder.
- :kp[r] renames the selected file or folder.
- :kp[d] deletes it, after asking. The question says whether git can restore it.

Names stay inside the workspace: Pulpo refuses a path that leads out of it.
