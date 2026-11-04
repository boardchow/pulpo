use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::style::Color;
use ratatui::widgets::ListState;

use crate::git::{self, Commit, History};
use crate::theme;
use crate::tree::{self, Row};
use crate::viewer::{self, Kind, ReviewDiff};

pub struct File {
    pub path: PathBuf,
    /// git's status letter, or '?' for an untracked file.
    pub status: char,
}

pub struct Changes {
    pub root: PathBuf,
    /// What the files are compared with: HEAD, or the parent of the commit shown.
    against: String,
    /// Shown instead of what isn't committed.
    pub commit: Option<Commit>,
    /// The commit a workspace Pulpo made was copied at, where its history starts; otherwise each read finds it again.
    copied: Option<String>,
    /// Where the history starts, for its title.
    pub since: String,
    loaded: bool,
    /// Bumped by each read asked for, so an older one arriving late is dropped.
    generation: u64,
    pub files: Vec<File>,
    pub rows: Vec<Row>,
    folded: HashSet<PathBuf>,
    pub state: ListState,
    pub diff: Option<(PathBuf, ReviewDiff)>,
    pub scroll: usize,
    pub hscroll: usize,
    pub height: usize,
    pub history: History,
    pub history_state: ListState,
}

impl Changes {
    pub fn new(root: &Path, copied: Option<String>) -> Changes {
        Changes {
            root: root.to_path_buf(),
            against: "HEAD".into(),
            commit: None,
            since: if copied.is_some() { "since the copy".into() } else { String::new() },
            copied,
            loaded: false,
            generation: 0,
            files: Vec::new(),
            rows: Vec::new(),
            folded: HashSet::new(),
            state: ListState::default().with_selected(Some(0)),
            diff: None,
            scroll: 0,
            hscroll: 0,
            height: 20,
            history: History::default(),
            history_state: ListState::default().with_selected(Some(0)),
        }
    }

    pub fn of_commit(root: &Path, commit: &Commit) -> Changes {
        let mut changes = Changes::new(root, None);
        // The first commit has no parent: it's compared with the empty tree.
        let parent = git(root, &["rev-parse", "--verify", "--quiet", &format!("{}^", commit.hash)]).unwrap_or_else(|| EMPTY_TREE.into());
        changes.against = format!("{parent}..{}", commit.hash);
        changes.commit = Some(commit.clone());
        changes
    }

    #[cfg(test)]
    pub fn open(root: &Path, copied: Option<String>) -> Changes {
        let mut changes = Changes::new(root, copied);
        changes.load();
        changes
    }

    #[cfg(test)]
    fn load(&mut self) {
        let read = self.request().read();
        self.apply(read);
    }

    pub fn request(&mut self) -> Request {
        self.generation += 1;
        Request {
            root: self.root.clone(),
            against: self.against.clone(),
            copied: self.copied.clone(),
            commit: self.commit.is_some(),
            shown: self.diff.as_ref().map(|(p, _)| p.clone()),
            generation: self.generation,
        }
    }

    pub fn apply(&mut self, read: Read) {
        if read.root != self.root || read.against != self.against || read.generation != self.generation {
            return;
        }
        let first = !self.loaded || self.files.is_empty();
        self.loaded = true;
        let mut selected = self.selected().map(|r| r.path.clone());
        if first {
            selected = read.diff.as_ref().map(|(p, _)| p.clone()).or(selected);
        }
        let (was, scroll) = (self.diff.take().map(|(p, _)| p), self.scroll);
        (self.since, self.files, self.history) = (read.since, read.files, read.history);
        self.layout(selected);
        if let Some((path, diff)) = read.diff {
            self.scroll = if was.as_ref() == Some(&path) { scroll } else { diff.changes.first().map_or(0, |&c| c.saturating_sub(3)) };
            self.diff = Some((path, diff));
            self.scroll_by(0);
        }
        let selected_file = self.selected().filter(|r| !r.is_dir).map(|r| r.path.clone());
        if self.diff.as_ref().map(|(p, _)| p) != selected_file.as_ref() {
            self.diff = None;
            self.follow();
        }
    }

