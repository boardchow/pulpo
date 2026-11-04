---
title: Files and changes
description: The workspace's files, what it changed, and its commits.
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

Every file not committed yet, shown whole with its changes in place. Marks on the right edge show where it changed. Opened full screen on a big screen, old and new sit side by side. A commit takes its files out of Changes and into History.

Pulpo never runs git for you. Agents, or you in a terminal, commit and push.

## History

Under Changes, :k[h] after the prefix: the workspace's commits since it started, newest first. A workspace you created starts at the commit it was made from, any other branch where it left `main`; `main` shows its last 20 commits.

`↑` marks a commit not pushed yet, to the branch of the same name on origin. The title says where the branch stands: `pushed`, `2 unpushed`, `not pushed`, and `1 to pull` when origin has commits you don't.

:k[⏎] or a double click opens a commit full screen: its files, each shown whole with the commit's changes in place, and its author and age in the title. :k[esc] goes back to History.

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
