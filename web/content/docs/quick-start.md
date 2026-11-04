---
title: Quick start
description: From the first run to reviewing what an agent did.
section: Get started
order: 3
---

::steps
1. **Open Pulpo in a repository.**

   ```sh
   cd ~/code/my-app
   pulpo
   ```

   The repository joins your workspaces and a shell opens in the middle. Keys go to it.

2. **Start an agent.** Type `claude` in that shell and give it a task.

3. **Use the prefix.** Press :k[⌘p], let go, then a key. :kp[?] lists them all. :kp[n] opens another terminal tab (try `make run` there); :kp{k="["} and :kp{k="]"} go between tabs.

4. **Give the next task its own worktree.** :kp[s] goes to Sessions, :kp[n] asks for a name. You get a shell in a fresh worktree on its own branch right away, with its own Docker containers and ports set up in the background. Start `claude` there too.

5. **See who needs you.** A ◆ marks the session and tab waiting for you, and Ghostty sends a notification. :kp[a] goes to the one that has waited longest.

6. **Review the work.** :kp[c] goes to Changes: every file the session changed. :k[⏎] opens a file over the right side, with its changes in place. :k[esc] goes back to the tree, :k[esc] again to the three columns.

7. **Quit.** :kp[q] quits; the agents keep working. Run `pulpo` to come back. :kp[Q] stops everything.
::
