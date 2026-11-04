<p align="center">
  <img src="assets/pulpo.svg" width="100" alt="Pulpo">
</p>

<h1 align="center">Pulpo</h1>

<p align="center">
  <a href="https://pulpo.sh">pulpo.sh</a> · <a href="#install">install</a> · <a href="https://pulpo.sh/docs/quick-start/">quick start</a> · <a href="https://pulpo.sh/docs/">docs</a>
</p>

<p align="center"><b>Many agents. One terminal.</b></p>

A terminal for running several coding agents side by side on a Mac.

- **A session per task.** Each session is a worktree with its own branch, files, ports, containers and database. [Sessions →](https://pulpo.sh/docs/workspaces-and-sessions/)
- **See who needs you.** Every session and tab shows whether its agent works or waits for you, and you get a notification. [Terminals →](https://pulpo.sh/docs/terminals/#agent-status)
- **Review next to the work.** The session's files and what it changed sit beside its terminals. [Files and changes →](https://pulpo.sh/docs/files-and-changes/)
- **Quit, reboot, update.** Terminals keep running, and Claude sessions resume after a reboot. [Terminals →](https://pulpo.sh/docs/terminals/)
- **One prefix and the mouse.** `⌘p` then a key, from anywhere, or click. [Keybinds →](https://pulpo.sh/docs/keybinds/)

## Install

```sh
curl -fsSL https://pulpo.sh/install | sh
```

Then, in a repository:

```sh
pulpo
```

`⌘p q` quits and leaves your agents working; `pulpo` brings them back. [Quick start →](https://pulpo.sh/docs/quick-start/)

macOS and [Ghostty](https://ghostty.org). [Claude Code](https://claude.com/claude-code) to see what agents do.

## Docs

[pulpo.sh/docs](https://pulpo.sh/docs/): [installation](https://pulpo.sh/docs/installation/) · [quick start](https://pulpo.sh/docs/quick-start/) · [sessions](https://pulpo.sh/docs/workspaces-and-sessions/) · [Docker and ports](https://pulpo.sh/docs/docker/) · [keybinds](https://pulpo.sh/docs/keybinds/) · [configuration](https://pulpo.sh/docs/configuration/) · [troubleshooting](https://pulpo.sh/docs/troubleshooting/)

## Development

```sh
make install   # build and install into ~/.cargo/bin
make test      # unit tests and the server's end-to-end test
```

How it's built: [PLAN.md](PLAN.md). Contributing: [CONTRIBUTING.md](CONTRIBUTING.md). The site and the docs: [web/](web).

## License

[MIT](LICENSE)
