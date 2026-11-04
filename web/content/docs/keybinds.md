---
title: Keybinds
description: Press the prefix, let go, then a key. The prefix works everywhere, in terminals and popups too.
section: Reference
order: 12
---

The prefix is :k[⌘p] or :k[⌃b]. Pressing :k[⌃b] twice sends one to the program. If no key follows within 3 seconds, nothing happens. To change it, see [Configuration](/docs/configuration).

## Panels

| After the prefix | Does |
| --- | --- |
| :k[w] :k[s] :k[t] | Workspaces, Sessions, the terminal |
| :k[f] :k[c] | Files, Changes |
| :k{k="<"} :k{k=">"} | Hide or show the left side, the right side |
| :k{k="["} :k{k="]"} | Previous, next terminal tab |
| :k[a] | The terminal that has waited longest |
| :k[?] | The keybinds |

## Make and close

| After the prefix | In Workspaces | In Sessions | In the terminal | In Files |
| --- | --- | --- | --- | --- |
| :k[n] | Add a repository | New task session | New terminal | New file or folder |
| :k[x] | Remove from the list | Delete the session | Close the terminal | Delete |

## Files

| After the prefix | Does |
| --- | --- |
| :k[/] | Search: names, :k[tab] for inside files |
| :k[e] :k[r] :k[d] | Edit, rename, delete |

Without the prefix, in Files and Changes:

| Keys | Does |
| --- | --- |
| :k[↑] :k[↓] :k[←] :k[→] :k[⏎] | Move, fold, open full screen |
| :k[esc] | From the file to the tree, then to the three columns |
| :k[tab] | Between the tree and the file, full screen |
| :k[pgup] :k[pgdn] | Scroll the file |

## Quit

| After the prefix | Does |
| --- | --- |
| :k[q] | Quit; terminals keep running |
| :k[Q] | Quit and stop every terminal |

## Mouse

| Do | Does |
| --- | --- |
| Click | Goes there: a panel, a row, a tab, `+` for a new tab |
| Double-click | Opens a file full screen |
| Drag | Selects text in a terminal (copied when you let go); a border resizes |
| Wheel, swipe | Scrolls; :k[shift] + wheel or a sideways swipe scrolls files sideways |
| Ctrl-click | Opens a link, or a `path:line` |
