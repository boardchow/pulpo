use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::ListState;

use crate::Msg;
use crate::agents::{Agents, State};
use crate::client::{self, Link};
use crate::editor::Editor;
use crate::git;
use crate::input;
use crate::keymap::{Key, Keymap};
use crate::layout::{self, Layout};
use crate::proto::{Info, PROTOCOL, ToClient, ToServer};
use crate::review::Changes;
use crate::search::{Hit, Kind, Mode, Search, expand_home};
use crate::worktree;
use crate::status::Status;
use crate::term::Term;
use crate::viewer::{Source, Viewer};
use crate::project;
use crate::workspace::Workspace;

const NOTICE_EVERY: Duration = Duration::from_secs(10);
const MESSAGE_FOR: Duration = Duration::from_secs(5);
const UNSAVED: &str = "unsaved changes in the editor: ⌃s saves them, esc twice throws them away";
const PREFIX_FOR: Duration = Duration::from_secs(3);
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Another workspace only shows how many changes it has: its `git status` can wait this long.
const OTHERS_STATUS: Duration = Duration::from_secs(3);
/// How long a terminal just switched to may wait for its snapshot before it's drawn empty.
const SNAPSHOT: Duration = Duration::from_millis(100);
/// A resumed agent reprints its conversation meanwhile: that isn't news.
const RESUMING: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Focus {
    Projects,
    Workspaces,
    Terminal,
    Files,
    Changes,
    History,
}

pub enum Ask {
    /// Which of `projects` the workspace is made in.
    Project(usize),
    /// The text is the name; this, the branch (empty: from the name).
    Workspace(String),
    /// The text is the branch; this, the name.
    Branch(String),
    File(PathBuf),
    Rename(PathBuf),
}

pub struct Prompt {
    pub ask: Ask,
    pub text: String,
}

pub enum Confirm {
    RemoveProject(PathBuf),
    DeleteWorkspace(PathBuf),
    CloseTerminal(usize),
    DeleteFile(PathBuf),
    StopAll,
}

#[derive(Default, Clone)]
pub struct Areas {
    pub projects: Rect,
    pub workspaces: Rect,
    pub tabs: Vec<(Option<usize>, Rect)>,
    pub term: Rect,
    pub files_tree: Rect,
    pub files_file: Rect,
    pub changes_tree: Rect,
    pub changes_file: Rect,
    pub changes_ruler: Rect,
    pub history: Rect,
    pub editor: Rect,
    pub left_divider: Rect,
    pub divider: Rect,
    pub tree_dividers: Vec<(Rect, u16)>,
    pub yes: Rect,
    pub popup: Rect,
    pub popup_list: Rect,
    pub width: u16,
}

#[derive(Clone, Copy)]
enum Drag {
    Sidebar,
    Right,
    Tree(u16),
}

pub struct Selecting {
    pub id: usize,
    pub from: (u16, u16),
    pub to: (u16, u16),
    pub dragging: bool,
}

pub struct App {
    pub workspaces: Vec<Workspace>,
    pub worktrees: Vec<worktree::Worktree>,
    pub current: usize,
    pub projects_state: ListState,
    pub workspaces_state: ListState,
    pub no_projects: bool,
    pub focus: Focus,
    pub agents: Agents,
    pub terms: Vec<Term>,
    pub active: Option<usize>,
    pub prefixed: bool,
    prefixed_at: Option<Instant>,
    pub quit: bool,
    pub help: bool,
    pub left_hidden: bool,
    pub right_hidden: bool,
    pub prompt: Option<Prompt>,
    pub asking: Option<(String, Confirm)>,
    pub message: Option<String>,
    message_since: Option<(String, Instant)>,
    pub search: Option<Search>,
    pub repos: Option<Search>,
    pub viewer: Viewer,
    pub editor: Option<Editor>,
    discard: bool,
    pub files_expanded: bool,
    pub changes_expanded: bool,
    pub reading: bool,
    pub changes: Option<Changes>,
    /// The workspace's changes, while a commit is shown in their place.
    shelved: Option<Changes>,
    pub left_width: Option<u16>,
    pub right_width: Option<u16>,
    pub tree_width: Option<u16>,
    dragging: Option<Drag>,
    pub selecting: Option<Selecting>,
    last_click: Option<(Position, Instant)>,
    pub keymap: Keymap,
    pub areas: Areas,
    pub setting_up: HashMap<PathBuf, u32>,
    /// Whether each worked, until you go there.
    setup_ended: HashMap<PathBuf, bool>,
    copying: HashSet<(PathBuf, String)>,
    removing: HashSet<PathBuf>,
    gone: HashSet<PathBuf>,
    last_term: HashMap<PathBuf, String>,
    last_workspace: HashMap<PathBuf, PathBuf>,
    saved: Layout,
    ready: bool,
    /// The tag each reopened terminal had, and whether it was waiting for you.
    reopening: HashMap<u64, (String, bool)>,
    resumed: HashMap<usize, Instant>,
    link: Link,
    /// Terminals asked for and not attached yet: whether to focus them, and where.
    spawning: HashMap<u64, (bool, PathBuf)>,
    /// The shell a setup that worked hands its tab's place to.
    handing_over: HashSet<u64>,
    /// The project a new workspace is made in.
    pub new_in: PathBuf,
    /// When each folder last got a shell of its own, so one that exits at once isn't started again and again.
    kept: HashMap<PathBuf, Instant>,
    /// The Workspaces list, top first, and each row's urgency when last placed.
    order: Vec<PathBuf>,
    placed: HashMap<PathBuf, u8>,
    next_token: u64,
    upgrade_asked: bool,
    pub release: Option<String>,
    pub updating: bool,
    /// The terminal the server sends output for: the one on screen.
    watching: Option<usize>,
    watched_at: Instant,
    reconnected: Option<usize>,
    last_reconnect: Option<Instant>,
    offline: bool,
    term_size: (u16, u16),
    fs_dirty: HashSet<PathBuf>,
    /// Other workspaces changed since their last `git status`.
    stale: HashSet<PathBuf>,
    others_read: Instant,
    reload_list: bool,
    watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
    tx: Sender<Msg>,
}

