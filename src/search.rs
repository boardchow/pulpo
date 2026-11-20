use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::Sender;

use ignore::WalkBuilder;
use nucleo_matcher::pattern::{CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher};
use ratatui::widgets::ListState;

use crate::Msg;

const MAX_FILES: usize = 100_000;
const MAX_HITS: usize = 500;
const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Copy, PartialEq)]
pub enum Mode {
    Files,
    Text,
    Repos,
}

#[derive(Clone)]
pub struct Hit {
    pub rel: String,
    /// 0-based line of a text match.
    pub line: Option<usize>,
    pub text: String,
    pub path: PathBuf,
}

pub struct Search {
    pub mode: Mode,
    pub query: String,
    pub rows: Vec<Hit>,
    pub state: ListState,
    pub busy: bool,
    pub root: PathBuf,
    files: Arc<Vec<String>>,
    scanning: bool,
    /// Bumped on every new text search so stale ones stop early.
    generation: Arc<AtomicUsize>,
}

impl Search {
    pub fn new(mode: Mode, root: PathBuf, tx: &Sender<Msg>) -> Self {
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
            files: Arc::new(files),
            scanning,
            root,
            generation: Arc::new(AtomicUsize::new(0)),
        };
        search.update(tx);
        search
    }

    pub fn repos(home: PathBuf, projects: Vec<String>, tx: &Sender<Msg>) -> Self {
        let mut search = Search::new(Mode::Repos, home, tx);
        search.files = Arc::new(projects);
        search.update(tx);
        search
    }

    pub fn set_repos(&mut self, found: Vec<String>, tx: &Sender<Msg>) {
        if self.mode == Mode::Repos && self.scanning {
            self.files = Arc::new(self.files.iter().cloned().chain(found).collect());
            self.scanning = false;
            self.update(tx);
        }
    }

    pub fn toggle(&mut self, tx: &Sender<Msg>) {
        self.mode = if self.mode == Mode::Files { Mode::Text } else { Mode::Files };
        self.rows.clear();
        self.update(tx);
    }

    pub fn update(&mut self, tx: &Sender<Msg>) {
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        self.busy = self.scanning;
        match self.mode {
            Mode::Files if self.query.is_empty() => self.rows.clear(),
            Mode::Files | Mode::Repos => {
                let mut matcher = Matcher::new(Config::DEFAULT.match_paths());
                let pattern = Pattern::parse(&self.query, CaseMatching::Smart, Normalization::Smart);
                let root = if self.mode == Mode::Files { self.root.as_path() } else { Path::new("") };
                self.rows = (pattern.match_list(self.files.iter(), &mut matcher).into_iter().take(MAX_HITS))
                    .map(|(rel, _)| Hit { rel: rel.clone(), line: None, text: String::new(), path: root.join(rel) })
                    .collect();
                if self.mode == Mode::Repos && (self.query.starts_with('/') || self.query.starts_with('~')) {
                    let typed = Hit { rel: self.query.clone(), line: None, text: String::new(), path: expand_home(&self.query) };
                    self.rows.insert(0, typed);
                }
            }
            Mode::Text if self.query.chars().count() < 2 => self.rows.clear(),
            // Kept until the new results come, so the list doesn't blink.
            Mode::Text => {
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
        if !(self.mode == Mode::Text && self.busy) {
            self.state.select((!self.rows.is_empty()).then_some(0));
        }
    }

    pub fn on_results(&mut self, generation: usize, hits: Vec<Hit>) {
        if generation == self.generation.load(Ordering::Relaxed) {
            self.rows = hits;
            self.busy = false;
            self.state.select((!self.rows.is_empty()).then_some(0));
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

    pub fn step(&mut self, delta: isize) {
        let last = self.rows.len().saturating_sub(1);
        self.state.select((!self.rows.is_empty()).then(|| self.state.selected().unwrap_or(0).saturating_add_signed(delta).min(last)));
    }

    pub fn selected(&self) -> Option<&Hit> {
        self.rows.get(self.state.selected()?)
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
                hits.push(Hit { rel: rel.clone(), line: Some(i), text, path: path.clone() });
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
    fn tab_switches_between_names_and_text() {
        let dir = std::env::temp_dir().join(format!("pulpo-search-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("pulpo.md"), "an octopus\n").unwrap();
        std::fs::write(dir.join("notes.txt"), "pulpo\n").unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let mut search = Search::new(Mode::Files, dir.clone(), &tx);
        search.query = "pulp".into();
        search.update(&tx);
        assert_eq!(search.rows.iter().map(|h| h.rel.as_str()).collect::<Vec<_>>(), ["pulpo.md"]);
        search.toggle(&tx);
        assert!(search.rows.is_empty() && search.busy);
        let Ok(Msg::Search(generation, hits)) = rx.recv_timeout(std::time::Duration::from_secs(10)) else { panic!() };
        search.on_results(generation, hits);
        let hit = search.selected().unwrap();
        assert_eq!((hit.rel.as_str(), hit.line), ("notes.txt", Some(0)));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
