---
title: Files
description: The session's files as a tree, the selected one beside it. Read, search, edit, create, rename, delete.
section: Using Pulpo
order: 7
---

## Tree

:k[↑] :k[↓] move, :k[←] :k[→] fold and unfold, :k[⏎] opens a folder or a file. The selected file shows on the right. Names are colored by git status: green new, blue changed, red deleted. Files git ignores are left out.

:k[⏎] on a file opens it full screen: see [Full screen](/docs/screen#full-screen). Long lines scroll sideways with a trackpad swipe, :k[shift] + wheel, or :k[←] :k[→] in the file.

## Search

:kp[/] opens the search over whatever is on screen.

- It starts on the file tree; typing filters it by name (fuzzy).
- :k[tab] searches inside the files instead (two characters or more; case-insensitive unless you type a capital).
- :k[⏎] opens the file full screen, on the line found.

## Edit

:kp[e] edits the selected file, full screen, in Pulpo's editor. The keys are the usual ones:

| Keys | Does |
| --- | --- |
| :k[⌘s] or :k[⌃s] | Save (you stay in the editor) |
| :k[esc] | Leave; with unsaved changes, press it twice to throw them away |
| :k[shift] + moves | Select |
| :k[⌘c] :k[⌘x] :k[⌘v], :k[⌘a] | Copy, cut, paste, select all |
| :k[⌘z], :k[⌘⇧z] | Undo, redo |
| :k[⌘f], :k[⌘g] | Find, next match |

If an agent changes the file while it's open and you haven't changed it, the editor takes the new version.

## Create, rename, delete

| Keys | Does |
| --- | --- |
| :kp[n] | New file in the selected folder. A path makes its folders; a name ending in `/` makes a folder. A new file opens in the editor. |
| :kp[r] | Rename the selected file or folder. |
| :kp[d] | Delete it, after asking. It says whether git can bring it back. |
