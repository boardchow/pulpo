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

Pulpo opens on a shell in the repository, which is now a [project](/docs/projects-and-workspaces).

::steps
1. **Try the prefix.** Every Pulpo action is :k[⌃b], released, then one key. It works even while a program runs in the terminal. Press :k[⌃b], release it, then press :k[?]. The list of every key opens; press any key to close it. To learn more, see [the prefix key](/docs/prefix-key).

2. **Create a workspace.** A workspace is a copy of your project on its own branch, so an agent's edits stay out of everything else. Press :kp[n], then :k[⏎] on your repository, at the top of the list. Type `add-tests` and press :k[⏎]. A shell opens in the workspace, on the branch `<you>/add-tests` (shown bottom right), where `<you>` is your git user name in lowercase.

3. **Start an agent.** Type `claude` or `codex`, press :k[⏎], and give it a task. While it works, its tab shows a spinner.

4. **Start a second agent.** Repeat steps 2 and 3 with the name `update-readme`. Two agents now work on two branches, each in its own folder.

5. **Switch between them.** Press :kp[w] to see your workspaces, and :k[⏎] to go to one. :kp[1] and :kp[2] go there directly. A workspace moves up the list when its agent starts working or waits for you, so the numbers can change.

6. **Answer the agent that needs you.** When an agent finishes or asks something, the frame names it on the right, like `◆ add-tests waiting`. With Claude Code or Codex, the bottom line says it too. Press :kp[a] to go to it.

7. **Review what it changed.** When the agent has changed something, the frame says `1 to review` on the right. Once it's done, press :kp[r] to open the review full screen. :k[n] goes to the next change, :k[a] approves it, and :k[c] comments on it. Once every change is approved or commented on, the review closes and your comments go to the agent. See [Review what an agent changed](/docs/review-changes).

8. **Quit and come back.** Press :kp[q]. Pulpo closes, and the agents keep working. Run `pulpo` again: they're where you left them.
::
