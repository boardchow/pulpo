//! Workspaces: every checkout (main or linked worktree) of the repositories
//! in the registry, a plain list of paths in `~/.config/pulpo/workspaces`.

use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ratatui::widgets::ListState;

use crate::git::{self, Status};
use crate::tree::{Row, Tree};

pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
    /// The main checkout this workspace belongs to.
    pub repo: PathBuf,
    pub status: Status,
    pub tree: Tree,
    pub changes_state: ListState,
    pub files_state: ListState,
    ignore: Gitignore,
}

impl Workspace {
    pub fn new(root: PathBuf, repo: PathBuf) -> Self {
        let mut ignore = GitignoreBuilder::new(&root);
        ignore.add(root.join(".gitignore"));
        Workspace {
            name: root.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            status: git::status(&root),
            tree: Tree::new(root.clone()),
            changes_state: ListState::default().with_selected(Some(0)),
            files_state: ListState::default().with_selected(Some(0)),
            ignore: ignore.build().unwrap_or_else(|_| Gitignore::empty()),
            root,
            repo,
        }
    }

    pub fn is_worktree(&self) -> bool {
        self.root != self.repo
    }

    pub fn refresh(&mut self) {
        let selected = self.selected_row().map(|r| r.path.clone());
        self.status = git::status(&self.root);
        self.tree.rebuild();
        self.reselect(selected);
        clamp(&mut self.changes_state, self.status.changes.len());
    }

    /// Whether a file system event concerns this workspace. Skips ignored
    /// paths (build output) and git internals other than HEAD, index and refs.
    pub fn is_affected(&self, path: &Path) -> bool {
        if let Ok(rel) = path.strip_prefix(self.repo.join(".git")) {
            let name = rel.file_name().unwrap_or_default();
            return name == "HEAD" || name == "index" || rel.starts_with("refs");
        }
        path.starts_with(&self.root) && !self.ignore.matched_path_or_any_parents(path, false).is_ignore()
    }

    pub fn reselect(&mut self, path: Option<PathBuf>) {
        if let Some(i) = path.and_then(|p| self.tree.rows.iter().position(|r| r.path == p)) {
            self.files_state.select(Some(i));
        }
        clamp(&mut self.files_state, self.tree.rows.len());
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.tree.rows.get(self.files_state.selected()?)
    }

    /// The selected change as an absolute path, and whether it is untracked.
    pub fn selected_change(&self) -> Option<(PathBuf, bool)> {
        let change = self.status.changes.get(self.changes_state.selected()?)?;
        Some((self.root.join(&change.path), change.kind() == '?'))
    }
}

pub fn clamp(state: &mut ListState, len: usize) {
    let max = len.saturating_sub(1);
    state.select(Some(state.selected().unwrap_or(0).min(max)));
}

fn registry_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("pulpo").join("workspaces")
}

/// Registered repositories, most recently opened first.
pub fn registered() -> Vec<PathBuf> {
    fs::read_to_string(registry_path())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

/// Add a repository (or move it to the top of the list).
pub fn register(repo: &Path) {
    let mut repos = registered();
    repos.retain(|r| r != repo);
    repos.insert(0, repo.to_path_buf());
    save(&repos);
}

pub fn unregister(repo: &Path) {
    let mut repos = registered();
    repos.retain(|r| r != repo);
    save(&repos);
}

fn save(repos: &[PathBuf]) {
    let path = registry_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let lines: Vec<String> = repos.iter().map(|r| r.display().to_string()).collect();
    let _ = fs::write(path, lines.join("\n") + "\n");
}