    fn layout(&mut self, selected: Option<PathBuf>) {
        let all = tree::of_paths(&self.root, self.files.iter().map(|f| f.path.to_str().unwrap_or_default()));
        self.rows = all.into_iter().filter(|r| !r.path.ancestors().skip(1).any(|a| self.folded.contains(a))).collect();
        for row in &mut self.rows {
            row.expanded = row.is_dir && !self.folded.contains(&row.path);
        }
        let at = selected.and_then(|p| self.rows.iter().position(|r| r.path == p));
        let at = at.unwrap_or_else(|| self.state.selected().unwrap_or(0));
        self.state.select(Some(at.min(self.rows.len().saturating_sub(1))));
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.state.selected()?)
    }

    pub fn follow(&mut self) {
        match self.selected().filter(|r| !r.is_dir).map(|r| r.path.clone()) {
            Some(path) => self.show(&path),
            None => self.diff = None,
        }
    }

    fn show(&mut self, path: &Path) {
        if self.diff.as_ref().is_some_and(|(p, _)| p == path) {
            return;
        }
        let untracked = self.files.iter().any(|f| self.root.join(&f.path) == path && f.status == '?');
        let diff = viewer::review_diff(&self.root, &self.against, path, untracked);
        self.scroll = diff.changes.first().map_or(0, |&c| c.saturating_sub(3));
        self.hscroll = 0;
        self.diff = Some((path.to_path_buf(), diff));
    }

    pub fn status_of(&self, path: &Path) -> Option<char> {
        self.files.iter().find(|f| self.root.join(&f.path).starts_with(path)).map(|f| f.status)
    }

    pub fn step(&mut self, delta: isize) {
        let max = self.rows.len().saturating_sub(1);
        let next = self.state.selected().unwrap_or(0).saturating_add_signed(delta).min(max);
        self.state.select(Some(next));
        self.follow();
    }

    pub fn fold(&mut self, open: bool) {
        let Some(row) = self.selected() else { return };
        let (path, is_dir) = (row.path.clone(), row.is_dir);
        if is_dir && self.folded.contains(&path) == open {
            if open {
                self.folded.remove(&path);
            } else {
                self.folded.insert(path.clone());
            }
            self.layout(Some(path));
        } else if !open && let Some(parent) = path.parent().filter(|p| *p != self.root) {
            let parent = parent.to_path_buf();
            self.layout(Some(parent));
        }
        self.follow();
    }

    pub fn scroll_sideways(&mut self, columns: isize) {
        let widest = self.diff.as_ref().map_or(0, |(_, d)| d.widest);
        self.hscroll = if columns == 0 { 0 } else { self.hscroll.saturating_add_signed(columns).min(widest) };
    }

    pub fn ruler(&self, cells: usize) -> Vec<(Option<Color>, bool)> {
        let Some((_, diff)) = &self.diff else { return Vec::new() };
        let lines = diff.kinds.len();
        (0..cells)
            .map(|cell| {
                let (start, end) = cell_lines(cell, cells, lines);
                let in_view = start < self.scroll + self.height && end > self.scroll;
                let kinds = diff.kinds.get(start..end).unwrap_or_default();
                let color = match (kinds.contains(&Kind::Added), kinds.contains(&Kind::Removed)) {
                    (true, true) => Some(theme::blue()),
                    (true, false) => Some(theme::green()),
                    (false, true) => Some(theme::red()),
                    _ => None,
                };
                (color, in_view)
            })
            .collect()
    }

    pub fn jump_to(&mut self, cell: usize, cells: usize) {
        let lines = self.diff.as_ref().map_or(0, |(_, d)| d.kinds.len());
        let (start, _) = cell_lines(cell, cells, lines);
        self.scroll = start.saturating_sub(self.height / 3);
        self.scroll_by(0);
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let len = self.diff.as_ref().map_or(0, |(_, d)| d.lines.len());
        self.scroll = self.scroll.saturating_add_signed(rows).min(len.saturating_sub(self.height.max(1)));
    }
}

pub struct Request {
    root: PathBuf,
    against: String,
    copied: Option<String>,
    commit: bool,
    shown: Option<PathBuf>,
    generation: u64,
}

