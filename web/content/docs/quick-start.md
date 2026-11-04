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

   A shell opens in the middle.

2. **Start an agent.** Type `claude` and give it a task.

3. **Learn the prefix.** Press :k[⌘p], release, then a key. :kp[?] lists every key.

4. **Start a second task in a new workspace.** :kp[w], then :kp[n], then a name. It's a worktree on its own branch, and a shell opens in it. Start `claude` there too.

5. **See who needs you.** A ◆ marks the workspace waiting for you. :kp[a] goes to it.

6. **Review the work.** :kp[c] shows every file the workspace changed. :k[⏎] opens one, :k[esc] goes back.

7. **Quit.** :kp[q] quits and leaves the agents working. Run `pulpo` to come back.
::
