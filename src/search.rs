use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use ignore::WalkBuilder;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};
use ratatui::widgets::ListState;

use crate::Msg;

const MAX_FILES: usize = 100_000;
const MAX_HITS: usize = 500;
const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Once something is typed, so what's further down still shows.
const MAX_PLACES: usize = 3;
const MAX_NAMES: usize = 10;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Everything,
    Repos,
}

/// In the order they're listed.
#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Project,
    Workspace,
    File,
    Text,
    Repo,
}

#[derive(Clone)]
pub struct Hit {
    pub kind: Kind,
    pub rel: String,
    /// 0-based line of a text match.
    pub line: Option<usize>,
    pub text: String,
    /// Where it goes: a workspace's folder, or the file.
    pub path: PathBuf,
}

pub enum Entry {
    Header(Kind),
    Hit(Hit),
}

pub struct Search {
    pub mode: Mode,
    pub query: String,
    pub rows: Vec<Entry>,
    pub state: ListState,
    pub busy: bool,
    pub root: PathBuf,
    places: Vec<Hit>,
    found: Vec<Hit>,
    files: Arc<Vec<String>>,
    scanning: bool,
    /// Bumped on every new text search so stale ones stop early.
    generation: Arc<AtomicUsize>,
}

impl Search {
    /// Everything: `places` are the projects and workspaces, `root` the one whose files are searched.
    pub fn new(mode: Mode, root: PathBuf, places: Vec<Hit>, tx: &Sender<Msg>) -> Self {
        let scanning = mode == Mode::Repos;
        let files = if scanning {
            let (home, tx) = (root.clone(), tx.clone());
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Repos(find_repos(&home)));
            });
            Vec::new()
        } else {
            list_files(&root)
        };
        let mut search = Search {
            mode,
            query: String::new(),
            rows: Vec::new(),
            state: ListState::default(),
            busy: false,
            places,
            found: Vec::new(),
            files: Arc::new(files),
            scanning,
            root,
            generation: Arc::new(AtomicUsize::new(0)),
        };
        search.update(tx);
        search
    }

    pub fn set_repos(&mut self, repos: Vec<String>, tx: &Sender<Msg>) {
        if self.mode == Mode::Repos {
            self.files = Arc::new(repos);
            self.scanning = false;
            self.update(tx);
        }
    }

    pub fn update(&mut self, tx: &Sender<Msg>) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.busy = self.scanning;
        let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
        let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
        let fuzzy = |kind: Kind, names: &[String], max: usize| -> Vec<Hit> {
            let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
            pattern
                .match_list(names.iter(), &mut matcher)
                .into_iter()
                .take(max)
                .map(|(rel, _)| {
                    let path = if kind == Kind::File { self.root.join(rel) } else { PathBuf::new() };
                    Hit { kind, rel: rel.clone(), line: None, text: String::new(), path }
                })
                .collect()
        };
        self.found.clear();
        match self.mode {
            Mode::Repos => {
                self.found = fuzzy(Kind::Repo, &self.files, MAX_HITS);
                if self.query.starts_with('/') || self.query.starts_with('~') {
                    let typed = Hit { kind: Kind::Repo, rel: self.query.clone(), line: None, text: String::new(), path: expand_home(&self.query) };
                    self.found.insert(0, typed);
                }
            }
            Mode::Everything if self.query.is_empty() => self.found = self.places.clone(),
            Mode::Everything => {
                let mut buf = Vec::new();
                for kind in [Kind::Project, Kind::Workspace] {
                    let mut scored: Vec<(u32, &Hit)> = (self.places.iter().filter(|p| p.kind == kind))
                        .filter_map(|p| Some((pattern.score(Utf32Str::new(&p.rel, &mut buf), &mut matcher)?, p)))
                        .collect();
                    scored.sort_by_key(|&(score, _)| std::cmp::Reverse(score));
                    self.found.extend(scored.into_iter().take(MAX_PLACES).map(|(_, p)| p.clone()));
                }
                self.found.extend(fuzzy(Kind::File, &self.files, MAX_NAMES));
                if self.query.chars().count() >= 2 {
                    self.busy = true;
                    let (root, files, query, current, tx) =
                        (self.root.clone(), self.files.clone(), self.query.clone(), self.generation.clone(), tx.clone());
                    std::thread::spawn(move || {
                        let hits = grep(&root, &files, &query, || current.load(Ordering::Relaxed) != generation);
                        if let Some(hits) = hits {
                            let _ = tx.send(Msg::Search(generation, hits));
                        }
                    });
                }
            }
        }
        self.rebuild();
        self.state.select(None);
        self.step(0);
    }

    /// Text matches come last, so the rows above them stay where they were.
    pub fn on_results(&mut self, generation: usize, hits: Vec<Hit>) {
        if generation == self.generation.load(Ordering::Relaxed) {
            self.found.extend(hits);
            self.busy = false;
            self.rebuild();
            if self.state.selected().is_none() {
                self.step(0);
            }
        }
    }

    /// A header over each kind there is, none for repositories.
    fn rebuild(&mut self) {
        self.rows.clear();
        let mut last = None;
        for hit in &self.found {
            if hit.kind != Kind::Repo && last != Some(hit.kind) {
                self.rows.push(Entry::Header(hit.kind));
            }
            last = Some(hit.kind);
            self.rows.push(Entry::Hit(hit.clone()));
        }
    }

    pub fn rescan(&mut self, tx: &Sender<Msg>) {
        if self.mode != Mode::Repos {
            self.files = Arc::new(list_files(&self.root));
            self.update(tx);
        }
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }

    pub fn hit_count(&self) -> usize {
        self.found.len()
    }

    /// Headers are skipped, onward in the direction of travel.
    pub fn step(&mut self, delta: isize) {
        let hits: Vec<usize> = (0..self.rows.len()).filter(|&i| matches!(self.rows[i], Entry::Hit(_))).collect();
        let to = self.state.selected().map_or(0, |from| from.saturating_add_signed(delta));
        let at = if delta < 0 {
            hits.iter().rev().find(|&&i| i <= to).or(hits.first())
        } else {
            hits.iter().find(|&&i| i >= to).or(hits.last())
        };
        self.state.select(at.copied());
    }

    pub fn selected(&self) -> Option<&Hit> {
        match self.rows.get(self.state.selected()?)? {
            Entry::Hit(hit) => Some(hit),
            Entry::Header(_) => None,
        }
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        // Stop any search still running.
        self.generation.fetch_add(1, Ordering::Relaxed);
    }
}

