use std::collections::HashSet;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

pub struct Row {
    pub path: PathBuf,
    pub name: String,
    pub depth: usize,
    pub is_dir: bool,
    pub expanded: bool,
}

/// A file tree that only lists expanded directories, so it stays cheap to
/// rebuild on every file system change.
pub struct Tree {
    root: PathBuf,
    expanded: HashSet<PathBuf>,
    pub rows: Vec<Row>,
}

impl Tree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = Tree { root, expanded: HashSet::new(), rows: Vec::new() };
        tree.rebuild();
        tree
    }

    pub fn rebuild(&mut self) {
        let mut rows = Vec::new();
        self.push_dir(&self.root, 0, &mut rows);
        self.rows = rows;
    }

    pub fn set_expanded(&mut self, path: &Path, expanded: bool) {
        if expanded {
            self.expanded.insert(path.to_path_buf());
        } else {
            self.expanded.remove(path);
        }
        self.rebuild();
    }

    fn push_dir(&self, dir: &Path, depth: usize, rows: &mut Vec<Row>) {
        for (path, is_dir) in list_dir(dir) {
            let expanded = is_dir && self.expanded.contains(&path);
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            rows.push(Row { path: path.clone(), name, depth, is_dir, expanded });
            if expanded {
                self.push_dir(&path, depth + 1, rows);
            }
        }
    }
}

/// Direct children of `dir`, gitignore-aware, directories first.
fn list_dir(dir: &Path) -> Vec<(PathBuf, bool)> {
    let mut entries: Vec<(PathBuf, bool)> = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.depth() == 1)
        .map(|e| (e.path().to_path_buf(), e.file_type().is_some_and(|t| t.is_dir())))
        .collect();
    entries.sort_by_cached_key(|(path, is_dir)| {
        (!is_dir, path.file_name().unwrap_or_default().to_string_lossy().to_lowercase())
    });
    entries
}
