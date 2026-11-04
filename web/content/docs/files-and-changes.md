---
title: Files and changes
description: The session's files, and what it changed, each as a tree beside the selected file.
section: Using Pulpo
order: 6
---

## Files

:k[↑] :k[↓] move, :k[←] :k[→] fold, :k[⏎] opens. Names are colored by git status: green new, blue changed, red deleted.

| After the prefix | Does |
| --- | --- |
| :k[/] | Search file names. :k[tab] searches inside files. |
| :k[e] | Edit the selected file |
| :k[n] | New file. End the name with `/` for a folder. |
| :k[r] | Rename |
| :k[d] | Delete, after asking |

## Changes

Every file the session changed, shown whole with its changes in place. Marks on the right edge show where it changed.

A session you created is compared with the commit it started from, so the agent's commits are included. `main` is compared with its last commit, any other branch with where it left `main`.

Pulpo never runs git for you. Agents, or you in a terminal, commit and push.

## Full screen

:k[⏎] or a double click on a file opens it over everything right of the sidebar.

- :k[esc] goes to the tree, then back to the three columns.
- :k[tab] goes from the tree back to the file.

## Editor

| Keys | Does |
| --- | --- |
| :k[⌘s] | Save |
| :k[esc] | Leave. Press twice to discard unsaved changes. |
| :k[⌘z], :k[⌘⇧z] | Undo, redo |
| :k[⌘f], :k[⌘g] | Find, next match |

Copy, cut, paste and select all use the usual keys. :k[⌃] works wherever :k[⌘] does.