impl App {
    pub fn new(launch: Option<PathBuf>, tx: Sender<Msg>, link: Link) -> anyhow::Result<Self> {
        let fs_tx = tx.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = fs_tx.send(Msg::Fs(event.paths));
            }
        })?;
        let mut agents = Agents::default();
        agents.load();
        let layout = layout::load();
        let (keymap, keys_error) = Keymap::load();
        let mut app = App {
            workspaces: Vec::new(),
            worktrees: Vec::new(),
            current: 0,
            projects_state: ListState::default().with_selected(Some(0)),
            workspaces_state: ListState::default().with_selected(Some(0)),
            no_projects: false,
            focus: Focus::Terminal,
            agents,
            terms: Vec::new(),
            active: None,
            prefixed: false,
            prefixed_at: None,
            quit: false,
            help: false,
            left_hidden: false,
            right_hidden: false,
            prompt: None,
            asking: None,
            message: keys_error.or_else(|| {
                (!crate::agents::hooks_installed())
                    .then(|| "run `pulpo hooks install` to see what agents run and ask in each terminal".into())
            }),
            message_since: None,
            search: None,
            repos: None,
            viewer: Viewer::default(),
            editor: None,
            discard: false,
            files_expanded: false,
            changes_expanded: false,
            reading: false,
            changes: None,
            shelved: None,
            left_width: None,
            right_width: None,
            tree_width: None,
            dragging: None,
            selecting: None,
            last_click: None,
            keymap,
            areas: Areas::default(),
            setting_up: HashMap::new(),
            setup_ended: HashMap::new(),
            copying: HashSet::new(),
            removing: HashSet::new(),
            gone: HashSet::new(),
            last_term: layout.terminals.clone().into_iter().collect(),
            last_workspace: layout.workspaces.clone().into_iter().collect(),
            saved: layout.clone(),
            ready: false,
            reopening: HashMap::new(),
            resumed: HashMap::new(),
            link,
            spawning: HashMap::new(),
            handing_over: HashSet::new(),
            new_in: PathBuf::new(),
            kept: HashMap::new(),
            order: Vec::new(),
            placed: HashMap::new(),
            // Far above any terminal id, so the two never mix.
            next_token: 1 << 40,
            upgrade_asked: false,
            release: None,
            updating: false,
            watching: None,
            watched_at: Instant::now(),
            reconnected: None,
            last_reconnect: None,
            offline: false,
            term_size: (24, 80),
            fs_dirty: HashSet::new(),
            stale: HashSet::new(),
            others_read: Instant::now(),
            reload_list: false,
            watcher,
            watched: HashSet::new(),
            tx,
        };
        app.load_workspaces();
        let at = |root: &Path| app.workspaces.iter().position(|w| w.root == root);
        let start = match &launch {
            Some(dir) => at(dir).map(|i| match &app.workspaces[i] {
                w if !w.is_worktree() => app.last_workspace.get(&w.repo).and_then(|s| at(s)).unwrap_or(i),
                _ => i,
            }),
            None => layout.current.as_deref().and_then(at),
        };
        app.current = usize::MAX;
        app.set_current(start.unwrap_or(0));
        Ok(app)
    }

    pub fn ws(&self) -> &Workspace {
        &self.workspaces[self.current]
    }

    fn ws_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.current]
    }

    fn load_workspaces(&mut self) {
        let (current, repo) = self.workspaces.get(self.current).map(|w| (w.root.clone(), w.repo.clone())).unzip();
        let mut old = std::mem::take(&mut self.workspaces);
        let known: HashSet<PathBuf> = old.iter().map(|w| w.root.clone()).collect();
        let mut reuse = |root: PathBuf, repo: &Path| match old.iter().position(|w| w.root == root) {
            Some(i) => old.swap_remove(i),
            None => Workspace::new(root, repo.to_path_buf()),
        };
        let worktrees = worktree::all();
        let gone: Vec<worktree::Worktree> = worktrees
            .iter()
            .filter(|s| s.repo.is_dir() && !s.root.exists() && !self.removing.contains(&s.root) && !self.gone.contains(&s.root))
            .cloned()
            .collect();
        self.worktrees.clear();
        for repo in project::registered().into_iter().filter(|r| r.is_dir()) {
            // Not other worktrees: agents and tools make their own, for a while.
            self.workspaces.push(reuse(repo.clone(), &repo));
            for s in worktrees.iter().filter(|s| s.repo == repo && s.root.is_dir() && !self.removing.contains(&s.root)) {
                self.workspaces.push(reuse(s.root.clone(), &repo));
                self.worktrees.push(s.clone());
            }
            let making: Vec<PathBuf> = self.copying.iter().filter(|(r, _)| *r == repo).map(|(r, name)| worktree::root_of(r, name)).collect();
            for root in making {
                if !self.workspaces.iter().any(|w| w.root == root) {
                    self.workspaces.push(reuse(root, &repo));
                }
            }
        }
        // All marked before any goes: each removal lists them again.
        self.gone.extend(gone.iter().map(|s| s.root.clone()));
        self.no_projects = self.workspaces.is_empty();
        if self.no_projects {
            // No folder, so nothing is watched or run until a project is added.
            self.workspaces.push(reuse(PathBuf::new(), Path::new("")));
            if self.repos.is_none() {
                self.open_repo_picker();
            }
        }
        for i in 0..self.workspaces.len() {
            if !known.contains(&self.workspaces[i].root) {
                self.read_status(i);
            }
        }
        match current.as_ref().map(|c| self.workspaces.iter().position(|w| &w.root == c)) {
            None => self.current = self.current.min(self.workspaces.len() - 1),
            Some(Some(i)) => self.current = i,
            Some(None) => {
                let i = repo.and_then(|r| self.workspaces.iter().position(|w| w.repo == r)).unwrap_or(0);
                self.current = usize::MAX;
                self.set_current(i);
            }
        }
        self.sync_lists();
        self.sync_watches();
        // Once the list and the current workspace are settled.
        for s in gone {
            self.remove(s, false);
        }
    }

    fn sync_watches(&mut self) {
        let wanted: HashSet<PathBuf> = self.workspaces.iter().map(|w| w.root.clone()).collect();
        for path in self.watched.difference(&wanted) {
            let _ = self.watcher.unwatch(path);
        }
        self.watched.retain(|path| wanted.contains(path));
        // Only once there: a workspace is listed before its folder is made.
        for path in wanted {
            if !self.watched.contains(&path) && self.watcher.watch(&path, RecursiveMode::Recursive).is_ok() {
                self.watched.insert(path);
            }
        }
    }

    fn set_current(&mut self, i: usize) -> bool {
        if i >= self.workspaces.len() {
            return false;
        }
        if i != self.current {
            // The workspace on screen is still there to keep its editor.
            if self.current < self.workspaces.len() && self.editing() {
                self.message = Some(UNSAVED.into());
                self.sync_lists();
                return false;
            }
            self.current = i;
            self.editor = None;
            self.viewer.close();
            self.search = None;
            self.files_expanded = false;
            self.changes_expanded = false;
            self.selecting = None;
            self.workspaces[i].refresh();
            self.stale.remove(&self.workspaces[i].root);
            if !self.workspaces[i].reading_status() {
                self.read_status(i);
            }
            let base = self.worktrees.iter().find(|s| s.root == self.workspaces[i].root).and_then(|s| s.base.clone());
            self.shelved = None;
            self.changes = Some(Changes::new(&self.workspaces[i].root, base));
            self.reload_changes();
            self.restore_term();
            if self.setup_ended.remove(&self.workspaces[i].root) == Some(false) {
                if let Some(id) = self.workspace_terms().into_iter().map(|t| &self.terms[t]).find(|t| is_setup_tab(t)).map(|t| t.id) {
                    self.select_term(id);
                }
                self.message = Some("this workspace's setup failed: its setup tab shows why".into());
            }
            self.keep_a_terminal();
            self.preview();
        }
        let ws = &self.workspaces[i];
        self.last_workspace.insert(ws.repo.clone(), ws.root.clone());
        self.sync_lists();
        true
    }

    fn editing(&self) -> bool {
        self.editor.as_ref().is_some_and(|e| e.dirty)
    }

    pub fn projects(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for (i, w) in self.workspaces.iter().enumerate() {
            if !out.iter().any(|&o| self.workspaces[o].repo == w.repo) {
                out.push(i);
            }
        }
        out.sort_by_key(|&i| self.workspaces[i].name.to_lowercase());
        out
    }

    /// The workspaces Pulpo made, as `reorder` last left them; a project's own folder is its row in Projects.
    pub fn workspace_order(&self) -> Vec<usize> {
        let mut all: Vec<usize> = self.order.iter().filter_map(|root| self.workspaces.iter().position(|w| &w.root == root)).collect();
        let unplaced: Vec<usize> = (0..self.workspaces.len()).filter(|&i| self.workspaces[i].is_worktree() && !all.contains(&i)).collect();
        all.extend(unplaced);
        all
    }

    /// A workspace goes up when it starts waiting or working, and stays put when it calms down.
    pub fn reorder(&mut self) {
        let urgency: HashMap<PathBuf, u8> = (0..self.workspaces.len())
            .filter(|&i| self.workspaces[i].is_worktree())
            .map(|i| (self.workspaces[i].root.clone(), self.workspace_state(i).map_or(0, State::urgency)))
            .collect();
        self.order.retain(|root| urgency.contains_key(root));
        self.placed.retain(|root, _| urgency.contains_key(root));
        for i in 0..self.workspaces.len() {
            let root = &self.workspaces[i].root;
            let Some(&now) = urgency.get(root) else { continue };
            if self.placed.insert(root.clone(), now).is_none_or(|was| was < now) || !self.order.contains(root) {
                rise(&mut self.order, root.clone(), |r| urgency[r]);
            }
        }
    }

    pub fn place(&self, dir: &Path) -> Option<String> {
        let ws = self.workspaces.iter().find(|w| w.root == dir)?;
        let repo = self.workspaces.iter().find(|w| w.repo == ws.repo && !w.is_worktree()).map_or(&ws.name, |r| &r.name);
        Some(if ws.is_worktree() { format!("{repo}/{}", ws.name) } else { repo.clone() })
    }

    fn sync_lists(&mut self) {
        let repo = &self.ws().repo;
        let in_repos = self.projects().iter().position(|&i| &self.workspaces[i].repo == repo);
        let in_workspaces = self.workspace_order().iter().position(|&i| i == self.current);
        self.projects_state.select(in_repos);
        self.workspaces_state.select(in_workspaces);
    }

    pub fn on_fs(&mut self, paths: &[PathBuf]) {
        for path in paths {
            for w in &self.workspaces {
                // A build writes thousands of files: once a workspace is marked, skip matching its .gitignore.
                if !self.fs_dirty.contains(&w.root) && w.is_affected(path) {
                    self.fs_dirty.insert(w.root.clone());
                }
            }
            let added_or_removed = self.workspaces.iter().any(|w| {
                path.strip_prefix(w.repo.join(".git/worktrees")).is_ok_and(|rel| rel.components().count() == 1)
            });
            let worktree_gone = self.worktrees.iter().any(|s| path.starts_with(&s.root) && !s.root.exists());
            self.reload_list |= added_or_removed || worktree_gone;
        }
    }

    pub fn fs_pending(&self) -> bool {
        !self.fs_dirty.is_empty() || self.reload_list || (!self.stale.is_empty() && self.others_read.elapsed() >= OTHERS_STATUS)
    }

    pub fn flush_fs(&mut self) {
        if std::mem::take(&mut self.reload_list) {
            self.load_workspaces();
        }
        let dirty = std::mem::take(&mut self.fs_dirty);
        let current = self.ws().root.clone();
        self.stale.extend(dirty.iter().filter(|root| **root != current).cloned());
        if dirty.contains(&current) {
            self.read_status(self.current);
        }
        if !self.stale.is_empty() && self.others_read.elapsed() >= OTHERS_STATUS {
            self.others_read = Instant::now();
            let stale = std::mem::take(&mut self.stale);
            for i in 0..self.workspaces.len() {
                if stale.contains(&self.workspaces[i].root) {
                    self.read_status(i);
                }
            }
        }
        if dirty.contains(&self.ws().root) {
            // Only the current workspace's tree shows; another's is rebuilt when it becomes current.
            self.ws_mut().refresh();
            if let Some(next) = self.viewer.reload() {
                self.load_preview(next);
            }
            self.reload_changes();
            if let Some(search) = &mut self.search {
                search.rescan(&self.tx);
            }
            if let Some(editor) = &mut self.editor
                && editor.reload_if_changed()
            {
                self.message = Some(format!("{} changed on disk: reloaded", editor.path.display()));
            }
        }
    }

    fn read_status(&mut self, i: usize) {
        // Half checked out, everything would show as changed.
        if self.making(&self.workspaces[i].root) || !self.workspaces[i].begin_status() {
            return;
        }
        let (root, tx) = (self.workspaces[i].root.clone(), self.tx.clone());
        std::thread::spawn(move || {
            let status = git::status(&root);
            let _ = tx.send(Msg::Status(root, Box::new(status)));
        });
    }

    pub fn on_status(&mut self, root: PathBuf, status: git::Status) {
        let Some(i) = self.workspaces.iter().position(|w| w.root == root) else { return };
        if self.workspaces[i].end_status(status) {
            self.read_status(i);
        }
    }

    fn load_preview(&self, (path, stamp): (PathBuf, crate::viewer::Stamp)) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Viewed(Box::new(crate::viewer::load(path, stamp))));
        });
    }

    pub fn on_viewed(&mut self, loaded: crate::viewer::Loaded) {
        if let Some(next) = self.viewer.loaded(loaded) {
            self.load_preview(next);
        }
    }

    fn reload_changes(&mut self) {
        if self.making(&self.ws().root) {
            return;
        }
        let Some(changes) = &mut self.changes else { return };
        let (request, tx) = (changes.request(), self.tx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Changes(Box::new(request.read())));
        });
    }

    pub fn on_changes(&mut self, read: crate::review::Read) {
        if let Some(changes) = &mut self.changes {
            changes.apply(read);
        }
    }

    fn open_commit(&mut self) {
        let Some(changes) = &self.changes else { return };
        let Some(commit) = changes.history_state.selected().and_then(|i| changes.history.commits.get(i)) else { return };
        let commit = Changes::of_commit(&changes.root, commit);
        self.shelved = self.changes.replace(commit);
        (self.focus, self.changes_expanded, self.reading) = (Focus::Changes, true, false);
        self.reload_changes();
    }

    /// A commit is shown only full screen: leaving it brings the workspace's changes back.
    pub fn settle(&mut self) {
        if self.watching != self.active {
            if let Some(term) = self.watching.and_then(|id| self.terms.iter_mut().find(|t| t.id == id)) {
                term.unmirror();
            }
            self.watching = self.active;
            self.watched_at = Instant::now();
            self.link.send(&ToServer::Watch { id: self.active.map(|id| id as u32) });
        }
        if self.shelved.is_some() && !(self.focus == Focus::Changes && self.changes_expanded) {
            self.changes = self.shelved.take();
            if self.focus == Focus::Changes {
                self.focus = Focus::History;
            }
            self.reload_changes();
        }
    }

    fn add_repo(&mut self, input: &str, typed: bool) -> bool {
        let path = expand_home(input);
        if typed
            && path.is_absolute()
            && !path.exists()
            && let Err(e) = git::init(&path)
        {
            self.message = Some(e);
            if !path.exists() {
                return false;
            }
        }
        match git::toplevel(&path).and_then(|top| git::main_repo(&top)) {
            Some(repo) => {
                project::register(&repo);
                self.load_workspaces();
                let Some(i) = self.workspaces.iter().position(|w| w.root == repo) else {
                    self.message = Some("couldn't save the project list".into());
                    return false;
                };
                self.set_current(i);
                true
            }
            None => {
                self.message = Some(format!("not a git repository: {}", path.display()));
                false
            }
        }
    }

    fn spawn_in(&mut self, cwd: PathBuf, program: &str, args: &[String], focus: bool) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        let (rows, cols) = self.term_size;
        let env = self.worktrees.iter().find(|s| s.root == cwd).map(worktree::env).unwrap_or_default();
        self.spawning.insert(token, (focus, cwd.clone()));
        self.link.send(&ToServer::Spawn { token, program: program.into(), args: args.to_vec(), cwd, rows, cols, env });
        token
    }

    fn new_terminal(&mut self, focus: bool) {
        let cwd = self.ws().root.clone();
        self.spawn_in(cwd, &shell(), &[], focus);
    }

    /// The project or workspace on screen always has a terminal: a shell when its last one goes.
    fn keep_a_terminal(&mut self) {
        let root = self.ws().root.clone();
        let opening = self.spawning.values().any(|(_, cwd)| *cwd == root);
        let just_kept = self.kept.get(&root).is_some_and(|at| at.elapsed() < Duration::from_secs(1));
        if self.ready && root.is_dir() && !self.removing.contains(&root) && !opening && !just_kept && self.workspace_terms().is_empty() {
            self.kept.insert(root, Instant::now());
            self.new_terminal(false);
        }
    }

    fn reopen(&mut self) {
        let open = std::mem::take(&mut self.saved.open);
        let (mut terminals, mut agents) = (0, 0);
        for was in open.iter().filter(|o| o.cwd.is_dir()) {
            let args = layout::reopen(was, &shell());
            agents += usize::from(!args.is_empty());
            let token = self.spawn_in(was.cwd.clone(), &shell(), &args, false);
            self.reopening.insert(token, (was.tag.clone(), was.unread));
            terminals += 1;
        }
        if terminals > 0 {
            let agents = if agents > 0 { format!(", resuming {agents} agent session(s)") } else { String::new() };
            self.message = Some(format!("reopened {terminals} terminal(s) from before the restart{agents}"));
        }
    }

    fn layout(&self) -> Layout {
        let open = self
            .terms
            .iter()
            .map(|t| {
                let session = self.agents.session_for_tab(&t.info.tag);
                layout::Open {
                    cwd: t.info.cwd.clone(),
                    tag: t.info.tag.clone(),
                    session: session.map(|(id, _)| id.to_string()),
                    agent: session.map(|(_, kind)| kind.name.to_string()).unwrap_or_default(),
                    unread: t.unread(),
                }
            })
            .collect();
        Layout {
            current: Some(self.ws().root.clone()),
            workspaces: self.last_workspace.clone().into_iter().collect(),
            terminals: self.last_term.clone().into_iter().collect(),
            open,
        }
    }

    pub fn on_server(&mut self, message: ToClient) {
        match message {
            ToClient::Attach { info, rows, cols, token } => {
                let id = info.id as usize;
                if self.terms.iter().any(|t| t.id == id) {
                    return;
                }
                // What its agent last said was known before: only what you hadn't seen waits.
                let reported = self.agents.for_tab(&info.tag).map_or(State::Idle, |a| a.state);
                let unread = self.saved.open.iter().any(|o| o.tag == info.tag && o.unread);
                self.terms.push(Term::new(info, (rows, cols), self.link.clone()));
                let i = self.terms.len() - 1;
                (self.terms[i].reported, self.terms[i].hook_unread) = (reported, unread);
                self.terms[i].resize(self.term_size.0, self.term_size.1);
                let wt = self.workspace_of(&self.terms[i]);
                if let Some((old, unread)) = token.and_then(|t| self.reopening.remove(&t)) {
                    self.terms[i].hook_unread = unread;
                    self.resumed.insert(id, Instant::now());
                    let new = self.terms[i].info.tag.clone();
                    for tag in self.last_term.values_mut().filter(|tag| **tag == old) {
                        *tag = new.clone();
                    }
                    self.spawning.remove(&token.unwrap_or_default());
                    if self.reopening.is_empty() || wt.is_none_or(|s| s == self.current) {
                        self.restore_term();
                    }
                    return;
                }
                let Some((focus, _)) = token.and_then(|t| self.spawning.remove(&t)) else {
                    if self.ready && self.active.is_none() && wt.is_none_or(|s| s == self.current) {
                        self.restore_term();
                    }
                    return;
                };
                if token.is_some_and(|t| self.handing_over.remove(&t)) {
                    let root = self.terms[i].info.cwd.clone();
                    let on_setup = self.last_term.get(&root).is_none_or(|tag| self.terms.iter().any(|t| &t.info.tag == tag && is_setup_tab(t)));
                    if on_setup && wt == Some(self.current) {
                        self.select_term(id);
                    } else if on_setup {
                        self.last_term.insert(root, self.terms[i].info.tag.clone());
                    }
                }
                if focus && let Some(wt) = wt
                    && !self.set_current(wt)
                {
                    return;
                }
                if focus || (self.active.is_none() && wt.is_none_or(|s| s == self.current)) {
                    self.select_term(id);
                }
                if focus {
                    self.set_focus(Focus::Terminal);
                }
            }
            ToClient::Hello { protocol, exe, built, .. } => self.check_server_build(protocol, &exe, built),
            ToClient::Ready => {
                let stopped = self.reconnected.take().is_some_and(|had| had > 0) && self.terms.is_empty();
                if self.terms.is_empty() && !self.saved.open.is_empty() {
                    self.reopen();
                }
                if stopped {
                    let log = crate::server::dir().join("log");
                    let reopened = self.message.take().map(|m| format!("; {m}")).unwrap_or_default();
                    self.message = Some(format!("the Pulpo server stopped (see {}){reopened}", log.display()));
                }
                self.ready = true;
                self.keep_a_terminal();
                self.restore_term();
            }
            ToClient::Info { mut info } => {
                if info.unread && self.resumed.get(&(info.id as usize)).is_some_and(|at| at.elapsed() < RESUMING) {
                    info.unread = false;
                    self.link.send(&ToServer::Seen { id: info.id });
                }
                if let Some(term) = self.terms.iter_mut().find(|t| t.id == info.id as usize) {
                    term.set_info(info);
                }
            }
            ToClient::Snapshot { id, rows, cols } => {
                if self.watching == Some(id as usize)
                    && let Some(term) = self.terms.iter_mut().find(|t| t.id == id as usize)
                {
                    term.mirror(rows, cols);
                }
            }
            ToClient::Resized { id, rows, cols } => {
                if let Some(term) = self.terms.iter_mut().find(|t| t.id == id as usize) {
                    term.set_size(rows, cols);
                }
            }
            ToClient::Exit { id } => self.remove_term(id as usize),
            ToClient::Error { message } => self.message = Some(message),
        }
    }

    pub fn shows(&self, id: u32) -> bool {
        self.active == Some(id as usize)
    }

    /// How much longer to wait for the snapshot of the terminal on screen: drawing now would flash it empty.
    pub fn loading(&self) -> Option<Duration> {
        let waited = self.watched_at.elapsed();
        (self.term().is_some_and(|term| !term.mirrored()) && waited < SNAPSHOT).then(|| SNAPSHOT - waited)
    }

    /// The title isn't shown, and programs change it all the time.
    pub fn changes_view(&self, info: &Info) -> bool {
        let Some(term) = self.terms.iter().find(|t| t.id == info.id as usize) else { return true };
        Info { title: term.info.title.clone(), ..info.clone() } != term.info
    }

    pub fn on_output(&mut self, id: u32, bytes: &[u8]) {
        if let Some(term) = self.terms.iter_mut().find(|t| t.id == id as usize) {
            term.feed(bytes);
        }
    }

    fn check_server_build(&mut self, protocol: u32, exe: &Path, built: u64) {
        let Ok(mine) = std::env::current_exe() else { return };
        let same = std::fs::canonicalize(&mine).ok() == std::fs::canonicalize(exe).ok();
        if same && !self.upgrade_asked && (crate::server::built(&mine) > built || protocol != PROTOCOL) {
            self.upgrade_asked = true;
            self.link.send(&ToServer::Upgrade { exe: mine });
        } else if protocol != PROTOCOL {
            self.message = Some(format!("the Pulpo server runs another build ({}); it stops once its terminals are closed", exe.display()));
        }
    }

    pub fn on_server_gone(&mut self) {
        let had = self.terms.len();
        self.ready = false;
        self.terms.clear();
        self.spawning.clear();
        self.active = None;
        self.watching = None;
        self.selecting = None;
        if self.last_reconnect.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
            self.offline = true;
            self.message = Some(format!("the Pulpo server keeps stopping (its log: ~/.local/state/pulpo/server/log); {} t tries again", self.keymap.label()));
            return;
        }
        self.last_reconnect = Some(Instant::now());
        match client::connect(self.tx.clone()) {
            Ok(link) => {
                self.link = link;
                self.reconnected = Some(had);
            }
            Err(e) => {
                self.offline = true;
                self.message = Some(format!("{e:#}; {} t tries again", self.keymap.label()));
            }
        }
    }

    /// Not on every key: starting the server can take seconds.
    fn retry_server(&mut self) {
        if std::mem::take(&mut self.offline) {
            self.last_reconnect = None;
            self.on_server_gone();
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        if let Some(prompt) = &mut self.prompt {
            prompt.text.push_str(text.trim());
        } else if let Some(search) = self.repos.as_mut().or(self.search.as_mut()) {
            search.query.push_str(text.trim());
            search.update(&self.tx);
        } else if self.focus == Focus::Files
            && let Some(editor) = &mut self.editor
        {
            if editor.find_typing {
                editor.find.get_or_insert_default().push_str(text.trim());
            } else {
                editor.insert(text);
                editor.follow_cursor();
            }
        } else if self.focus == Focus::Terminal
            && let Some(term) = self.term_mut()
        {
            term.paste(text);
        }
    }

    pub fn set_term_size(&mut self, rows: u16, cols: u16) {
        self.term_size = (rows.max(1), cols.max(1));
    }

    pub fn term_state(&self, term: &Term) -> State {
        // Its pauses aren't news.
        if is_setup_tab(term) {
            return State::Idle;
        }
        match term.status().map(|(status, _)| status) {
            Some(Status::Working) => State::Running,
            Some(Status::Blocked) => State::Waiting,
            None if term.working() => State::Running,
            _ if term.unread() => State::Waiting,
            _ => State::Idle,
        }
    }

    pub fn tick(&mut self) {
        if self.prefixed_at.is_some_and(|at| at.elapsed() >= PREFIX_FOR) {
            self.prefixed = false;
            self.prefixed_at = None;
        }
        match &self.message {
            None => self.message_since = None,
            Some(m) if self.message_since.as_ref().is_none_or(|(shown, _)| shown != m) => self.message_since = Some((m.clone(), Instant::now())),
            Some(_) if !self.offline && self.message_since.as_ref().is_some_and(|(_, at)| at.elapsed() >= MESSAGE_FOR) => self.message = None,
            Some(_) => {}
        }
        if self.ready {
            let layout = self.layout();
            if layout != self.saved {
                layout::save(&layout);
                self.saved = layout;
            }
        }
        let viewing = self.active.filter(|_| self.focus == Focus::Terminal);
        let mut notices = Vec::new();
        for term in &mut self.terms {
            let agent = self.agents.for_tab(&term.info.tag);
            let reported = agent.map_or(State::Idle, |a| a.state);
            if reported != term.reported {
                if matches!(reported, State::Waiting | State::Done) {
                    term.hook_unread = true;
                    term.notice = Some(agent.map(|a| a.detail.clone()).unwrap_or_default());
                }
                term.reported = reported;
            }
            if viewing == Some(term.id) {
                term.seen();
            }
            if let Some(detail) = term.notice.take()
                && term.last_notice.is_none_or(|at| at.elapsed() >= NOTICE_EVERY)
            {
                term.last_notice = Some(Instant::now());
                let detail = if detail.is_empty() { "finished".to_string() } else { detail };
                notices.push((term.label().to_string(), term.info.cwd.clone(), detail));
            }
        }
        for (label, cwd, detail) in notices {
            let place = self.place(&cwd).map(|place| format!(" in {place}")).unwrap_or_default();
            notify(&format!("{label}{place}: {detail}"));
        }
    }

    pub fn animating(&self) -> bool {
        !self.setting_up.is_empty() || !self.copying.is_empty() || !self.spawning.is_empty() || self.terms.iter().any(|term| self.term_state(term) == State::Running)
    }

    fn workspace_of(&self, term: &Term) -> Option<usize> {
        self.workspaces.iter().position(|w| w.root == term.info.cwd)
    }

    /// One started outside every checkout shows in all of them, so it stays reachable.
    pub fn workspace_terms(&self) -> Vec<usize> {
        (0..self.terms.len()).filter(|&i| self.workspace_of(&self.terms[i]).is_none_or(|s| s == self.current)).collect()
    }

    pub fn workspace_state(&self, i: usize) -> Option<State> {
        let root = &self.workspaces[i].root;
        if self.making(root) || self.setting_up.contains_key(root) {
            return Some(State::Running);
        }
        if let Some(&ok) = self.setup_ended.get(root) {
            return Some(if ok { State::Ready } else { State::Failed });
        }
        let states = self.terms.iter().filter(|t| &t.info.cwd == root).map(|t| self.term_state(t));
        states.max_by_key(|state| state.urgency())
    }

    pub fn making(&self, root: &Path) -> bool {
        self.copying.iter().any(|(repo, name)| worktree::root_of(repo, name) == root)
    }

    /// Its folder or a terminal still on the way.
    pub fn opening(&self, root: &Path) -> bool {
        self.making(root) || self.spawning.values().any(|(_, cwd)| cwd == root)
    }

    pub fn term(&self) -> Option<&Term> {
        self.active.and_then(|id| self.terms.iter().find(|t| t.id == id))
    }

    fn term_mut(&mut self) -> Option<&mut Term> {
        self.active.and_then(|id| self.terms.iter_mut().find(|t| t.id == id))
    }

    fn select_term(&mut self, id: usize) {
        self.active = Some(id);
        self.selecting = None;
        if let Some(term) = self.terms.iter().find(|t| t.id == id) {
            self.last_term.insert(self.ws().root.clone(), term.info.tag.clone());
        }
    }

    fn restore_term(&mut self) {
        let root = self.ws().root.clone();
        let terms: Vec<&Term> = self.workspace_terms().into_iter().map(|i| &self.terms[i]).collect();
        let last = self.last_term.get(&root).and_then(|tag| terms.iter().find(|t| &t.info.tag == tag));
        self.active = last.or(terms.first()).map(|t| t.id);
    }

    fn remove_term(&mut self, id: usize) {
        let tabs: Vec<usize> = self.workspace_terms().into_iter().map(|i| self.terms[i].id).collect();
        self.terms.retain(|t| t.id != id);
        if self.active == Some(id) {
            let at = tabs.iter().position(|&t| t == id).unwrap_or(0);
            let next = tabs.get(at + 1).or(at.checked_sub(1).and_then(|p| tabs.get(p))).copied();
            self.active = None;
            if let Some(next) = next {
                self.select_term(next);
            }
        }
        self.keep_a_terminal();
    }

    fn cycle_tab(&mut self, dir: isize) {
        let tabs: Vec<usize> = self.workspace_terms().into_iter().map(|i| self.terms[i].id).collect();
        if tabs.is_empty() {
            return;
        }
        let at = self.active.and_then(|id| tabs.iter().position(|&t| t == id)).unwrap_or(0);
        let next = tabs[(at as isize + dir).rem_euclid(tabs.len() as isize) as usize];
        self.select_term(next);
        self.set_focus(Focus::Terminal);
    }

    fn go_to_workspace(&mut self, n: usize) {
        match self.workspace_order().get(n) {
            Some(&i) if self.set_current(i) => self.set_focus(Focus::Terminal),
            Some(_) => {}
            None => self.message = Some(format!("no workspace {}", n + 1)),
        }
    }

    fn next_waiting(&mut self) {
        let ended = (0..self.workspaces.len()).find(|&i| i != self.current && self.setup_ended.contains_key(&self.workspaces[i].root));
        if let Some(i) = ended {
            if self.set_current(i) {
                self.set_focus(Focus::Terminal);
            }
            return;
        }
        let mut waiting: Vec<(bool, u64, usize)> = self
            .terms
            .iter()
            .filter(|t| !(self.focus == Focus::Terminal && self.active == Some(t.id)))
            .filter_map(|t| {
                let state = self.term_state(t);
                let since = self.agents.for_tab(&t.info.tag).map_or(u64::MAX, |a| a.ts);
                matches!(state, State::Waiting | State::Done).then_some((state != State::Waiting, since, t.id))
            })
            .collect();
        waiting.sort_unstable();
        let Some(&(.., id)) = waiting.first() else {
            self.message = Some("no terminal has anything to read".into());
            return;
        };
        if let Some(i) = self.terms.iter().find(|t| t.id == id).and_then(|t| self.workspace_of(t))
            && !self.set_current(i)
        {
            return;
        }
        self.select_term(id);
        self.set_focus(Focus::Terminal);
    }

    fn close_terminal(&mut self, id: usize) {
        self.link.send(&ToServer::Close { id: id as u32 });
        self.remove_term(id);
    }

    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        if self.no_projects {
            return self.repos_key(key);
        }
        let pressed = Key::of(key);
        let open = self.repos.is_some() || self.search.is_some() || self.prompt.is_some() || self.asking.is_some() || self.help;
        if open && !self.prefixed && self.keymap.is_prefix(pressed) {
            (self.repos, self.search, self.prompt, self.asking, self.help) = (None, None, None, None, false);
            self.prefixed = true;
            self.prefixed_at = Some(Instant::now());
            return false;
        }
        if self.help {
            self.help = false;
            return false;
        }
        if self.repos.is_some() {
            return self.repos_key(key);
        }
        if self.search.is_some() {
            self.search_key(key);
            return false;
        }
        if self.prompt.is_some() {
            self.prompt_key(key);
            return false;
        }
        if let Some((_, confirm)) = self.asking.take() {
            self.message = None;
            return matches!(key.code, KeyCode::Char('y' | 'Y')) && self.confirmed(confirm);
        }
        if std::mem::take(&mut self.prefixed) {
            // Twice: the key itself, for the program (Claude's ⌃b).
            if self.keymap.is_prefix(pressed) {
                if self.focus == Focus::Terminal
                    && let Some(term) = self.term_mut()
                {
                    term.key(key);
                }
                return false;
            }
            return self.command(pressed);
        }
        if self.keymap.is_prefix(pressed) {
            self.prefixed = true;
            self.prefixed_at = Some(Instant::now());
            return false;
        }
        if !self.offline {
            self.message = None;
        }
        match self.focus {
            Focus::Terminal => {
                self.selecting = None;
                if let Some(term) = self.term_mut() {
                    term.key(key);
                }
            }
            Focus::Projects | Focus::Workspaces => self.list_key(key),
            Focus::Files => self.files_key(key),
            Focus::Changes => self.changes_key(key),
            Focus::History => self.history_key(key),
        }
        false
    }

    fn command(&mut self, key: Key) -> bool {
        let KeyCode::Char(c) = key.code else { return false };
        match c {
            'p' => self.set_focus(Focus::Projects),
            'w' => self.set_focus(Focus::Workspaces),
            't' => self.set_focus(Focus::Terminal),
            'f' => self.set_focus(Focus::Files),
            'c' => self.set_focus(Focus::Changes),
            'h' => self.set_focus(Focus::History),
            'F' => {
                self.set_focus(Focus::Files);
                self.files_expanded = true;
            }
            'C' => {
                self.set_focus(Focus::Changes);
                self.changes_expanded = true;
            }
            'H' => {
                self.set_focus(Focus::History);
                // A commit already full screen gives the workspace's changes back first.
                self.settle();
                self.open_commit();
            }
            'n' => self.new_here(),
            'N' => self.pick_project(),
            'x' => self.close_here(),
            '[' => self.cycle_tab(-1),
            ']' => self.cycle_tab(1),
            'a' => self.next_waiting(),
            '1'..='9' => self.go_to_workspace(c as usize - '1' as usize),
            '?' => self.help = true,
            '<' => {
                self.left_hidden = !self.left_hidden;
                if self.left_hidden && matches!(self.focus, Focus::Projects | Focus::Workspaces) {
                    self.set_focus(Focus::Terminal);
                }
            }
            '>' => {
                self.right_hidden = !self.right_hidden;
                if self.right_hidden && matches!(self.focus, Focus::Files | Focus::Changes | Focus::History) {
                    self.set_focus(Focus::Terminal);
                }
            }
            '/' => self.start_search(),
            'd' | 'e' | 'r' if self.focus != Focus::Files => self.message = Some(format!("{} f first: that's for files", self.keymap.label())),
            'd' => self.ask_delete(),
            'e' => self.edit(),
            'r' => self.start_rename(),
            'q' | 'Q' | 'u' if self.editing() => {
                self.message = Some(UNSAVED.into());
            }
            'q' => return true,
            'u' if self.release.is_some() => {
                self.updating = true;
                return true;
            }
            'u' => self.message = Some("no update found".into()),
            'Q' => {
                let n = self.terms.len();
                self.ask(format!("stop {n} terminal(s) and quit?"), Confirm::StopAll);
            }
            _ => {
                let p = self.keymap.label();
                self.message = Some(format!("{p} ? keybinds"));
            }
        }
        false
    }

    fn ask(&mut self, question: String, confirm: Confirm) {
        self.message = None;
        self.asking = Some((question, confirm));
    }

    fn confirmed(&mut self, confirm: Confirm) -> bool {
        match confirm {
            Confirm::RemoveProject(repo) => {
                let name = repo.file_name().unwrap_or_default().to_string_lossy().into_owned();
                project::unregister(&repo);
                self.message = Some(format!("removed {name} from the list (files kept)"));
                self.load_workspaces();
            }
            Confirm::DeleteWorkspace(root) => self.remove_workspace(&root),
            Confirm::CloseTerminal(id) => self.close_terminal(id),
            Confirm::DeleteFile(path) => self.delete(&path),
            Confirm::StopAll => {
                for &pid in self.setting_up.values() {
                    unsafe { libc::killpg(pid as i32, libc::SIGTERM) };
                }
                for term in &self.terms {
                    self.link.send(&ToServer::Close { id: term.info.id });
                }
                // Stopped on purpose: nothing to bring back next time.
                layout::save(&Layout { open: Vec::new(), ..self.layout() });
                return true;
            }
        }
        false
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        match focus {
            Focus::Projects | Focus::Workspaces => self.left_hidden = false,
            Focus::Files | Focus::Changes | Focus::History => self.right_hidden = false,
            Focus::Terminal => {}
        }
        self.discard = false;
        self.reading = false;
        match focus {
            Focus::Terminal => {
                self.retry_server();
                self.keep_a_terminal();
            }
            Focus::Files => self.preview(),
            Focus::Changes => {
                if let Some(changes) = &mut self.changes {
                    changes.follow();
                }
            }
            _ => {}
        }
    }

    fn new_here(&mut self) {
        match self.focus {
            Focus::Projects => self.open_repo_picker(),
            Focus::Workspaces => self.ask_workspace(self.ws().repo.clone()),
            Focus::Terminal => self.new_terminal(true),
            Focus::Files => {
                let root = self.ws().root.clone();
                let dir = match self.ws().selected_row() {
                    Some(row) if row.is_dir => row.path.clone(),
                    Some(row) => row.path.parent().map_or(root.clone(), Path::to_path_buf),
                    None => root,
                };
                self.prompt = Some(Prompt { ask: Ask::File(dir), text: String::new() });
            }
            Focus::Changes => self.message = Some("changes are the agents' to make".into()),
            Focus::History => self.message = Some("commits are the agents' to make".into()),
        }
    }

    fn close_here(&mut self) {
        if matches!(self.focus, Focus::Projects | Focus::Workspaces) && self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        match self.focus {
            Focus::Projects => {
                let (repo, name) = (self.ws().repo.clone(), self.ws().name.clone());
                if project::registered().contains(&repo) {
                    self.ask(format!("remove {name} from the list? its files stay."), Confirm::RemoveProject(repo));
                }
            }
            Focus::Workspaces => match self.worktrees.iter().find(|s| s.root == self.ws().root).cloned() {
                Some(wt) => {
                    let lost = worktree::unsaved(&wt);
                    let lost = if lost.is_empty() { String::new() } else { format!(", losing {}", lost.join(" and ")) };
                    let what = if wt.kind == worktree::Kind::Worktree { "folder" } else { "copy" };
                    let kept = worktree::kept_branch(&wt).map(|b| format!("; branch {b} stays")).unwrap_or_default();
                    let place = self.place(&wt.root).unwrap_or(wt.name.clone());
                    let question = format!("delete workspace {place}: its {what}, containers and database{lost}{kept}?");
                    self.ask(question, Confirm::DeleteWorkspace(wt.root));
                }
                None => self.message = Some(format!("this is the project itself: {p} p, then {p} x takes it off the list", p = self.keymap.label())),
            },
            Focus::Terminal => {
                let Some(term) = self.term() else { return };
                let (id, label) = (term.id, term.label().to_string());
                match term.info.running.clone() {
                    Some(program) => self.ask(format!("{program} is running in {label}: close it?"), Confirm::CloseTerminal(id)),
                    None => self.close_terminal(id),
                }
            }
            Focus::Files => self.ask_delete(),
            Focus::Changes | Focus::History => {}
        }
    }

    fn pick_project(&mut self) {
        let projects = self.projects();
        let at = projects.iter().position(|&i| self.workspaces[i].repo == self.ws().repo).unwrap_or(0);
        self.prompt = Some(Prompt { ask: Ask::Project(at), text: String::new() });
    }

    fn ask_workspace(&mut self, repo: PathBuf) {
        self.new_in = repo;
        self.prompt = Some(Prompt { ask: Ask::Workspace(String::new()), text: String::new() });
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(Prompt { ask: Ask::Project(at), .. }) = &self.prompt {
            let (at, projects) = (*at, self.projects());
            let step = |by: usize| Ask::Project((at + by) % projects.len().max(1));
            match key.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Enter => match projects.get(at) {
                    Some(&i) => self.ask_workspace(self.workspaces[i].repo.clone()),
                    None => self.prompt = None,
                },
                KeyCode::Up | KeyCode::Left => self.prompt = Some(Prompt { ask: step(projects.len().saturating_sub(1)), text: String::new() }),
                KeyCode::Down | KeyCode::Right => self.prompt = Some(Prompt { ask: step(1), text: String::new() }),
                _ => {}
            }
            return;
        }
        let Some(prompt) = &mut self.prompt else { return };
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Enter => {
                if let Some(Prompt { ask, text }) = self.prompt.take() {
                    let (text, ask) = match ask {
                        Ask::Branch(name) => (name, Ask::Workspace(text)),
                        ask => (text, ask),
                    };
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        match ask {
                            Ask::Workspace(branch) => self.create_workspace(&text, Some(branch.trim()).filter(|b| !b.is_empty())),
                            Ask::File(dir) => self.create(&dir, &text),
                            Ask::Rename(path) => self.rename(&path, &text),
                            Ask::Branch(_) | Ask::Project(_) => {}
                        }
                    }
                }
            }
            KeyCode::Tab | KeyCode::BackTab => {
                let (Ask::Workspace(other) | Ask::Branch(other)) = &mut prompt.ask else { return };
                let text = std::mem::replace(&mut prompt.text, std::mem::take(other));
                prompt.ask = if matches!(prompt.ask, Ask::Workspace(_)) { Ask::Branch(text) } else { Ask::Workspace(text) };
            }
            // Workspace names are lowercase, like Compose projects.
            KeyCode::Char(c) if !ctrl && matches!(prompt.ask, Ask::Workspace(_)) => prompt.text.push(c.to_ascii_lowercase()),
            _ => {
                input::edit(&mut prompt.text, key);
            }
        }
    }

    fn list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Home | KeyCode::Char('g') => self.step(i32::MIN),
            KeyCode::End | KeyCode::Char('G') => self.step(i32::MAX),
            KeyCode::Right if self.focus == Focus::Projects => self.set_focus(Focus::Workspaces),
            KeyCode::Enter | KeyCode::Right => self.set_focus(Focus::Terminal),
            KeyCode::Left if self.focus == Focus::Workspaces => self.set_focus(Focus::Projects),
            _ => {}
        }
    }

    fn step(&mut self, delta: i32) {
        let (projects, workspaces) = (self.projects().len(), self.workspace_order().len());
        let (state, len) = match self.focus {
            Focus::Projects => (&mut self.projects_state, projects),
            _ => (&mut self.workspaces_state, workspaces),
        };
        let to = state.selected().map_or(0, |from| from as i64 + delta as i64);
        // One past the end of a list: on into the other, as if they were one.
        match self.focus {
            Focus::Projects if delta == 1 && to >= projects as i64 && workspaces > 0 => {
                self.set_focus(Focus::Workspaces);
                self.workspaces_state.select(Some(0));
            }
            Focus::Workspaces if delta == -1 && to < 0 && projects > 0 => {
                self.set_focus(Focus::Projects);
                self.projects_state.select(Some(projects - 1));
            }
            _ => state.select(Some(to.clamp(0, len.saturating_sub(1) as i64) as usize)),
        }
        self.on_select();
    }

    fn on_select(&mut self) {
        match self.focus {
            Focus::Projects => {
                if let Some(project) = self.projects_state.selected().and_then(|i| self.projects().get(i).copied()) {
                    self.set_current(project);
                }
            }
            _ => {
                if let Some(i) = self.workspaces_state.selected().and_then(|i| self.workspace_order().get(i).copied()) {
                    self.set_current(i);
                }
            }
        }
    }

    fn preview(&mut self) {
        if self.editor.is_some() {
            return;
        }
        match self.ws().selected_row().filter(|r| !r.is_dir).map(|r| r.path.clone()) {
            Some(path) => self.viewer.open(Source::File(path)),
            None => self.viewer.close(),
        }
    }

    fn files_key(&mut self, key: KeyEvent) {
        if self.editor.is_some() {
            return self.editor_key(key);
        }
        if self.files_expanded {
            let half = (self.viewer.height / 2).max(1) as isize;
            match key.code {
                KeyCode::Esc if self.reading => self.reading = false,
                KeyCode::Esc => self.files_expanded = false,
                KeyCode::Tab | KeyCode::BackTab => self.reading = !self.reading,
                KeyCode::PageDown => self.viewer.scroll_by(half),
                KeyCode::PageUp => self.viewer.scroll_by(-half),
                _ if self.reading => match scroll_keys(key, self.viewer.height) {
                    Some(Scroll::Rows(rows)) => self.viewer.scroll_by(rows),
                    Some(Scroll::Sideways(columns)) => self.viewer.scroll_sideways(columns),
                    None => {}
                },
                _ => self.tree_key(key),
            }
            return;
        }
        if key.code == KeyCode::Esc {
            return self.set_focus(Focus::Terminal);
        }
        self.tree_key(key);
    }

    fn tree_key(&mut self, key: KeyEvent) {
        let row = self.ws().selected_row().map(|r| (r.is_dir, r.expanded));
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.tree_step(1),
            KeyCode::Up | KeyCode::Char('k') => self.tree_step(-1),
            KeyCode::PageDown => self.tree_step(10),
            KeyCode::PageUp => self.tree_step(-10),
            KeyCode::Home | KeyCode::Char('g') => self.tree_step(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.tree_step(isize::MAX / 2),
            KeyCode::Right | KeyCode::Char('l') => self.fold_tree(true),
            KeyCode::Left | KeyCode::Char('h') => self.fold_tree(false),
            KeyCode::Enter => match row {
                Some((true, expanded)) => self.fold_tree(!expanded),
                Some((false, _)) if self.files_expanded => self.reading = true,
                Some((false, _)) => (self.files_expanded, self.reading) = (true, true),
                None => {}
            },
            _ => {}
        }
    }

    fn tree_step(&mut self, delta: isize) {
        let w = self.ws_mut();
        let max = w.tree.rows.len().saturating_sub(1) as isize;
        let next = (w.files_state.selected().unwrap_or(0) as isize).saturating_add(delta).clamp(0, max);
        w.files_state.select(Some(next as usize));
        self.preview();
    }

    fn fold_tree(&mut self, open: bool) {
        let w = self.ws_mut();
        let Some(row) = w.selected_row() else { return };
        let path = row.path.clone();
        if row.is_dir && row.expanded != open {
            w.tree.set_expanded(&path, open);
            w.reselect(Some(path));
        } else if !open && path.parent() != Some(&w.root) {
            w.reselect(path.parent().map(Path::to_path_buf));
        }
        self.preview();
    }

    fn selected_path(&self) -> Option<PathBuf> {
        self.ws().selected_row().map(|r| r.path.clone())
    }

    fn rel(&self, path: &Path) -> String {
        path.strip_prefix(&self.ws().root).unwrap_or(path).display().to_string()
    }

    fn ask_delete(&mut self) {
        let Some(path) = self.selected_path() else { return };
        let rel = self.rel(&path);
        let what = if path.is_dir() { format!("{rel}/ and everything in it") } else { rel };
        let undo = if git::is_tracked(&self.ws().root, &path) { "git can restore the last commit" } else { "not in git, it can't be recovered" };
        self.ask(format!("delete {what}? {undo}."), Confirm::DeleteFile(path));
    }

    fn delete(&mut self, path: &Path) {
        let rel = self.rel(path);
        let deleted = if path.is_dir() { std::fs::remove_dir_all(path) } else { std::fs::remove_file(path) };
        match deleted {
            Ok(()) => {
                if self.editor.as_ref().is_some_and(|e| e.path.starts_with(path)) {
                    self.editor = None;
                }
                let w = self.ws_mut();
                w.tree.rebuild();
                w.reselect(None);
                self.viewer.close();
                self.preview();
                self.message = Some(format!("deleted {rel}"));
            }
            Err(e) => self.message = Some(format!("could not delete {rel}: {e}")),
        }
    }

    fn start_rename(&mut self) {
        let Some(path) = self.selected_path() else { return };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        self.prompt = Some(Prompt { ask: Ask::Rename(path), text: name });
    }

    fn inside(&mut self, dir: &Path, name: &str) -> Option<PathBuf> {
        let path = dir.join(name.trim_end_matches('/'));
        let inside = path.starts_with(&self.ws().root) && !path.components().any(|c| c == std::path::Component::ParentDir);
        if !inside {
            self.message = Some("files stay inside the workspace".into());
        }
        inside.then_some(path)
    }

    fn rename(&mut self, path: &Path, name: &str) {
        let Some(to) = path.parent().and_then(|dir| self.inside(dir, name)) else { return };
        if to == path {
            return;
        }
        let (from_rel, to_rel) = (self.rel(path), self.rel(&to));
        if to.exists() {
            self.message = Some(format!("{to_rel} already exists"));
            return;
        }
        match std::fs::rename(path, &to) {
            Ok(()) => {
                if let Some(editor) = &mut self.editor
                    && let Ok(rest) = editor.path.strip_prefix(path)
                {
                    editor.path = to.join(rest);
                }
                self.ws_mut().reveal(&to);
                self.viewer.close();
                self.preview();
                self.message = Some(format!("renamed {from_rel} to {to_rel}"));
            }
            Err(e) => self.message = Some(format!("could not rename {from_rel}: {e}")),
        }
    }

    fn create(&mut self, dir: &Path, name: &str) {
        let Some(path) = self.inside(dir, name) else { return };
        let rel = self.rel(&path);
        if path.exists() {
            self.message = Some(format!("{rel} already exists"));
            return;
        }
        let folder = name.ends_with('/');
        let made = if folder {
            std::fs::create_dir_all(&path)
        } else {
            path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|()| std::fs::File::create_new(&path).map(drop))
        };
        if let Err(e) = made {
            self.message = Some(format!("could not create {rel}: {e}"));
            return;
        }
        self.ws_mut().reveal(&path);
        self.set_focus(Focus::Files);
        if folder {
            self.message = Some(format!("created {rel}/"));
        } else {
            self.open_editor(path, None);
        }
    }

    fn edit(&mut self) {
        if self.editor.is_some() {
            return;
        }
        match self.selected_path() {
            Some(path) if path.is_file() => {
                let line = matches!(&self.viewer.source, Some(Source::File(p)) if *p == path).then(|| self.viewer.scroll + 1);
                self.open_editor(path, line);
            }
            _ => self.message = Some("select a file to edit".into()),
        }
    }

    fn open_editor(&mut self, path: PathBuf, line: Option<usize>) {
        match Editor::open(path, line) {
            Ok(editor) => {
                self.editor = Some(editor);
                self.discard = false;
            }
            Err(e) => self.message = Some(e),
        }
    }

    fn close_editor(&mut self) {
        let Some(editor) = self.editor.take() else { return };
        self.viewer.close();
        self.viewer.open_at(editor.path.clone(), None);
        self.viewer.scroll = editor.scroll;
        self.files_expanded = true;
        self.reading = false;
        self.discard = false;
    }

    fn editor_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let cmd = key.modifiers.contains(KeyModifiers::SUPER);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(editor) = &mut self.editor else { return };
        if editor.find_typing {
            match key.code {
                KeyCode::Esc => editor.find_typing = false,
                KeyCode::Enter => {
                    editor.find_next(!shift);
                }
                KeyCode::Backspace => {
                    editor.find.get_or_insert_default().pop();
                }
                KeyCode::Char(c) if !ctrl && !cmd => {
                    editor.find.get_or_insert_default().push(c);
                    if let Some((start, _)) = editor.selection() {
                        editor.click_at(start);
                    }
                    editor.find_next(true);
                }
                _ => {}
            }
            editor.follow_cursor();
            return;
        }
        let discard = std::mem::take(&mut self.discard);
        match key.code {
            KeyCode::Esc if editor.find.is_some() => editor.find = None,
            KeyCode::Esc if editor.dirty && !discard => {
                self.discard = true;
                self.message = Some("unsaved changes: esc again throws them away, ⌃s saves them".into());
                return;
            }
            KeyCode::Esc => return self.close_editor(),
            KeyCode::Char('s') if ctrl || cmd => {
                let name = editor.path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                match editor.save() {
                    Ok(()) => self.message = Some(format!("saved {name}")),
                    Err(e) => self.message = Some(e),
                }
                return;
            }
            KeyCode::Char('z') if (ctrl || cmd) && shift => editor.redo(),
            KeyCode::Char('Z') if ctrl || cmd => editor.redo(),
            KeyCode::Char('z') if ctrl || cmd => editor.undo(),
            KeyCode::Char('y') if ctrl => editor.redo(),
            KeyCode::Char('a') if ctrl || cmd => editor.select_all(),
            KeyCode::Char('c') if ctrl || cmd => {
                let text = editor.copy();
                set_clipboard(&text);
            }
            KeyCode::Char('x') if ctrl || cmd => {
                let text = editor.cut();
                set_clipboard(&text);
            }
            KeyCode::Char('v') if ctrl || cmd => {
                if let Some(text) = clipboard() {
                    editor.insert(&text);
                }
            }
            KeyCode::Char('f') if ctrl || cmd => {
                editor.find_typing = true;
                editor.find.get_or_insert_default();
            }
            KeyCode::Char('g') if ctrl || cmd => {
                editor.find_next(!shift);
            }
            KeyCode::Left if alt || ctrl => editor.word(false, shift),
            KeyCode::Right if alt || ctrl => editor.word(true, shift),
            KeyCode::Left if cmd => editor.home(shift),
            KeyCode::Right if cmd => editor.end(shift),
            KeyCode::Up if cmd => editor.edge(false, shift),
            KeyCode::Down if cmd => editor.edge(true, shift),
            KeyCode::Left => editor.left(shift),
            KeyCode::Right => editor.right(shift),
            KeyCode::Up => editor.vertical(-1, shift),
            KeyCode::Down => editor.vertical(1, shift),
            KeyCode::Home if ctrl => editor.edge(false, shift),
            KeyCode::End if ctrl => editor.edge(true, shift),
            KeyCode::Home => editor.home(shift),
            KeyCode::End => editor.end(shift),
            KeyCode::PageUp => editor.page(-1, shift),
            KeyCode::PageDown => editor.page(1, shift),
            KeyCode::Enter => editor.newline(),
            KeyCode::Tab => editor.tab(),
            KeyCode::BackTab => editor.untab(),
            KeyCode::Backspace => editor.backspace(alt || ctrl),
            KeyCode::Delete => editor.delete(alt || ctrl),
            KeyCode::Char(c) if !ctrl && !cmd => editor.insert(&c.to_string()),
            _ => {}
        }
        editor.follow_cursor();
    }

    fn start_search(&mut self) {
        let place = |kind, i: usize, text: String| {
            let w = &self.workspaces[i];
            Hit { kind, rel: w.name.clone(), line: None, text, path: w.root.clone() }
        };
        let projects = self.projects().into_iter().map(|i| place(Kind::Project, i, String::new()));
        let workspaces = self.workspace_order().into_iter().map(|i| {
            let repo = self.workspaces[i].repo.file_name().unwrap_or_default().to_string_lossy().into_owned();
            place(Kind::Workspace, i, repo)
        });
        let places = projects.chain(workspaces).collect();
        self.search = Some(Search::new(Mode::Everything, self.ws().root.clone(), places, &self.tx));
    }

    fn open_repo_picker(&mut self) {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        self.repos = Some(Search::new(Mode::Repos, home, Vec::new(), &self.tx));
    }

    pub fn on_search(&mut self, generation: usize, hits: Vec<crate::search::Hit>) {
        if let Some(search) = self.search.as_mut().or(self.repos.as_mut()) {
            search.on_results(generation, hits);
        }
    }

    pub fn on_repos(&mut self, repos: Vec<String>) {
        if let Some(search) = &mut self.repos {
            search.set_repos(repos, &self.tx);
        }
    }

    fn search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(search) = &mut self.search else { return };
        match key.code {
            KeyCode::Esc => self.search = None,
            KeyCode::Enter => self.open_hit(),
            KeyCode::Up => self.search_step(-1),
            KeyCode::Down => self.search_step(1),
            KeyCode::Char('k' | 'p') if ctrl => self.search_step(-1),
            KeyCode::Char('j' | 'n') if ctrl => self.search_step(1),
            KeyCode::PageUp => self.search_step(-10),
            KeyCode::PageDown => self.search_step(10),
            _ => {
                if input::edit(&mut search.query, key) {
                    search.update(&self.tx);
                }
            }
        }
    }

    fn search_step(&mut self, delta: isize) {
        if let Some(search) = &mut self.search {
            search.step(delta);
        }
    }

    fn open_hit(&mut self) {
        let Some(hit) = self.search.as_ref().and_then(Search::selected).cloned() else { return };
        if matches!(hit.kind, Kind::Project | Kind::Workspace) {
            self.search = None;
            if let Some(i) = self.workspaces.iter().position(|w| w.root == hit.path)
                && self.set_current(i)
            {
                self.set_focus(Focus::Terminal);
            }
            return;
        }
        let (path, line) = (hit.path, hit.line);
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        self.search = None;
        self.editor = None;
        self.set_focus(Focus::Files);
        self.show_file(path, line);
        self.files_expanded = true;
        self.reading = true;
    }

    fn show_file(&mut self, path: PathBuf, line: Option<usize>) {
        self.ws_mut().reveal(&path);
        self.viewer.open_at(path, line);
    }

    fn repos_key(&mut self, key: KeyEvent) -> bool {
        let Some(search) = &mut self.repos else { return false };
        match key.code {
            KeyCode::Esc if self.no_projects => return true,
            KeyCode::Esc => self.repos = None,
            KeyCode::Enter => {
                let picked = search.selected().map(|h| h.rel.clone()).or_else(|| (!search.query.is_empty()).then(|| search.query.clone()));
                let typed = picked.as_ref() == Some(&search.query);
                if picked.is_some_and(|picked| self.add_repo(&picked, typed)) {
                    self.repos = None;
                }
            }
            KeyCode::Up => search.step(-1),
            KeyCode::Down => search.step(1),
            KeyCode::PageUp => search.step(-10),
            KeyCode::PageDown => search.step(10),
            _ => {
                if input::edit(&mut search.query, key) {
                    self.message = None;
                    search.update(&self.tx);
                }
            }
        }
        false
    }

    fn changes_key(&mut self, key: KeyEvent) {
        if self.changes_expanded {
            let Some(changes) = &mut self.changes else { return };
            let half = (changes.height / 2).max(1) as isize;
            match key.code {
                KeyCode::Esc if self.reading => self.reading = false,
                KeyCode::Esc => self.changes_expanded = false,
                KeyCode::Tab | KeyCode::BackTab => self.reading = !self.reading,
                KeyCode::PageDown => changes.scroll_by(half),
                KeyCode::PageUp => changes.scroll_by(-half),
                _ if self.reading => match scroll_keys(key, changes.height) {
                    Some(Scroll::Rows(rows)) => changes.scroll_by(rows),
                    Some(Scroll::Sideways(columns)) => changes.scroll_sideways(columns),
                    None => {}
                },
                _ => self.changes_tree_key(key),
            }
            return;
        }
        if key.code == KeyCode::Esc {
            return self.set_focus(Focus::Terminal);
        }
        self.changes_tree_key(key);
    }

    fn changes_tree_key(&mut self, key: KeyEvent) {
        let Some(changes) = &mut self.changes else { return };
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => changes.step(1),
            KeyCode::Up | KeyCode::Char('k') => changes.step(-1),
            KeyCode::PageDown => changes.step(10),
            KeyCode::PageUp => changes.step(-10),
            KeyCode::Right | KeyCode::Char('l') => changes.fold(true),
            KeyCode::Left | KeyCode::Char('h') => changes.fold(false),
            KeyCode::Enter => match changes.selected().map(|r| (r.is_dir, r.expanded)) {
                Some((true, expanded)) => changes.fold(!expanded),
                Some((false, _)) if self.changes_expanded => self.reading = true,
                Some((false, _)) => (self.changes_expanded, self.reading) = (true, true),
                None => {}
            },
            _ => {}
        }
    }

    fn history_step(&mut self, delta: isize) {
        let Some(changes) = &mut self.changes else { return };
        let len = changes.history.commits.len();
        let next = changes.history_state.selected().unwrap_or(0).saturating_add_signed(delta).min(len.saturating_sub(1));
        changes.history_state.select(Some(next));
        // Near the end, the next page; once, until it's read.
        if changes.history.more && next + 20 >= len && changes.history_limit <= len {
            changes.history_limit += crate::review::HISTORY_PAGE;
            self.reload_changes();
        }
    }

    fn history_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.history_step(1),
            KeyCode::Up | KeyCode::Char('k') => self.history_step(-1),
            KeyCode::PageDown => self.history_step(10),
            KeyCode::PageUp => self.history_step(-10),
            KeyCode::Home | KeyCode::Char('g') => self.history_step(isize::MIN / 2),
            KeyCode::End | KeyCode::Char('G') => self.history_step(isize::MAX / 2),
            KeyCode::Enter => self.open_commit(),
            KeyCode::Esc => self.set_focus(Focus::Terminal),
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.prefixed = false;
        }
        if self.help {
            self.help = !matches!(mouse.kind, MouseEventKind::Down(_));
            return;
        }
        if self.repos.is_some() || self.search.is_some() {
            self.popup_mouse(mouse);
            return;
        }
        if self.prompt.is_some() || self.asking.is_some() {
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                let pos = Position::new(mouse.column, mouse.row);
                self.prompt = None;
                if let Some((_, confirm)) = self.asking.take()
                    && self.areas.yes.contains(pos)
                    && self.confirmed(confirm)
                {
                    self.quit = true;
                }
            }
            return;
        }
        let pos = Position::new(mouse.column, mouse.row);
        let areas = self.areas.clone();
        let down = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
        let (wheel, sideways): (i32, isize) = match mouse.kind {
            MouseEventKind::ScrollUp if shift => (0, -6),
            MouseEventKind::ScrollDown if shift => (0, 6),
            MouseEventKind::ScrollUp => (-3, 0),
            MouseEventKind::ScrollDown => (3, 0),
            MouseEventKind::ScrollLeft => (0, -4),
            MouseEventKind::ScrollRight => (0, 4),
            _ => (0, 0),
        };
        if let Some(drag) = self.dragging {
            match mouse.kind {
                MouseEventKind::Drag(MouseButton::Left) => match drag {
                    Drag::Sidebar => self.left_width = Some(pos.x + 1),
                    Drag::Right => self.right_width = Some(areas.width.saturating_sub(pos.x)),
                    Drag::Tree(x) => self.tree_width = Some(pos.x.saturating_sub(x).max(8)),
                },
                _ => self.dragging = None,
            }
            return;
        }
        if down {
            let tree = areas.tree_dividers.iter().find(|(r, _)| r.contains(pos)).map(|&(_, x)| Drag::Tree(x));
            self.dragging = tree.or_else(|| {
                if areas.left_divider.contains(pos) {
                    Some(Drag::Sidebar)
                } else {
                    areas.divider.contains(pos).then_some(Drag::Right)
                }
            });
            if self.dragging.is_some() {
                return;
            }
        }
        if let Some(selecting) = self.selecting.as_mut().filter(|s| s.dragging)
            && matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left))
        {
            let term = areas.term;
            let cell = (pos.y.clamp(term.y, term.bottom().saturating_sub(1)) - term.y, pos.x.clamp(term.x, term.right().saturating_sub(1)) - term.x);
            selecting.to = cell;
            if mouse.kind == MouseEventKind::Up(MouseButton::Left) {
                selecting.dragging = false;
                let (id, from, to) = (selecting.id, selecting.from, selecting.to);
                if from == to {
                    self.selecting = None;
                } else if let Some(text) = self.terms.iter().find(|t| t.id == id).map(|t| t.selection(from, to)) {
                    let lines = text.lines().count().max(1);
                    self.message = Some(if set_clipboard(&text) { format!("copied {lines} line(s)") } else { "could not copy".into() });
                }
            }
            return;
        }
        if down {
            self.selecting = None;
        }
        let double = down && self.last_click.is_some_and(|(at, when)| at.y == pos.y && at.x.abs_diff(pos.x) <= 1 && when.elapsed() < DOUBLE_CLICK);
        if down {
            self.last_click = Some((pos, Instant::now()));
        }
        let row = |area: Rect, offset: usize| (pos.y - area.y) as usize + offset;

        if down && areas.projects.contains(pos) {
            self.set_focus(Focus::Projects);
            let i = row(areas.projects, self.projects_state.offset());
            if i < self.projects().len() {
                self.projects_state.select(Some(i));
                self.on_select();
            }
        } else if down && areas.workspaces.contains(pos) {
            self.set_focus(Focus::Workspaces);
            let i = row(areas.workspaces, self.workspaces_state.offset());
            if i < self.workspace_order().len() {
                self.workspaces_state.select(Some(i));
                self.on_select();
                if double {
                    self.set_focus(Focus::Terminal);
                }
            }
        } else if let Some(&(tab, _)) = areas.tabs.iter().find(|(_, r)| r.contains(pos)) {
            if down {
                match tab {
                    Some(id) => {
                        self.select_term(id);
                        self.set_focus(Focus::Terminal);
                    }
                    None => self.new_terminal(true),
                }
            }
        } else if areas.term.contains(pos) {
            self.mouse_in_terminal(mouse, pos, wheel);
        } else if areas.files_tree.contains(pos) {
            if down {
                self.focus = Focus::Files;
            }
            if down && self.editor.is_none() {
                self.reading = false;
                let w = self.ws_mut();
                let i = row(areas.files_tree, w.files_state.offset());
                if i < w.tree.rows.len() {
                    let again = w.files_state.selected() == Some(i);
                    w.files_state.select(Some(i));
                    self.preview();
                    match self.ws().selected_row().map(|r| (r.is_dir, r.expanded)) {
                        Some((true, expanded)) if again || double => self.fold_tree(!expanded),
                        Some((false, _)) if double => (self.files_expanded, self.reading) = (true, true),
                        _ => {}
                    }
                }
            } else if wheel != 0 && self.editor.is_none() {
                self.tree_step(wheel.signum() as isize);
            }
        } else if areas.files_file.contains(pos) {
            if down {
                self.focus = Focus::Files;
            }
            if down {
                self.reading = self.files_expanded;
            }
            if let Some(editor) = &mut self.editor {
                let (r, column) = ((pos.y - areas.editor.y) as usize, (pos.x.saturating_sub(areas.editor.x)) as usize);
                let column = column.saturating_sub(editor.gutter());
                match mouse.kind {
                    MouseEventKind::Down(MouseButton::Left) => editor.click(r, column, shift),
                    MouseEventKind::Drag(MouseButton::Left) => editor.click(r, column, true),
                    _ if wheel != 0 => editor.scroll_by(wheel as isize),
                    _ if sideways != 0 => editor.scroll_sideways(sideways),
                    _ => {}
                }
            } else if wheel != 0 {
                self.viewer.scroll_by(wheel as isize);
            } else if sideways != 0 {
                self.viewer.scroll_sideways(sideways);
            } else if double && self.viewer.is_open() {
                self.files_expanded = !self.files_expanded;
                self.reading = self.files_expanded;
            }
        } else if areas.changes_tree.contains(pos) {
            if down {
                self.focus = Focus::Changes;
            }
            if down {
                self.reading = false;
            }
            let Some(changes) = &mut self.changes else { return };
            if down {
                let i = row(areas.changes_tree, changes.state.offset());
                if i < changes.rows.len() {
                    let again = changes.state.selected() == Some(i);
                    changes.state.select(Some(i));
                    changes.follow();
                    match changes.selected().map(|r| (r.is_dir, r.expanded)) {
                        Some((true, expanded)) if again || double => changes.fold(!expanded),
                        Some((false, _)) if double => (self.changes_expanded, self.reading) = (true, true),
                        _ => {}
                    }
                }
            } else if wheel != 0 {
                changes.step(wheel.signum() as isize);
            }
        } else if areas.changes_ruler.contains(pos) {
            if let Some(changes) = &mut self.changes
                && matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left) | MouseEventKind::Drag(MouseButton::Left))
            {
                changes.jump_to((pos.y - areas.changes_ruler.y) as usize, areas.changes_ruler.height as usize);
            }
        } else if areas.changes_file.contains(pos) {
            if down {
                self.focus = Focus::Changes;
                self.reading = self.changes_expanded;
            }
            if let Some(changes) = &mut self.changes {
                if sideways != 0 {
                    changes.scroll_sideways(sideways);
                } else if wheel != 0 {
                    changes.scroll_by(wheel as isize);
                } else if double && changes.diff.is_some() {
                    self.changes_expanded = !self.changes_expanded;
                    self.reading = self.changes_expanded;
                }
            }
        } else if areas.history.contains(pos) {
            if down {
                self.focus = Focus::History;
                self.reading = false;
                let Some(changes) = &mut self.changes else { return };
                let i = row(areas.history, changes.history_state.offset());
                if i < changes.history.commits.len() {
                    changes.history_state.select(Some(i));
                    if double {
                        self.open_commit();
                    }
                }
            } else if wheel != 0 {
                self.history_step(wheel.signum() as isize);
            }
        }
    }

    fn popup_mouse(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        let (popup, list) = (self.areas.popup, self.areas.popup_list);
        let down = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        let delta = match mouse.kind {
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::ScrollDown => 1,
            _ if down && list.contains(pos) => {
                let offset_and_at = match (&self.search, &self.repos) {
                    (Some(s), _) | (None, Some(s)) => (s.state.offset(), s.state.selected().unwrap_or(0)),
                    (None, None) => return,
                };
                (pos.y - list.y) as isize + offset_and_at.0 as isize - offset_and_at.1 as isize
            }
            _ if down && !popup.contains(pos) && !self.no_projects => {
                (self.search, self.repos) = (None, None);
                return;
            }
            _ => return,
        };
        let double = down && self.last_click.is_some_and(|(at, when)| at == pos && when.elapsed() < DOUBLE_CLICK);
        if down {
            self.last_click = Some((pos, Instant::now()));
        }
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        if self.search.is_some() {
            self.search_step(delta);
            if double {
                self.search_key(enter);
            }
        } else if let Some(repos) = &mut self.repos {
            repos.step(delta);
            if double {
                self.repos_key(enter);
            }
        }
    }

    fn mouse_in_terminal(&mut self, mouse: MouseEvent, pos: Position, wheel: i32) {
        let area = self.areas.term;
        let Some(id) = self.active else { return };
        let Some(term) = self.terms.iter_mut().find(|t| t.id == id) else { return };
        let (row, col) = (pos.y - area.y, pos.x - area.x);
        let down = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        if down && mouse.modifiers.contains(KeyModifiers::CONTROL) {
            let (word, cwd) = (term.word_at(row, col), term.info.cwd.clone());
            if !word.is_empty() {
                self.open_link(&word, &cwd);
            }
            return;
        }
        if term.wants_mouse() && !mouse.modifiers.contains(KeyModifiers::SHIFT) {
            term.mouse(mouse.kind, col + 1, row + 1, mouse.modifiers);
        } else if wheel != 0 {
            scroll_term(term, wheel as isize);
        } else if down {
            self.selecting = Some(Selecting { id, from: (row, col), to: (row, col), dragging: true });
        }
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.focus = Focus::Terminal;
        }
    }

    fn open_link(&mut self, word: &str, cwd: &Path) {
        if word.starts_with("http://") || word.starts_with("https://") {
            let opened = std::process::Command::new("open").arg(word).status().is_ok_and(|s| s.success());
            self.message = Some(if opened { format!("opened {word}") } else { format!("could not open {word}") });
            return;
        }
        let mut parts = word.split(':');
        let path = PathBuf::from(parts.next().unwrap_or_default());
        let line = parts.next().and_then(|n| n.parse::<usize>().ok()).map(|n| n.saturating_sub(1));
        let path = if path.is_absolute() { path } else { cwd.join(path) };
        if !path.is_file() {
            self.message = Some(format!("no link or file at {word:?}"));
            return;
        }
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        self.editor = None;
        self.search = None;
        self.focus = Focus::Files;
        self.show_file(path, line);
        self.files_expanded = true;
        self.reading = true;
    }

    fn create_workspace(&mut self, name: &str, branch: Option<&str>) {
        let (repo, name, branch, tx) = (self.new_in.clone(), name.to_string(), branch.map(str::to_string), self.tx.clone());
        // Asked twice: the first copy is still being made.
        if !self.copying.insert((repo.clone(), name.clone())) {
            self.message = Some(format!("workspace {name} is being made already"));
            return;
        }
        self.load_workspaces();
        if let Some(i) = self.workspaces.iter().position(|w| w.root == worktree::root_of(&repo, &name)) {
            self.set_current(i);
        }
        std::thread::spawn(move || {
            let made = worktree::config(&repo, &repo).and_then(|(config, _)| worktree::create(&repo, &name, branch.as_deref(), &config));
            let made = made.map(|(s, notes)| (s.root, notes)).map_err(|e| format!("could not make the workspace: {e:#}"));
            let _ = tx.send(Msg::Copied((repo, name), made));
        });
    }

    pub fn on_copied(&mut self, copy: (PathBuf, String), made: Result<(PathBuf, Vec<String>), String>) {
        self.copying.remove(&copy);
        let (root, mut notes) = match made {
            Ok(made) => made,
            Err(e) => {
                self.message = Some(e);
                self.load_workspaces();
                return;
            }
        };
        // Read again: listed before its folder was there.
        if let Some(w) = self.workspaces.iter_mut().find(|w| w.root == root) {
            *w = Workspace::new(root.clone(), w.repo.clone());
        }
        self.load_workspaces();
        let Some(wt) = self.worktrees.iter().find(|s| s.root == root).cloned() else { return };
        // Emptied: not an older setup's. Asked for first, so it is the workspace's terminal rather than one more.
        let log = setup_log(&wt);
        let _ = std::fs::write(&log, "");
        self.spawn_in(root.clone(), &setup_viewer().to_string_lossy(), &[log.to_string_lossy().into_owned()], true);
        if let Some(i) = self.workspaces.iter().position(|w| w.root == root) {
            // Shown already, without its folder: shown again, with it.
            if i == self.current {
                self.current = usize::MAX;
            }
            self.set_current(i);
        }
        let config = match worktree::config(&wt.repo, &root) {
            Ok((config, warnings)) => {
                notes.extend(warnings.iter().map(|w| format!("! {w}")));
                config
            }
            Err(e) => {
                let error = format!("workspace {}: could not read its settings: {e:#}", wt.name);
                let _ = std::fs::write(&log, format!("{error}\n"));
                self.message = Some(error);
                return;
            }
        };
        self.set_up(&wt, config, notes);
    }

    fn set_up(&mut self, wt: &worktree::Worktree, config: worktree::Config, notes: Vec<String>) {
        let root = wt.root.clone();
        let script = worktree::setup_script(wt, &config, &notes);
        let log = setup_log(wt);
        match run_in_background(&script, &root, wt, &log) {
            Ok(mut child) => {
                self.setting_up.insert(root.clone(), child.id());
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let ok = child.wait().is_ok_and(|status| status.success());
                    let _ = tx.send(Msg::SetUp(root, log, ok));
                });
                self.message = Some(format!("workspace {} is yours; its setup tab shows the setup", wt.name));
            }
            Err(e) => self.message = Some(format!("could not set up workspace {}: {e}", wt.name)),
        }
    }

    pub fn on_set_up(&mut self, root: PathBuf, log: PathBuf, ok: bool) {
        self.setting_up.remove(&root);
        let name = root.file_name().unwrap_or_default().to_string_lossy().into_owned();
        self.message = Some(if ok {
            format!("workspace {name} is ready")
        } else {
            format!("the setup of workspace {name} failed: its setup tab and {} show why", log.display())
        });
        if !root.is_dir() || self.removing.contains(&root) {
            return;
        }
        if self.ws().root != root {
            self.setup_ended.insert(root.clone(), ok);
            let place = self.place(&root).unwrap_or(name);
            notify(&if ok { format!("{place} is ready") } else { format!("the setup of {place} failed") });
        }
        let token = self.spawn_in(root, &shell(), &[], false);
        if ok {
            self.handing_over.insert(token);
        }
    }

    fn remove_workspace(&mut self, root: &Path) {
        let Some(wt) = self.worktrees.iter().find(|s| s.root == root).cloned() else { return };
        self.remove(wt, true);
    }

    fn remove(&mut self, wt: worktree::Worktree, asked: bool) {
        let Ok(exe) = std::env::current_exe() else { return };
        if self.removing.contains(&wt.root) {
            return;
        }
        let script = worktree::removal_script(&wt, &exe);
        let log = logs().join(format!("remove-{}.log", wt.project));
        let mut child = match run_in_background(&script, &wt.repo, &wt, &log) {
            Ok(child) => child,
            Err(e) => {
                self.message = Some(format!("could not delete workspace {}: {e}", wt.name));
                return;
            }
        };
        let (tx, root) = (self.tx.clone(), wt.root.clone());
        std::thread::spawn(move || {
            let ok = child.wait().is_ok_and(|status| status.success());
            let _ = tx.send(Msg::Removed(root, log, ok));
        });
        if let Some(main) = self.workspaces.iter().position(|w| w.root == wt.repo).filter(|_| asked) {
            self.set_current(main);
        }
        self.removing.insert(wt.root.clone());
        let ids: Vec<usize> = self.terms.iter().filter(|t| t.info.cwd == wt.root).map(|t| t.id).collect();
        for id in ids {
            self.close_terminal(id);
        }
        self.load_workspaces();
        self.message = Some(if asked {
            format!("workspace {} deleted", wt.name)
        } else {
            format!("workspace {}'s folder was deleted: removing its containers and worktree", wt.name)
        });
    }

    pub fn on_removed(&mut self, root: PathBuf, log: PathBuf, ok: bool) {
        self.removing.remove(&root);
        if ok {
            self.gone.remove(&root);
        }
        self.load_workspaces();
        if ok {
            let _ = std::fs::remove_file(&log);
        } else {
            let name = root.file_name().unwrap_or_default().to_string_lossy();
            self.message = Some(format!("could not delete workspace {name}; see {}", log.display()));
        }
    }
}

