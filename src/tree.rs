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

/// A tree of only `paths` (relative to `root`) and the folders down to
/// them, all unfolded; folders first at every level, like the full tree.
pub fn of_paths<'a>(root: &Path, paths: impl IntoIterator<Item = &'a str>) -> Vec<Row> {
    let mut seen = HashSet::new();
    let mut entries: Vec<(PathBuf, bool)> = Vec::new();
    for path in paths {
        let rel = Path::new(path);
        for dir in rel.ancestors().skip(1).filter(|d| !d.as_os_str().is_empty()) {
            if seen.insert(dir.to_path_buf()) {
                entries.push((dir.to_path_buf(), true));
            }
        }
        if seen.insert(rel.to_path_buf()) {
            entries.push((rel.to_path_buf(), false));
        }
    }
    // A folder sorts by its path, so its contents come right after it.
    entries.sort_by_cached_key(|(rel, is_dir)| {
        let last = rel.components().count() - 1;
        rel.components()
            .enumerate()
            .map(|(i, c)| (i == last && !is_dir, c.as_os_str().to_string_lossy().to_lowercase()))
            .collect::<Vec<_>>()
    });
    entries
        .into_iter()
        .map(|(rel, is_dir)| Row {
            name: rel.file_name().unwrap_or_default().to_string_lossy().into_owned(),
            depth: rel.components().count() - 1,
            path: root.join(rel),
            is_dir,
            expanded: is_dir,
        })
        .collect()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_only_the_paths_and_their_folders() {
        let rows = of_paths(Path::new("/r"), ["src/ui.rs", "README.md", "src/app.rs", "src/web/index.html"]);
        let lines: Vec<String> = rows.iter().map(|r| format!("{}{}{}", "  ".repeat(r.depth), r.name, if r.is_dir { "/" } else { "" })).collect();
        assert_eq!(lines, ["src/", "  web/", "    index.html", "  app.rs", "  ui.rs", "README.md"]);
        assert_eq!(rows[3].path, Path::new("/r/src/app.rs"));
    }
}
