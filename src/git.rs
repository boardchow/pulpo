use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default)]
pub struct Status {
    pub branch: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub changes: Vec<Change>,
}

pub struct Change {
    /// Index and worktree status, as in `git status --short` ('.' means unchanged).
    pub code: [char; 2],
    pub path: String,
    /// Lines added and removed compared to HEAD (0 for binary files).
    pub added: usize,
    pub removed: usize,
}

impl Change {
    /// The letter that best describes the change: worktree first, then index.
    pub fn kind(&self) -> char {
        match self.code {
            ['?', _] => '?',
            [x, '.'] => x,
            [_, y] => y,
        }
    }
}

/// Run git in `dir`; stdout on success, the error message otherwise.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").args(args).current_dir(dir).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The checkout containing `dir` (a linked worktree's own directory, if in one).
pub fn toplevel(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"]).ok().map(|out| PathBuf::from(out.trim()))
}

/// The main checkout of the repository containing `dir`, even from a linked worktree.
pub fn main_repo(dir: &Path) -> Option<PathBuf> {
    let common = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok()?;
    let common = PathBuf::from(common.trim());
    match common.file_name() {
        Some(name) if name == ".git" => common.parent().map(Path::to_path_buf),
        _ => toplevel(dir),
    }
}

/// All checkouts of a repository, main checkout first.
pub fn worktrees(repo: &Path) -> Vec<PathBuf> {
    git(repo, &["worktree", "list", "--porcelain"])
        .map(|out| out.lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).collect())
        .unwrap_or_default()
}

pub fn status(root: &Path) -> Status {
    // --no-optional-locks keeps git from rewriting the index, which would
    // wake our own file watcher in a loop.
    let mut status = git(root, &["--no-optional-locks", "status", "--porcelain=v2", "--branch", "-z"])
        .map(|out| parse(&out))
        .unwrap_or_default();
    let counts = git(root, &["--no-optional-locks", "diff", "--numstat", "-z", "HEAD"])
        .map(|out| parse_numstat(&out))
        .unwrap_or_default();
    for change in &mut status.changes {
        if let Some(&(added, removed)) = counts.get(&change.path) {
            (change.added, change.removed) = (added, removed);
        } else if change.kind() == '?' {
            // Untracked: every line is new. Skip large files.
            let path = root.join(&change.path);
            if std::fs::metadata(&path).is_ok_and(|m| m.is_file() && m.len() < 1024 * 1024) {
                change.added = std::fs::read(&path).map_or(0, |b| b.iter().filter(|&&c| c == b'\n').count());
            }
        }
    }
    status
}

/// `git diff --numstat -z` output: path -> (added, removed).
fn parse_numstat(out: &str) -> std::collections::HashMap<String, (usize, usize)> {
    let mut counts = std::collections::HashMap::new();
    let mut records = out.split('\0');
    while let Some(rec) = records.next() {
        let mut fields = rec.splitn(3, '\t');
        let (Some(added), Some(removed), Some(path)) = (fields.next(), fields.next(), fields.next()) else { continue };
        // A rename has an empty path followed by the old and new paths.
        let path = if path.is_empty() { records.nth(1).unwrap_or_default() } else { path };
        // Binary files show "-".
        counts.insert(path.to_string(), (added.parse().unwrap_or(0), removed.parse().unwrap_or(0)));
    }
    counts
}

pub fn parse(out: &str) -> Status {
    let mut status = Status::default();
    let mut records = out.split('\0').filter(|r| !r.is_empty());
    while let Some(rec) = records.next() {
        let code = |xy: &str| {
            let mut c = xy.chars();
            [c.next().unwrap_or('.'), c.next().unwrap_or('.')]
        };
        match rec.as_bytes()[0] {
            b'#' => {
                if let Some(head) = rec.strip_prefix("# branch.head ") {
                    status.branch = Some(head.to_string());
                } else if let Some(ab) = rec.strip_prefix("# branch.ab ") {
                    for n in ab.split(' ') {
                        if let Some(a) = n.strip_prefix('+') {
                            status.ahead = a.parse().unwrap_or(0);
                        } else if let Some(b) = n.strip_prefix('-') {
                            status.behind = b.parse().unwrap_or(0);
                        }
                    }
                }
            }
            b'1' | b'2' | b'u' => {
                let fields = match rec.as_bytes()[0] {
                    b'1' => 9,
                    b'2' => 10,
                    _ => 11,
                };
                let parts: Vec<&str> = rec.splitn(fields, ' ').collect();
                if let (Some(xy), Some(path)) = (parts.get(1), parts.last()) {
                    status.changes.push(Change { code: code(xy), path: path.to_string(), added: 0, removed: 0 });
                }
                if rec.starts_with('2') {
                    records.next(); // original path of a rename/copy
                }
            }
            b'?' => status.changes.push(Change { code: ['?', '?'], path: rec[2..].to_string(), added: 0, removed: 0 }),
            _ => {}
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_numstat() {
        let counts = parse_numstat("3\t1\tsrc/a.rs\0-\t-\timg.png\x000\t0\t\0old.rs\0new.rs\0");
        assert_eq!(counts["src/a.rs"], (3, 1));
        assert_eq!(counts["img.png"], (0, 0));
        assert_eq!(counts["new.rs"], (0, 0));
    }

    #[test]
    fn parses_branch_and_changes() {
        let out = "# branch.oid abc\0# branch.head main\0# branch.upstream origin/main\0# branch.ab +2 -1\0\
                   1 .M N... 100644 100644 100644 a a src/my file.rs\0\
                   1 A. N... 000000 100644 100644 0 b new.rs\0\
                   2 R. N... 100644 100644 100644 a a R100 renamed.rs\0old.rs\0\
                   ? untracked.txt\0";
        let s = parse(out);
        assert_eq!(s.branch.as_deref(), Some("main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        let got: Vec<(char, &str)> = s.changes.iter().map(|c| (c.kind(), c.path.as_str())).collect();
        assert_eq!(
            got,
            [('M', "src/my file.rs"), ('A', "new.rs"), ('R', "renamed.rs"), ('?', "untracked.txt")]
        );
    }
}
