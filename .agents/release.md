# Releasing

Release only when the user asks, and never push without asking. A release is a tag `vX.Y.Z` on `main`. Pushing it starts [the Release workflow](../.github/workflows/release.yml): it checks the tag against `Cargo.toml`, runs the tests, builds one binary for Apple silicon and Intel, and publishes it as a GitHub release with the tag's message as its notes. The install script and `pulpo update` download the latest release.

## The version

`version` in `Cargo.toml` is the only place it's written. Pulpo's title, the landing page and the docs read it from there. Change it only in a release commit.

Pulpo is before 1.0, so the version is `0.MINOR.PATCH`:

- **Patch** (0.3.1 to 0.3.2): only fixes. Nothing new to learn, nothing users must change.
- **Minor** (0.3.2 to 0.4.0): anything else. A new feature, key, command or setting, a change in what users see, or a change to files or state that earlier versions wrote (`keys.toml`, the hooks, what an upgrade resumes). Patch goes back to 0.
- **Major** (1.0.0): only when the user says Pulpo is stable. Never on your own.

One step at a time from the last release: never skip a number, never reuse one, never move the tag of a published release. A broken release gets fixed by the next patch. If you're unsure between patch and minor, take minor and say why.

## Steps

1. **Start clean.** `git switch main`, `git pull --ff-only`, and `git status --short` must be empty.
2. **See what changed.** `git describe --tags --abbrev=0 --match 'v*'` gives the last release, then `git log --no-merges --format='%h %s' <last tag>..HEAD`. With no tag yet, the first release is the version `Cargo.toml` already has, and it must be published before the site serves the install script that downloads it.
3. **Pick the number** with the rules above. Tell the user the number, why, and the notes from step 6, and wait for their go-ahead.
4. **Bump.** Set `version` in `Cargo.toml`, run `cargo check` so `Cargo.lock` follows, then `make test`.
5. **Commit** both files as `Release X.Y.Z`.
6. **Tag** with the notes: write them to a file in a temp folder, then `git tag -a vX.Y.Z -F <file>`. The notes are a short list of what changed for users, from the commit subjects, merged and reworded where needed, under `New` and `Fixed`. Leave out refactors, tests and changes to the docs alone.
7. **Push**, once the user says so: `git push --atomic origin main vX.Y.Z`.
8. **Check.** `gh run watch` until the Release workflow passes, then `gh release view vX.Y.Z` lists `pulpo-macos`. Run `curl -fsSL https://pulpo.sh/install | sh`: `pulpo` shows the new version by its name. Once the site deploys from `main`, the landing page shows it too.

If the workflow fails, no release was published and nobody installed it. Fix the cause on `main` with a new commit, then ask the user before deleting the tag (`git tag -d vX.Y.Z`, `git push origin :refs/tags/vX.Y.Z`) and tagging the fix with the same number.