/// Not in the state folder itself, which is watched for the agents' events.
fn logs() -> PathBuf {
    let dir = crate::agents::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

fn is_setup_tab(term: &Term) -> bool {
    term.info.name == "setup"
}

fn setup_log(wt: &worktree::Worktree) -> PathBuf {
    logs().join(format!("setup-{}.log", wt.project))
}

/// A program named `setup`, so its tab is.
fn setup_viewer() -> PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = logs().join("setup");
    let _ = std::fs::write(&path, "#!/bin/sh\nexec tail -n +1 -f \"$1\"\n");
    let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755));
    path
}

/// In its own process group, so it outlives Pulpo.
fn run_in_background(script: &str, dir: &Path, wt: &worktree::Worktree, log: &Path) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let out = std::fs::File::create(log)?;
    std::process::Command::new("sh")
        .args(["-c", script])
        .current_dir(dir)
        .envs(worktree::env(wt))
        .stdin(std::process::Stdio::null())
        .stdout(out.try_clone()?)
        .stderr(out)
        .process_group(0)
        .spawn()
}

enum Scroll {
    Rows(isize),
    Sideways(isize),
}

fn scroll_keys(key: KeyEvent, height: usize) -> Option<Scroll> {
    let half = (height / 2).max(1) as isize;
    Some(match key.code {
        KeyCode::Down | KeyCode::Char('j') => Scroll::Rows(1),
        KeyCode::Up | KeyCode::Char('k') => Scroll::Rows(-1),
        KeyCode::PageDown | KeyCode::Char('d' | ' ') => Scroll::Rows(half),
        KeyCode::PageUp | KeyCode::Char('u') => Scroll::Rows(-half),
        KeyCode::Home | KeyCode::Char('g') => Scroll::Rows(isize::MIN / 2),
        KeyCode::End | KeyCode::Char('G') => Scroll::Rows(isize::MAX / 2),
        KeyCode::Left | KeyCode::Char('h') => Scroll::Sideways(-8),
        KeyCode::Right | KeyCode::Char('l') => Scroll::Sideways(8),
        KeyCode::Char('0') => Scroll::Sideways(0),
        _ => return None,
    })
}