fn list_files(root: &Path) -> Vec<String> {
    WalkBuilder::new(root)
        .hidden(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_some_and(|t| t.is_file()))
        .filter_map(|e| e.path().strip_prefix(root).ok().map(|p| p.to_string_lossy().into_owned()))
        .take(MAX_FILES)
        .collect()
}

pub fn expand_home(path: &str) -> PathBuf {
    match path.strip_prefix("~/") {
        Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
        None => PathBuf::from(path),
    }
}

fn find_repos(home: &Path) -> Vec<String> {
    const SKIP: &[&str] =
        &["Library", "Applications", "Pictures", "Music", "Movies", "node_modules", "target", "vendor", "dist", "build"];
    const MAX_DEPTH: usize = 5;
    let registered = crate::project::registered();
    let mut found = Vec::new();
    let mut stack = vec![(home.to_path_buf(), 0)];
    while let Some((dir, depth)) = stack.pop() {
        // Linked worktrees and submodules have a .git file; only count real repositories.
        if depth > 0 && dir.join(".git").is_dir() {
            if !registered.contains(&dir) {
                found.push(dir);
            }
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if depth < MAX_DEPTH
                && !name.starts_with('.')
                && !SKIP.contains(&name.as_ref())
                && entry.file_type().is_ok_and(|t| t.is_dir())
            {
                stack.push((entry.path(), depth + 1));
            }
        }
    }
    found.sort();
    found
        .iter()
        .map(|p| match p.strip_prefix(home) {
            Ok(rel) => format!("~/{}", rel.display()),
            Err(_) => p.display().to_string(),
        })
        .collect()
}

fn grep(root: &Path, files: &[String], query: &str, cancelled: impl Fn() -> bool) -> Option<Vec<Hit>> {
    let ignore_case = !query.chars().any(char::is_uppercase);
    let needle = if ignore_case { query.to_lowercase() } else { query.to_string() };
    let mut hits = Vec::new();
    for rel in files {
        if cancelled() {
            return None;
        }
        let path = root.join(rel);
        if std::fs::metadata(&path).map_or(true, |m| m.len() > MAX_FILE_BYTES) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        if bytes[..bytes.len().min(8000)].contains(&0) {
            continue;
        }
        for (i, line) in String::from_utf8_lossy(&bytes).lines().enumerate() {
            let found = if ignore_case { line.to_lowercase().contains(&needle) } else { line.contains(&needle) };
            if found {
                let text: String = line.trim().chars().take(200).collect();
                hits.push(Hit { kind: Kind::Text, rel: rel.clone(), line: Some(i), text, path: path.clone() });
                if hits.len() >= MAX_HITS {
                    return Some(hits);
                }
            }
        }
    }
    Some(hits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn greps_with_smart_case() {
        let dir = std::env::temp_dir().join(format!("pulpo-grep-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "Hello world\nnothing\nhello again\n").unwrap();
        let files = vec!["a.txt".to_string()];
        let lines = |q: &str| grep(&dir, &files, q, || false).unwrap().iter().map(|h| h.line.unwrap()).collect::<Vec<_>>();
        assert_eq!(lines("hello"), [0, 2]);
        assert_eq!(lines("Hello"), [0]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn lists_places_before_files_and_steps_over_headers() {
        let dir = std::env::temp_dir().join(format!("pulpo-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pulpo.md"), "").unwrap();
        let place = |kind, name: &str| Hit { kind, rel: name.into(), line: None, text: String::new(), path: PathBuf::new() };
        let places = vec![place(Kind::Project, "pulpo"), place(Kind::Project, "other"), place(Kind::Workspace, "ui-polish")];
        let (tx, _rx) = std::sync::mpsc::channel();
        let mut search = Search::new(Mode::Everything, dir.clone(), places, &tx);
        search.query = "pulp".into();
        search.update(&tx);
        let rows: Vec<String> = search
            .rows
            .iter()
            .map(|r| match r {
                Entry::Header(_) => "#".into(),
                Entry::Hit(h) => h.rel.clone(),
            })
            .collect();
        assert_eq!(rows, ["#", "pulpo", "#", "pulpo.md"]);
        assert_eq!(search.selected().unwrap().rel, "pulpo");
        search.step(1);
        assert_eq!(search.selected().unwrap().rel, "pulpo.md");
        search.step(-1);
        assert_eq!(search.selected().unwrap().rel, "pulpo");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
