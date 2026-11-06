# Writing Pulpo's docs

The docs are the Markdown pages in `docs/`. Follow these rules for every change to them.

Fix what a change makes wrong. Before adding anything, ask: does this really need to be in the public docs? Usually it doesn't. Something new isn't a reason to write about it, and what Pulpo already shows, like a hint, the footer or the keybinds list, stays out.

Pulpo is a Mac terminal app that runs several coding agents side by side, each in its own workspace. A new reader should understand what Pulpo is in 10 seconds, and have two agents running in 5 minutes. Aim for the clarity of the best developer tool docs, like Linear, Vercel, Stripe or Zed.

## Source of truth

- Verify every claim against the code before you write it: keys, CLI commands and flags, config keys, defaults, file paths.
- Never invent features, options or behavior. If something is unclear, leave `<!-- TODO: verify ... -->` instead of guessing, and list it when you hand the work back.
- Keep the frontmatter (`title`, `description`, `section`, `order`) and the URL slugs. If you change a slug, fix every link to it and say so.

## One purpose per page

`section` in the frontmatter sets where a page goes:

| Section | Holds | Length |
| --- | --- | --- |
| Get started | Introduction (what Pulpo is, who it's for, why it beats tmux and worktrees by hand), Installation, Quick start (one tutorial with a clear end result) | Introduction under 200 words |
| Guides | How-tos titled by the goal, like "Review what an agent changed" | Under 400 words |
| Concepts | Short explanations: projects, workspaces, the prefix key, terminals that keep running | Short |
| Reference | Keybinds, Configuration, CLI, Troubleshooting: exhaustive tables, no story | As long as it needs |

## Writing

- Open each page with one sentence on what the reader gets done, in `description` and the first line. Value first, mechanics second.
- Use second person, present tense and active voice. Keep sentences short. No filler like "simply", "just" or "easily".
- Define each term the first time a page uses it, and link it to its Concepts page. Then use that same word every time.
- Start each step with a verb. After a step that changes something, say what the reader sees.
- Make code blocks copy-pasteable: no `$` prompts, and explain any placeholder right before the block.
- Use callouts (`::note`, `::careful`) only when skipping them would hurt the reader.
- The site adds Previous and Next links in `order`. Set `order` so that Next is the logical follow-up.

## House rules

- **Words:** project (a git repository), workspace (a worktree Pulpo made, on its own branch), terminal, files, changes. "Session" only means an agent's session.
- **Keys:** write a prefix key as `:kp[w]`, which shows as ⌃b then w, and any other key as `:k[⏎]`. Use `:kp{k="["}` for a bracket. Link to [the prefix](/docs/prefix-key) the first time a page uses it.
- **Agents:** Pulpo runs any agent that works in a terminal. Name agents as examples, never as the only one: "your agent's command, like `claude` or `codex`". Claude Code and Codex also report their status through hooks.
- **Other products:** never name them, except tmux in the Introduction's comparison.
- **Punctuation:** never use the em dash. Use a colon, a comma, parentheses or a period.
- **Layout:** keep everything flush left. A list item is one paragraph: put a step's command or note after the list, or write the steps as prose, never indented under the item.

## Before you finish

- Every internal link resolves, `/docs/<slug>` and `#anchors` included.
- Grep the docs for what you changed, like a key, a flag or a path, so no page still says the old thing.
- `README.md` and the landing page (`web/app/pages/index.vue`) agree with the docs.
- Every TODO you left is listed in your summary.