/// Like Ghostty, the wheel sends arrow keys to full-screen programs (less, man...).
fn scroll_term(term: &mut Term, rows: isize) {
    if term.alternate_screen() {
        let arrow = if rows < 0 { KeyCode::Up } else { KeyCode::Down };
        for _ in 0..rows.abs() {
            term.key(KeyEvent::new(arrow, KeyModifiers::NONE));
        }
    } else {
        term.scroll(-rows);
    }
}

fn notify(text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
    crate::term::to_host(format!("\x1b]9;{text}\x07").as_bytes());
}

fn set_clipboard(text: &str) -> bool {
    use std::io::Write;
    std::process::Command::new("pbcopy")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            child.stdin.take().map_or(Ok(()), |mut stdin| stdin.write_all(text.as_bytes()))?;
            child.wait()
        })
        .is_ok_and(|status| status.success())
}

fn clipboard() -> Option<String> {
    let out = std::process::Command::new("pbpaste").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}

/// Moves `item` up past the rows less urgent than it, never down.
fn rise<T: PartialEq>(order: &mut Vec<T>, item: T, urgency: impl Fn(&T) -> u8) {
    let now = urgency(&item);
    let target = order.iter().filter(|r| **r != item).take_while(|r| urgency(r) >= now).count();
    match order.iter().position(|r| *r == item) {
        Some(at) if at <= target => {}
        Some(at) => {
            let item = order.remove(at);
            order.insert(target, item);
        }
        None => order.insert(target, item),
    }
}

#[cfg(test)]
mod tests {
    use super::rise;

    #[test]
    fn rows_only_move_up() {
        let mut urgency = std::collections::HashMap::from([('a', 0), ('b', 0), ('c', 0), ('d', 0)]);
        let mut order = Vec::new();
        for item in ['a', 'b', 'c', 'd'] {
            rise(&mut order, item, |r| urgency[r]);
        }
        assert_eq!(order, ['a', 'b', 'c', 'd']);
        urgency.insert('c', 2);
        rise(&mut order, 'c', |r| urgency[r]);
        assert_eq!(order, ['c', 'a', 'b', 'd']);
        // Read: it stays first.
        urgency.insert('c', 0);
        assert_eq!(order, ['c', 'a', 'b', 'd']);
        // Working goes above the quiet rows, under the waiting ones.
        urgency.extend([('a', 2), ('d', 1)]);
        rise(&mut order, 'a', |r| urgency[r]);
        rise(&mut order, 'd', |r| urgency[r]);
        assert_eq!(order, ['a', 'd', 'c', 'b']);
        // Working again where it already is doesn't move it.
        rise(&mut order, 'd', |r| urgency[r]);
        assert_eq!(order, ['a', 'd', 'c', 'b']);
    }
}
