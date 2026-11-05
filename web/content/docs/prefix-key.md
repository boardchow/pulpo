---
title: The prefix key
description: "Every Pulpo action is the prefix, then one key. It works everywhere, terminals included."
section: Concepts
order: 13
---

## How it works

Press :k[⌘p], release it, then press one key. In these docs that's written :kp[w]: the prefix, then the key.

While Pulpo waits for the second key, the bottom left corner shows :k[⌘p]. If no key comes within three seconds, the prefix is dropped.

The prefix works everywhere, including while a program runs in a terminal. Every other key goes to the program or panel you're in, so Pulpo never takes a key your agent or editor needs.

:kp[?] lists every key. Any key closes the list.

## Two prefixes

By default, both :k[⌘p] and :k[⌃b] are the prefix. :k[⌘p] is free in Ghostty, macOS and terminal programs. :k[⌃b] is for terminals that keep :k[⌘] for themselves.

To send :k[⌃b] itself to the program, for example to Claude Code, press it twice.

## Keys act on where you are

The same key does what fits the panel you're in:

| Key | Projects | Workspaces | Terminal | Files |
| --- | --- | --- | --- | --- |
| :kp[n] | Add a project | New workspace | New terminal | New file |
| :kp[x] | Remove from the list | Delete the workspace | Close the terminal | Delete the file |

:kp[e], :kp[r] and :kp[d] edit, rename and delete files, so they work in Files only.

## Change the prefix

Set your own in `~/.config/pulpo/keys.toml`:

```toml
prefix = ["cmd-j", "ctrl-a"]
```

To see what your terminal sends for a key, run `pulpo keytest`. See [Configuration](/docs/configuration) for the key names.