pub struct Read {
    root: PathBuf,
    against: String,
    since: String,
    files: Vec<File>,
    diff: Option<(PathBuf, ReviewDiff)>,
    history: History,
    generation: u64,
}

impl Request {
    pub fn read(self) -> Read {
        let files: Vec<File> = changed(&self.root, &self.against, !self.commit).into_iter().map(|(path, status)| File { path, status }).collect();
        let shown = self.shown.or_else(|| {
            let rows = tree::of_paths(&self.root, files.iter().map(|f| f.path.to_str().unwrap_or_default()));
            rows.into_iter().find(|r| !r.is_dir).map(|r| r.path)
        });
        let diff = shown.map(|path| {
            let untracked = files.iter().any(|f| self.root.join(&f.path) == path && f.status == '?');
            let diff = viewer::review_diff(&self.root, &self.against, &path, untracked);
            (path, diff)
        });
        let (history, since) = if self.commit {
            (History::default(), String::new())
        } else {
            let (base, since) = self.copied.map(|c| (c, "since the copy".to_string())).unwrap_or_else(|| self::base(&self.root));
            (git::history(&self.root, &base), since)
        };
        Read { root: self.root, against: self.against, since, files, diff, history, generation: self.generation }
    }
}

fn cell_lines(cell: usize, cells: usize, lines: usize) -> (usize, usize) {
    let start = cell * lines / cells.max(1);
    (start, ((cell + 1) * lines / cells.max(1)).max(start + 1).min(lines))
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").current_dir(root).args(["--no-optional-locks"]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Where a branch left the main one; the main branch itself is HEAD.
fn base(root: &Path) -> (String, String) {
    let uncommitted = || ("HEAD".to_string(), String::new());
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let main = git(root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .or_else(|| ["main", "master"].iter().find(|b| git(root, &["rev-parse", "--verify", "--quiet", b]).is_some()).map(|b| b.to_string()));
    let Some(main) = main.filter(|m| m.trim_start_matches("origin/") != branch) else { return uncommitted() };
    match git(root, &["merge-base", "HEAD", &main]) {
        Some(commit) => (commit, format!("since {main}")),
        None => uncommitted(),
    }
}

fn changed(root: &Path, against: &str, untracked: bool) -> Vec<(PathBuf, char)> {
    let mut files: Vec<(PathBuf, char)> = Vec::new();
    if let Some(out) = git(root, &["diff", "--name-status", "--no-renames", "-z", against]) {
        let mut parts = out.split('\0').filter(|p| !p.is_empty());
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            files.push((PathBuf::from(path), status.chars().next().unwrap_or('M')));
        }
    }
    if let Some(out) = git(root, &["ls-files", "--others", "--exclude-standard", "-z"]).filter(|_| untracked) {
        files.extend(out.split('\0').filter(|p| !p.is_empty()).map(|p| (PathBuf::from(p), '?')));
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        let t = std::env::temp_dir().join(format!("pulpo-review-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("src")).unwrap();
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(t.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(t.join("src/b.txt"), "b\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "start"]);
        run(&["checkout", "-qb", "task"]);
        std::fs::write(t.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        run(&["commit", "-qam", "agent"]);
        std::fs::write(t.join("src/b.txt"), "b\nmore\n").unwrap();
        std::fs::write(t.join("c.txt"), "new\n").unwrap();
        t
    }

    #[test]
    fn shows_what_isnt_committed_as_a_tree() {
        let t = repo();
        let mut changes = Changes::open(&t, None);
        assert_eq!(changes.since, "since main");
        let rows: Vec<String> = changes.rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(rows, ["src", "b.txt", "c.txt"]);
        assert_eq!(changes.status_of(&t.join("src")), Some('M'));
        assert_eq!(changes.status_of(&t.join("c.txt")), Some('?'));
        assert_eq!(changes.selected().map(|r| r.name.as_str()), Some("b.txt"));
        assert_eq!(changes.diff.as_ref().map(|(p, d)| (p.clone(), d.added)), Some((t.join("src/b.txt"), 1)));
        changes.step(-1);
        assert!(changes.diff.is_none());
        changes.step(1);
        changes.height = 10;
        let marks: Vec<Option<ratatui::style::Color>> = changes.ruler(4).into_iter().map(|(color, _)| color).collect();
        assert_eq!(marks, [None, None, Some(theme::green()), Some(theme::green())]);
        assert!(changes.ruler(4).iter().all(|&(_, in_view)| in_view));
        assert_eq!(cell_lines(3, 4, 2), (1, 2));
        changes.scroll_sideways(100);
        assert_eq!(changes.hscroll, 4);
        changes.scroll_sideways(0);
        assert_eq!(changes.hscroll, 0);
        let stale = changes.request().read();
        let fresh = changes.request().read();
        changes.apply(stale);
        changes.apply(fresh);
        assert_eq!(changes.diff.as_ref().map(|(p, _)| p.clone()), Some(t.join("src/b.txt")));
        std::fs::write(t.join("src/b.txt"), "b\n").unwrap();
        let read = changes.request().read();
        changes.apply(read);
        assert_ne!(changes.diff.as_ref().map(|(p, _)| p.clone()), Some(t.join("src/b.txt")));
        std::fs::write(t.join("src/b.txt"), "b\nmore\n").unwrap();
        let read = changes.request().read();
        changes.apply(read);
        changes.state.select(Some(0));
        changes.fold(false);
        assert_eq!(changes.rows.len(), 2);
        changes.fold(true);
        assert_eq!(changes.rows.len(), 3);
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        run(&["stash", "-qu"]);
        run(&["checkout", "-q", "main"]);
        std::fs::write(t.join("a.txt"), "main\n").unwrap();
        let mut on_main = Changes::open(&t, None);
        assert_eq!((on_main.since.as_str(), on_main.files.len()), ("", 1));
        run(&["commit", "-qam", "done"]);
        on_main.load();
        assert!(on_main.files.is_empty());
        assert_eq!(on_main.history.commits.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["done", "start"]);
        run(&["checkout", "-q", "task"]);
        run(&["stash", "pop", "-q"]);
        let head = git(&t, &["rev-parse", "HEAD"]).unwrap();
        let changes = Changes::open(&t, Some(head));
        assert_eq!(changes.files.iter().map(|f| f.path.display().to_string()).collect::<Vec<_>>(), vec!["src/b.txt", "c.txt"]);
        assert!(changes.history.commits.is_empty());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn shows_a_commit_on_its_own() {
        let t = repo();
        let history = Changes::open(&t, None).history;
        let mut agent = Changes::of_commit(&t, &history.commits[0]);
        agent.load();
        assert_eq!(agent.files.iter().map(|f| (f.path.display().to_string(), f.status)).collect::<Vec<_>>(), [("a.txt".to_string(), 'M')]);
        assert_eq!(agent.diff.as_ref().map(|(_, d)| (d.added, d.removed)), Some((1, 1)));
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        run(&["checkout", "-q", "-f", "main"]);
        let mut start = Changes::of_commit(&t, &Changes::open(&t, None).history.commits[0]);
        start.load();
        assert_eq!(start.files.len(), 2);
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn history_marks_what_origin_has() {
        let t = repo();
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        let subjects = |c: &Changes| c.history.commits.iter().map(|c| (c.subject.clone(), c.pushed)).collect::<Vec<_>>();
        let changes = Changes::open(&t, None);
        assert_eq!((subjects(&changes), changes.history.remote.is_none()), (vec![("agent".to_string(), false)], true));
        run(&["update-ref", "refs/remotes/origin/task", "HEAD"]);
        run(&["commit", "-qam", "more"]);
        let changes = Changes::open(&t, None);
        assert_eq!(subjects(&changes), [("more".to_string(), false), ("agent".to_string(), true)]);
        run(&["update-ref", "refs/remotes/origin/task", "HEAD"]);
        run(&["reset", "-q", "HEAD~1"]);
        let changes = Changes::open(&t, None);
        assert_eq!((subjects(&changes), changes.history.behind), (vec![("agent".to_string(), true)], 1));
        run(&["checkout", "-q", "-f", "main"]);
        assert_eq!(subjects(&Changes::open(&t, None)), [("start".to_string(), false)]);
        std::fs::remove_dir_all(&t).unwrap();
    }
}
