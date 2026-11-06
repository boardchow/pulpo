---
title: Quick start
description: "In five minutes, you'll have two coding agents working on separate branches, and you'll review what one of them changed."
section: Get started
order: 3
---

You need Pulpo [installed](/docs/installation), Claude Code or Codex, and a git repository with at least one commit. Any other agent that runs in a terminal works too, but Pulpo can only guess its status. Open Pulpo in your repository, replacing `~/code/my-app` with its path:

```sh
cd ~/code/my-app
pulpo
```

Pulpo opens in three columns. The repository shows under Projects on the left, a shell runs in the middle, and the repository's files and changes are on the right. The repository is now a [project](/docs/projects-and-workspaces).

::steps
1. **Try the prefix.** Every Pulpo action is :k[⌃b], released, then one key. It works even while a program runs in the terminal. Press :k[⌃b], release it, then press :k[?]. The list of every key opens; press any key to close it. To learn more, see [the prefix key](/docs/prefix-key).

2. **Create a workspace.** A workspace is a copy of your project on its own branch, so an agent's edits stay out of everything else. Press :kp[w] to go to Workspaces, then :kp[n]. Type `add-tests` and press :k[⏎]. `add-tests` shows in Workspaces with the number 1, and a shell opens in it, on the branch `<you>/add-tests` (shown bottom right), where `<you>` is your git user name in lowercase. If Pulpo asks whether to run the repository's setup script, press :k[y] to run it or :k[n] to skip it.

3. **Start an agent.** Type `claude` or `codex`, press :k[⏎], and give it a task.

4. **Start a second agent.** Repeat steps 2 and 3 with the name `update-readme`. Two agents now work on two branches, each in its own folder.

5. **Switch between them.** Press :kp[1] for the first workspace and :kp[2] for the second. A workspace moves up the list when its agent starts working or waits for you, so the numbers can change. Check the number next to its name.

6. **Answer the agent that needs you.** When an agent finishes or asks something, its workspace shows ◆ and your terminal app shows a notification. Press :kp[a] to go to it.

7. **Review what it changed.** Press :kp[c]. Changes lists every file the agent changed and hasn't committed. Move with :k[↑] and :k[↓], then press :k[⏎] to open a file full screen, with its changes in place. Press :k[esc] to step back: to the list, to the three columns, then to the terminal.

8. **Quit and come back.** Press :kp[q]. Pulpo closes, and the agents keep working. Run `pulpo` again: they're where you left them.
::
