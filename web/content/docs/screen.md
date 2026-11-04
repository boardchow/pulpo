---
title: The screen
description: Three columns and a bottom line.
section: Using Pulpo
order: 4
---

## Columns

| Column | Panels | Go there |
| --- | --- | --- |
| Left | Workspaces, Sessions | :kp[w], :kp[s] |
| Middle | The session's terminals | :kp[t] |
| Right | Files, Changes | :kp[f], :kp[c] |

The panel with the focus has the accent border. A click also moves the focus; hovering doesn't.

## Full screen

Opening a file in Files or Changes (:k[⏎] or a double click) gives that panel everything right of the sidebar, terminal included, with the keys on the file.

- :k[esc] puts the keys on the tree; moving in it shows each file.
- :k[⏎] or :k[tab] goes back into the file.
- :k[esc] on the tree goes back to the three columns.

## Hide and resize

- :kp{k="<"} hides or shows the left side, :kp{k=">"} the right side. Going to a hidden panel shows it again.
- Drag a border to resize: the sidebar's, the right side's, and the line between a tree and its file.

## Bottom line

Names you type (a new file, a session), questions (answer :k[y], or click *yes*; anything else is no) and short messages.
