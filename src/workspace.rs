use std::fs;
use std::path::{Path, PathBuf};

use ignore::gitignore::{Gitignore, GitignoreBuilder};
use ratatui::widgets::ListState;

use crate::git::{self, Status};
use crate::tree::{Row, Tree};

pub struct Workspace {
    pub root: PathBuf,
    pub name: String,
    pub repo: PathBuf,
    pub status: Status,
    pub tree: Tree,
    pub files_state: ListState,
    ignore: Gitignore,
    /// A linked worktree's own git folder, in main's `.git/worktrees`.
    own_git: Option<PathBuf>,
}

impl Workspace {
    pub fn new(root: PathBuf, repo: PathBuf) -> Self {
        let mut ignore = GitignoreBuilder::new(&root);
        ignore.add(root.join(".gitignore"));
        Workspace {
            name: root.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            status: git::status(&root),
            tree: Tree::new(root.clone()),
            files_state: ListState::default().with_selected(Some(0)),
            ignore: ignore.build().unwrap_or_else(|_| Gitignore::empty()),
            // Canonical: a relative gitdir (worktree.useRelativePaths) has `..` in it.
            own_git: fs::read_to_string(root.join(".git"))
                .ok()
                .and_then(|t| t.trim().strip_prefix("gitdir:").and_then(|p| root.join(p.trim()).canonicalize().ok())),
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
    }

    /// A worktree's HEAD and index are its own; refs are everyone's.
    pub fn is_affected(&self, path: &Path) -> bool {
        if let Ok(rel) = path.strip_prefix(self.repo.join(".git")) {
            let name = rel.file_name().unwrap_or_default();
            if rel.starts_with("worktrees") {
                return self.own_git.as_ref().is_some_and(|own| path.starts_with(own)) && (name == "HEAD" || name == "index");
            }
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

    pub fn reveal(&mut self, path: &Path) {
        let root = self.root.clone();
        for dir in path.ancestors().skip(1).take_while(|d| d.starts_with(&root) && *d != root) {
            self.tree.set_expanded(dir, true);
        }
        self.tree.rebuild();
        self.reselect(Some(path.to_path_buf()));
    }

    pub fn selected_row(&self) -> Option<&Row> {
        self.tree.rows.get(self.files_state.selected()?)
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

pub fn registered() -> Vec<PathBuf> {
    fs::read_to_string(registry_path())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_worktrees_head_and_index_are_its_own() {
        let t = std::env::temp_dir().join(format!("pulpo-affected-{}", std::process::id()));
        let _ = fs::remove_dir_all(&t);
        fs::create_dir_all(t.join("main")).unwrap();
        let git = |args: &[&str]| {
            let ok = std::process::Command::new("git").current_dir(t.join("main")).args(["-c", "user.email=t@t", "-c", "user.name=t"]).args(args).status().unwrap();
            assert!(ok.success(), "git {args:?}");
        };
        git(&["init", "-q"]);
        git(&["commit", "-q", "--allow-empty", "-m", "a"]);
        git(&["worktree", "add", "-q", "-b", "a", "../a"]);
        git(&["-c", "worktree.useRelativePaths=true", "worktree", "add", "-q", "-b", "b", "../b"]);
        let main = t.join("main").canonicalize().unwrap();
        let (a, b) = (Workspace::new(t.join("a").canonicalize().unwrap(), main.clone()), Workspace::new(t.join("b").canonicalize().unwrap(), main.clone()));
        let m = Workspace::new(main.clone(), main.clone());
        let a_index = main.join(".git/worktrees/a/index");
        assert!(a.is_affected(&a_index) && !b.is_affected(&a_index) && !m.is_affected(&a_index));
        assert!(fs::read_to_string(t.join("b/.git")).unwrap().contains("gitdir: .."));
        assert!(b.is_affected(&main.join(".git/worktrees/b/HEAD")));
        let refs = main.join(".git/refs/heads/a");
        assert!(a.is_affected(&refs) && b.is_affected(&refs) && m.is_affected(&refs));
        fs::remove_dir_all(&t).unwrap();
    }
}
