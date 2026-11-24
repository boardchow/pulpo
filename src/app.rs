use std::collections::{BTreeMap, HashMap, HashSet};
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
use crate::linear::{self, Issue};
use crate::panes::{self, Dir, Node};
use crate::proto::{Info, PROTOCOL, ToClient, ToServer};
use crate::review::{Changes, Comment, Shown};
use crate::search::{Mode, Search, expand_home};
use crate::viewer::{Source, Viewer};
use crate::worktree;
use crate::status::Status;
use crate::term::Term;
use crate::project;
use crate::workspace::Workspace;

const NOTICE_EVERY: Duration = Duration::from_secs(10);
const MESSAGE_FOR: Duration = Duration::from_secs(5);
const UNSAVED: &str = "unsaved changes in the editor: ⌃s saves them, esc twice throws them away";
const PREFIX_FOR: Duration = Duration::from_secs(3);
const REPEAT_FOR: Duration = Duration::from_secs(1);
const DETAIL_AFTER: Duration = Duration::from_millis(250);
const SEARCH_AFTER: Duration = Duration::from_millis(350);
const MIN_SPLIT_WIDTH: u16 = 40;
const MIN_SPLIT_HEIGHT: u16 = 10;
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// How long a terminal just switched to may wait for its snapshot before it's drawn empty.
const SNAPSHOT: Duration = Duration::from_millis(100);
/// A resumed agent reprints its conversation meanwhile: that isn't news.
const RESUMING: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Focus {
    Terminal,
    Review,
    Files,
}

pub enum FileAsk {
    New(PathBuf),
    Rename(PathBuf),
}

pub struct FileInput {
    pub ask: FileAsk,
    pub text: String,
}

#[derive(Clone)]
pub enum Pick {
    Go(usize),
    New,
}

pub struct Naming {
    pub repo: PathBuf,
    pub name: String,
    pub branch: String,
    pub on_branch: bool,
}

pub struct Issues {
    pub query: String,
    pub state: ListState,
    pub loaded: Option<Result<linear::Loaded, String>>,
    /// Everyone's issues rather than yours, when nothing is typed.
    pub everyone: bool,
    pub reading: bool,
    pub scroll: usize,
    pub details: HashMap<String, Result<linear::Detail, String>>,
    pub fetching: HashSet<String>,
    resting: Option<(String, Instant)>,
    /// What Linear's search found for a term, and the term last asked.
    found: Option<(String, Result<Vec<Issue>, String>)>,
    asked: Option<String>,
    typed_at: Instant,
    /// The workspace it was opened from, whose own issue is shown first.
    from: String,
}

impl Issues {
    pub fn selected(&self) -> Option<&Issue> {
        self.state.selected().and_then(|i| self.visible().get(i).copied())
    }

    /// Selects the issue the workspace is named after, widening the filters until it shows.
    fn show_own(&mut self) {
        let Some(Ok(loaded)) = &self.loaded else { return };
        let Some(own) = loaded.issues.iter().find(|i| i.id.to_lowercase() == self.from).filter(|_| self.query.is_empty()) else { return };
        let everyone = own.assignee != loaded.me;
        if let Some(row) = loaded.visible("", everyone).iter().position(|i| *i == own) {
            self.everyone = everyone;
            self.state.select(Some(row));
        }
    }

    /// The selected issue's detail isn't here yet.
    pub fn loading(&self) -> bool {
        self.selected().is_some_and(|i| !self.details.contains_key(&i.id))
    }

    fn parsed(&self) -> linear::Query {
        linear::Query::parse(&self.query, self.loaded.as_ref().and_then(|l| l.as_ref().ok()).map_or("", |l| l.me.as_str()))
    }

    /// What Linear is asked for.
    fn term(&self) -> String {
        self.parsed().key()
    }

    pub fn visible(&self) -> Vec<&Issue> {
        let Some(Ok(loaded)) = &self.loaded else { return Vec::new() };
        match &self.found {
            Some((term, Ok(found))) if !term.is_empty() && *term == self.term() => found.iter().collect(),
            _ => loaded.visible(&self.query, self.everyone),
        }
    }

    fn search_due(&self) -> bool {
        let term = self.term();
        !term.is_empty() && self.asked.as_deref() != Some(&term)
    }

    /// What to say beside the search: it is on its way, or it failed.
    pub fn search_note(&self) -> String {
        let term = self.term();
        match &self.found {
            Some((found, Err(error))) if *found == term && !term.is_empty() => format!("Linear search failed: {error}"),
            Some((found, Ok(_))) if *found == term => String::new(),
            _ if term.is_empty() => String::new(),
            _ => "searching Linear…".into(),
        }
    }

    fn step(&mut self, delta: isize) {
        let last = self.visible().len().saturating_sub(1) as isize;
        self.state.select(Some((self.state.selected().unwrap_or(0) as isize + delta).clamp(0, last) as usize));
    }
}

pub struct Picker {
    pub query: String,
    pub state: ListState,
}

pub enum Confirm {
    DeleteFile(PathBuf),
    DeleteWorkspace(PathBuf),
    HideProject(PathBuf),
    CloseTerminal(usize),
    DeleteComment(Comment),
    StopAll,
}

#[derive(Default, Clone)]
pub struct Areas {
    pub name: Rect,
    pub waiting: Rect,
    pub to_review: Rect,
    pub panes: Vec<(usize, Rect)>,
    /// The sidebar's rows: a workspace, or none for "new workspace".
    pub sidebar: Vec<(Option<usize>, Rect)>,
    pub review: Rect,
    pub changes_tree: Rect,
    pub changes_file: Rect,
    pub changes_ruler: Rect,
    pub editor: Rect,
    pub files_tree: Rect,
    pub files_file: Rect,
    pub yes: Rect,
    pub popup: Rect,
    pub popup_list: Rect,
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
    pub no_projects: bool,
    pub focus: Focus,
    pub agents: Agents,
    pub terms: Vec<Term>,
    pub active: Option<usize>,
    pub prefixed: bool,
    prefixed_at: Option<Instant>,
    pub quit: bool,
    pub help: bool,
    pub asking: Option<(String, Confirm)>,
    pub message: Option<String>,
    message_since: Option<(String, Instant)>,
    pub picker: Option<Picker>,
    pub issues: Option<Issues>,
    /// The issue the repository being picked is for.
    pending_issue: Option<Issue>,
    /// The issue each workspace being made starts its agent on, by folder.
    starts: HashMap<PathBuf, String>,
    pub naming: Option<Naming>,
    /// Only while Files is open.
    pub search: Option<Search>,
    pub file_input: Option<FileInput>,
    pub viewer: Viewer,
    pub repos: Option<Search>,
    pub editor: Option<Editor>,
    discard: bool,
    pub writing: Option<Comment>,
    /// A byte index in its text.
    pub writing_at: usize,
    /// The comments of workspaces not on screen.
    drafts: HashMap<PathBuf, Vec<Comment>>,
    pub reading: bool,
    pub changes: Option<Changes>,
    pub selecting: Option<Selecting>,
    last_click: Option<(Position, Instant)>,
    pub keymap: Keymap,
    pub areas: Areas,
    pub setting_up: HashMap<PathBuf, u32>,
    /// Whether each worked, until you go there.
    setup_ended: HashMap<PathBuf, bool>,
    copying: HashSet<(PathBuf, String)>,
    /// Repositories that become projects only if the workspace made in them is.
    new_projects: HashSet<PathBuf>,
    removing: HashSet<PathBuf>,
    gone: HashSet<PathBuf>,
    last_term: HashMap<PathBuf, String>,
    last_workspace: HashMap<PathBuf, PathBuf>,
    saved: Layout,
    /// Panes from before, until their terminals are back.
    old_panes: BTreeMap<PathBuf, Node<String>>,
    ready: bool,
    /// The tag each reopened terminal had, and whether it was waiting for you.
    reopening: HashMap<u64, (String, bool)>,
    resumed: HashMap<usize, Instant>,
    link: Link,
    /// Terminals asked for and not attached yet: whether to focus them, and where.
    spawning: HashMap<u64, (bool, PathBuf)>,
    /// The shell a setup that worked hands its tab's place to.
    handing_over: HashSet<u64>,
    /// When each folder last got a shell of its own, so one that exits at once isn't started again and again.
    kept: HashMap<PathBuf, Instant>,
    /// The workspaces in the picker, top first, and each row's urgency when last placed.
    order: Vec<PathBuf>,
    placed: HashMap<PathBuf, u8>,
    next_token: u64,
    upgrade_asked: bool,
    pub release: Option<String>,
    pub updating: bool,
    /// The terminal the server sends output for: the one on screen.
    watching: Vec<usize>,
    panes: HashMap<PathBuf, Node<usize>>,
    zoomed: bool,
    splits: HashMap<u64, bool>,
    repeat: Option<Instant>,
    watched_at: Instant,
    reconnected: Option<usize>,
    last_reconnect: Option<Instant>,
    offline: bool,
    term_size: (u16, u16),
    fs_dirty: bool,
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
            no_projects: false,
            focus: Focus::Terminal,
            agents,
            terms: Vec::new(),
            active: None,
            prefixed: false,
            prefixed_at: None,
            quit: false,
            help: false,
            asking: None,
            message: keys_error.or_else(|| {
                (!crate::agents::hooks_installed())
                    .then(|| "run `pulpo hooks install` to see what agents run and ask in each terminal".into())
            }),
            message_since: None,
            picker: None,
            issues: None,
            pending_issue: None,
            starts: HashMap::new(),
            naming: None,
            search: None,
            file_input: None,
            viewer: Viewer::default(),
            repos: None,
            editor: None,
            discard: false,
            writing: None,
            writing_at: 0,
            drafts: HashMap::new(),
            reading: false,
            changes: None,
            selecting: None,
            last_click: None,
            keymap,
            areas: Areas::default(),
            setting_up: HashMap::new(),
            setup_ended: HashMap::new(),
            copying: HashSet::new(),
            new_projects: HashSet::new(),
            removing: HashSet::new(),
            gone: HashSet::new(),
            last_term: layout.terminals.clone().into_iter().collect(),
            last_workspace: layout.workspaces.clone().into_iter().collect(),
            old_panes: layout.panes.clone(),
            saved: layout.clone(),
            ready: false,
            reopening: HashMap::new(),
            resumed: HashMap::new(),
            link,
            spawning: HashMap::new(),
            handing_over: HashSet::new(),
            kept: HashMap::new(),
            order: Vec::new(),
            placed: HashMap::new(),
            // Far above any terminal id, so the two never mix.
            next_token: 1 << 40,
            upgrade_asked: false,
            release: None,
            updating: false,
            watching: Vec::new(),
            panes: HashMap::new(),
            zoomed: false,
            splits: HashMap::new(),
            repeat: None,
            watched_at: Instant::now(),
            reconnected: None,
            last_reconnect: None,
            offline: false,
            term_size: (24, 80),
            fs_dirty: false,
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
                return false;
            }
            let approved = self.changes.take().map(|changes| {
                self.drafts.insert(changes.root, changes.comments);
                changes.approved
            });
            self.writing = None;
            self.current = i;
            self.editor = None;
            self.focus = Focus::Terminal;
            self.viewer.close();
            (self.search, self.file_input) = (None, None);
            self.selecting = None;
            self.zoomed = false;
            if !self.workspaces[i].reading_status() {
                self.read_status(i);
            }
            let mut changes = Changes::new(&self.workspaces[i].root);
            changes.comments = self.drafts.remove(&changes.root).unwrap_or_default();
            changes.approved = approved.unwrap_or_else(crate::review::load_approved);
            self.changes = Some(changes);
            self.reload_changes();
            self.restore_term();
            if self.setup_ended.remove(&self.workspaces[i].root) == Some(false) {
                if let Some(id) = self.workspace_terms().into_iter().map(|t| &self.terms[t]).find(|t| is_setup_tab(t)).map(|t| t.id) {
                    self.select_term(id);
                }
                self.message = Some("this workspace's setup failed: its setup tab shows why".into());
            }
            self.keep_a_terminal();
        }
        let ws = &self.workspaces[i];
        self.last_workspace.insert(ws.repo.clone(), ws.root.clone());
        true
    }

    fn editing(&self) -> bool {
        self.editor.as_ref().is_some_and(|e| e.dirty)
    }

    /// The workspaces Pulpo made, as `reorder` last left them.
    /// A workspace Pulpo made, or a project's own folder while terminals run there, so they stay reachable.
    fn is_listed(&self, i: usize) -> bool {
        let w = &self.workspaces[i];
        w.is_worktree() || (!self.no_projects && self.terms.iter().any(|t| t.info.cwd == w.root))
    }

    pub fn workspace_order(&self) -> Vec<usize> {
        let mut all: Vec<usize> = self.order.iter().filter_map(|root| self.workspaces.iter().position(|w| &w.root == root)).filter(|&i| self.is_listed(i)).collect();
        let unplaced: Vec<usize> = (0..self.workspaces.len()).filter(|&i| self.is_listed(i) && !all.contains(&i)).collect();
        all.extend(unplaced);
        all
    }

    /// A workspace goes up when it starts waiting or working, and stays put when it calms down.
    pub fn reorder(&mut self) {
        let urgency: HashMap<PathBuf, u8> = (0..self.workspaces.len())
            .filter(|&i| self.is_listed(i))
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

    pub fn on_fs(&mut self, paths: &[PathBuf]) {
        for path in paths {
            // A build writes thousands of files: once marked, skip matching the .gitignore.
            self.fs_dirty = self.fs_dirty || self.ws().is_affected(path);
            let added_or_removed = self.workspaces.iter().any(|w| {
                path.strip_prefix(w.repo.join(".git/worktrees")).is_ok_and(|rel| rel.components().count() == 1)
            });
            let worktree_gone = self.worktrees.iter().any(|s| path.starts_with(&s.root) && !s.root.exists());
            self.reload_list |= added_or_removed || worktree_gone;
        }
    }

    pub fn fs_pending(&self) -> bool {
        self.fs_dirty || self.reload_list
    }

    pub fn flush_fs(&mut self) {
        if std::mem::take(&mut self.reload_list) {
            self.load_workspaces();
        }
        if std::mem::take(&mut self.fs_dirty) {
            self.read_status(self.current);
            self.reload_changes();
            if self.focus == Focus::Files {
                self.ws_mut().refresh();
                if let Some(next) = self.viewer.reload() {
                    self.load_preview(next);
                }
                if let Some(search) = &mut self.search {
                    search.rescan(&self.tx);
                }
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
        if i == self.current {
            self.viewer.blame_again();
        }
        if self.workspaces[i].end_status(status) {
            self.read_status(i);
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
        let Some(changes) = &mut self.changes else { return };
        changes.apply(read);
        let Some(draft) = &mut self.writing else { return };
        if changes.diff.as_ref().is_some_and(|(path, _)| *path == draft.path) {
            changes.follow_draft(draft);
        } else {
            // Its file left the review: kept, the comment keeps the file there.
            self.keep_writing();
        }
    }

    pub fn settle(&mut self) {
        self.arrange();
        let shown = self.shown();
        if self.watching != shown {
            for term in self.terms.iter_mut().filter(|t| self.watching.contains(&t.id) && !shown.contains(&t.id)) {
                term.unmirror();
            }
            self.watched_at = Instant::now();
            self.link.send(&ToServer::Watch { ids: shown.iter().map(|&id| id as u32).collect() });
            self.watching = shown;
        }
        if self.focus == Focus::Files && self.reading && self.editor.is_none()
            && let Some((path, stamp)) = self.viewer.blame_due()
        {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let blame = git::blame(&path);
                let _ = tx.send(Msg::Blamed(path, stamp, blame));
            });
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

    /// The one on screen first.
    fn agent_term(&self) -> Option<usize> {
        // An agent started as the terminal's own command shares the shell's process group, so `running` can't tell.
        let agent = |t: &Term| self.agents.session_for_tab(&t.info.tag).is_some();
        let terms = self.workspace_terms();
        let on_screen = terms.iter().map(|&t| &self.terms[t]).find(|t| Some(t.id) == self.active);
        on_screen.filter(|t| agent(t)).or_else(|| terms.iter().map(|&t| &self.terms[t]).find(|t| agent(t))).map(|t| t.id)
    }

    fn send_review(&mut self) {
        let Some(id) = self.agent_term() else {
            self.message = Some("no agent running in this workspace: start one, then c ⏎ sends the review".into());
            return;
        };
        let Some(changes) = &mut self.changes else { return };
        changes.follow_all();
        let review = changes.review();
        let Some(term) = self.terms.iter_mut().find(|t| t.id == id) else { return };
        // The agent's screen may not be mirrored, but an agent always takes bracketed paste.
        term.paste_bracketed(&review);
        if let Some(changes) = &mut self.changes {
            changes.comments.clear();
        }
        self.select_term(id);
        self.close_review();
        self.message = Some("review typed in: ⏎ sends it".into());
    }

    fn finish_review(&mut self) {
        match &self.changes {
            Some(changes) if changes.left() == 0 && !changes.comments.is_empty() => self.send_review(),
            Some(changes) if changes.left() == 0 => self.close_review(),
            _ => {}
        }
    }

    fn open_review(&mut self) {
        self.set_focus(Focus::Review);
        self.reading = true;
        if let Some(changes) = &mut self.changes {
            self.message = changes.first_unapproved();
        }
        self.reload_changes();
    }

    fn close_review(&mut self) {
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        self.keep_writing();
        self.editor = None;
        self.set_focus(Focus::Terminal);
    }

    fn write_comment(&mut self, comment: Option<Comment>) {
        match comment {
            Some(comment) => {
                self.writing_at = comment.text.len();
                self.writing = Some(comment);
                if let Some(changes) = &mut self.changes {
                    changes.reveal = true;
                }
            }
            None if self.reading => self.message = Some("nothing to comment on here".into()),
            None => self.message = Some("comments go on a file, or on its lines".into()),
        }
    }

    fn comment_command(&mut self, c: char) {
        let (reading, p) = (self.reading, self.keymap.label());
        let Some(changes) = &mut self.changes else { return };
        if c == 'n' {
            let comment = changes.commenting(reading);
            return self.write_comment(comment);
        }
        let (here, above, root) = (changes.comment_at(reading), !changes.above().is_empty(), changes.root.clone());
        match here {
            Some(comment) => {
                let at = comment.at(&root);
                self.ask(format!("delete the comment on {at}?"), Confirm::DeleteComment(comment));
            }
            _ if reading && above => self.message = Some(format!("no comment here; for those above the file, tab to the tree, then {p} d")),
            _ if reading => self.message = Some("no comment here: c makes one".into()),
            _ => self.message = Some(format!("no comment here: {p} n makes one")),
        }
    }

    fn writing_key(&mut self, key: KeyEvent) {
        let Some(comment) = &mut self.writing else { return };
        match key.code {
            KeyCode::Esc => self.writing = None,
            KeyCode::Enter if key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT) => self.insert_in_comment("\n"),
            // ⌥⏎ only reaches Pulpo when the terminal sends Option as Meta.
            KeyCode::Char('j') if key.modifiers.contains(KeyModifiers::CONTROL) => self.insert_in_comment("\n"),
            KeyCode::Enter => {
                let left = self.changes.as_ref().map(Changes::left);
                self.keep_writing();
                if let Some(changes) = self.changes.as_mut().filter(|c| self.reading && (Some(c.left()) < left || c.on_commented())) {
                    self.message = changes.next_change(true, true);
                }
                if self.reading {
                    self.finish_review();
                }
            }
            _ => {
                input::edit_at(&mut comment.text, &mut self.writing_at, key);
            }
        }
        if let Some(changes) = &mut self.changes {
            changes.reveal = true;
        }
    }

    fn insert_in_comment(&mut self, text: &str) {
        if let Some(comment) = &mut self.writing {
            comment.text.insert_str(self.writing_at, text);
            self.writing_at += text.len();
        }
    }

    fn keep_writing(&mut self) {
        if let Some((comment, changes)) = self.writing.take().zip(self.changes.as_mut()) {
            changes.keep(comment);
        }
    }

    fn add_repo(&mut self, input: &str, typed: bool) -> Option<PathBuf> {
        let path = expand_home(input);
        if typed
            && path.is_absolute()
            && !path.exists()
            && let Err(e) = git::init(&path)
        {
            self.message = Some(e);
            if !path.exists() {
                return None;
            }
        }
        let Some(repo) = git::toplevel(&path).and_then(|top| git::main_repo(&top)) else {
            self.message = Some(format!("not a git repository: {}", path.display()));
            return None;
        };
        Some(repo)
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
        let tag = |id: &usize| self.terms.iter().find(|t| t.id == *id).map(|t| t.info.tag.clone());
        let mut panes = self.old_panes.clone();
        panes.extend(self.panes.iter().filter_map(|(root, tree)| Some((root.clone(), tree.filter_map(&tag)?))));
        Layout {
            current: Some(self.ws().root.clone()),
            workspaces: self.last_workspace.clone().into_iter().collect(),
            terminals: self.last_term.clone().into_iter().collect(),
            panes,
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
                    for tree in self.old_panes.values_mut() {
                        tree.replace(&old, new.clone());
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
                if let Some(down) = token.and_then(|t| self.splits.remove(&t)) {
                    let root = self.ws().root.clone();
                    if let (Some(at), Some(tree)) = (self.active, self.panes.get_mut(&root)) {
                        tree.split(&at, id, down);
                    }
                    self.zoomed = false;
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
                if self.watching.contains(&(id as usize))
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
        self.areas.panes.iter().any(|(pane, _)| *pane == id as usize)
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
        self.old_panes = self.layout().panes;
        self.ready = false;
        self.terms.clear();
        self.spawning.clear();
        self.active = None;
        self.watching.clear();
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
        if self.writing.is_some() {
            self.insert_in_comment(&text.replace("\r\n", "\n").replace('\r', "\n"));
        } else if let Some(search) = &mut self.repos {
            search.query.push_str(text.trim());
            search.update(&self.tx);
        } else if let Some(view) = self.issues.as_mut().filter(|view| !view.reading) {
            view.query.push_str(text.trim());
            view.state.select(Some(0));
            view.typed_at = Instant::now();
        } else if self.issues.is_some() {
        } else if let Some(picker) = &mut self.picker {
            picker.query.push_str(text.trim());
            picker.state.select(Some(0));
        } else if let Some(naming) = &mut self.naming {
            if naming.on_branch { &mut naming.branch } else { &mut naming.name }.push_str(text.trim());
        } else if let Some(search) = &mut self.search {
            search.query.push_str(text.trim());
            search.update(&self.tx);
        } else if let Some(input) = &mut self.file_input {
            input.text.push_str(text.trim());
        } else if self.focus != Focus::Terminal
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
        self.fetch_resting_issue();
        self.fetch_search();
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
            let place = self.place(&cwd);
            notify(&format!("{label}{}: {detail}", place.map(|p| format!(" in {p}")).unwrap_or_default()));
        }
    }

    pub fn animating(&self) -> bool {
        self.issues.as_ref().is_some_and(|view| view.loading() || view.search_due()) || !self.setting_up.is_empty() || !self.copying.is_empty() || !self.spawning.is_empty() || self.terms.iter().any(|term| self.term_state(term) == State::Running)
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
        if self.ready && self.reopening.is_empty() {
            self.adopt_panes();
        }
    }

    fn adopt_panes(&mut self) {
        for (root, node) in std::mem::take(&mut self.old_panes) {
            let id = |tag: &String| self.terms.iter().find(|t| &t.info.tag == tag).map(|t| t.id);
            if let Some(tree) = node.filter_map(&id) {
                self.panes.insert(root, tree);
            }
        }
    }

    fn cycle_order(&self) -> Vec<usize> {
        self.workspaces.get(self.current).and_then(|w| self.panes.get(&w.root)).map(Node::leaves).unwrap_or_default()
    }

    fn remove_term(&mut self, id: usize) {
        let panes = self.cycle_order();
        self.terms.retain(|t| t.id != id);
        if self.active == Some(id) {
            let at = panes.iter().position(|&t| t == id).unwrap_or(0);
            let next = panes.get(at + 1).or(at.checked_sub(1).and_then(|p| panes.get(p))).copied();
            self.active = None;
            if let Some(next) = next {
                self.select_term(next);
            }
        }
        self.keep_a_terminal();
    }

    pub fn arrange(&mut self) {
        if !self.ready || self.no_projects || self.workspaces.is_empty() {
            return;
        }
        let root = self.ws().root.clone();
        let mut ids: Vec<usize> = self.workspace_terms().into_iter().map(|i| self.terms[i].id).collect();
        ids.sort_by_key(|id| self.active != Some(*id));
        let mut tree = self.panes.remove(&root).and_then(|tree| tree.filter_map(&|id| ids.contains(id).then_some(*id)));
        for id in ids {
            match &mut tree {
                None => tree = Some(Node::Leaf(id)),
                Some(tree) if !tree.contains(&id) => tree.add(id),
                Some(_) => {}
            }
        }
        if let Some(tree) = tree {
            self.panes.insert(root, tree);
        }
    }

    pub fn shown(&self) -> Vec<usize> {
        let tree = self.workspaces.get(self.current).and_then(|w| self.panes.get(&w.root));
        let mut shown: Vec<usize> = tree.filter(|_| !self.zoomed).map(Node::leaves).unwrap_or_default();
        shown.retain(|id| self.terms.iter().any(|t| t.id == *id));
        shown.extend(self.active.filter(|id| !shown.contains(id)));
        shown.sort_unstable();
        shown
    }

    pub fn pane_rects(&self, area: Rect) -> Vec<(usize, Rect)> {
        let tree = self.workspaces.get(self.current).and_then(|w| self.panes.get(&w.root));
        match (tree, self.active) {
            (Some(tree), _) if !self.zoomed => tree.rects(area),
            (_, Some(active)) => vec![(active, area)],
            _ => Vec::new(),
        }
    }

    fn focus_pane(&mut self, dir: Dir) -> bool {
        let Some(from) = self.active.filter(|_| self.focus == Focus::Terminal) else { return false };
        let Some(to) = panes::neighbor(&self.areas.panes, &from, dir) else { return false };
        self.select_term(to);
        self.repeat = Some(Instant::now() + REPEAT_FOR);
        true
    }

    fn split_pane(&mut self, down: Option<bool>) {
        let Some(area) = self.active.and_then(|id| self.areas.panes.iter().find(|(pane, _)| *pane == id)).map(|(_, area)| *area) else { return };
        if self.focus != Focus::Terminal {
            return;
        }
        let down = down.unwrap_or(area.width < 2 * area.height);
        if if down { area.height < MIN_SPLIT_HEIGHT } else { area.width < MIN_SPLIT_WIDTH } {
            self.message = Some("this pane is too small to split".into());
            return;
        }
        let token = self.spawn_in(self.ws().root.clone(), &shell(), &[], true);
        self.splits.insert(token, down);
    }

    fn step_workspace(&mut self, delta: isize) {
        let rows = self.workspace_order();
        let Some(at) = rows.iter().position(|&i| i == self.current) else { return };
        let next = rows[(at as isize + delta).rem_euclid(rows.len() as isize) as usize];
        if next != self.current && self.set_current(next) {
            self.set_focus(Focus::Terminal);
            self.repeat = Some(Instant::now() + REPEAT_FOR);
        }
    }

    fn cycle_pane(&mut self, dir: isize) {
        let panes = self.cycle_order();
        if panes.is_empty() {
            return;
        }
        let at = self.active.and_then(|id| panes.iter().position(|&t| t == id)).unwrap_or(0);
        let next = panes[(at as isize + dir).rem_euclid(panes.len() as isize) as usize];
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
        let pressed = Key::of(key);
        let open = self.repos.is_some() || self.picker.is_some() || self.naming.is_some() || self.search.is_some() || self.file_input.is_some() || self.asking.is_some() || self.help;
        if open && !self.prefixed && self.keymap.is_prefix(pressed) {
            (self.repos, self.picker, self.naming, self.search, self.file_input, self.asking, self.help) = (None, None, None, None, None, None, false);
            self.prefixed = true;
            self.prefixed_at = Some(Instant::now());
            return false;
        }
        if self.help {
            self.help = false;
            return false;
        }
        if self.repos.is_some() {
            self.repos_key(key);
            return false;
        }
        if self.picker.is_some() {
            self.picker_key(key);
            return false;
        }
        if self.naming.is_some() {
            self.naming_key(key);
            return false;
        }
        if self.search.is_some() {
            self.search_key(key);
            return false;
        }
        if self.file_input.is_some() {
            self.file_input_key(key);
            return false;
        }
        if self.writing.is_some() && !self.prefixed && !self.keymap.is_prefix(pressed) {
            self.writing_key(key);
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
        let repeating = self.repeat.take().is_some_and(|until| until > Instant::now());
        if self.keymap.is_prefix(pressed) {
            self.keep_writing();
            self.prefixed = true;
            self.prefixed_at = Some(Instant::now());
            return false;
        }
        if repeating && arrow(pressed).is_some_and(|dir| self.focus_pane(dir)) {
            return false;
        }
        if repeating && let Some(delta) = workspace_step(pressed) {
            self.step_workspace(delta);
            return false;
        }
        if self.issues.is_some() {
            self.issues_key(key);
            return false;
        }
        if !self.offline {
            self.message = None;
        }
        match self.focus {
            Focus::Terminal if self.no_projects => {
                if key.code == KeyCode::Enter {
                    self.open_picker();
                }
            }
            Focus::Terminal => {
                self.selecting = None;
                if let Some(term) = self.term_mut() {
                    term.key(key);
                }
            }
            Focus::Review => self.review_key(key),
            Focus::Files => self.files_key(key),
        }
        false
    }

    fn command(&mut self, key: Key) -> bool {
        if self.issues.is_some() {
            match key.code {
                KeyCode::Char('n') => {
                    self.start_from_issue();
                    return false;
                }
                KeyCode::Char('l') => {
                    self.issues = None;
                    return false;
                }
                KeyCode::Char('?' | 'q' | 'Q') => {}
                _ => self.issues = None,
            }
        }
        if let Some(dir) = arrow(key) {
            self.focus_pane(dir);
            return false;
        }
        if let Some(delta) = workspace_step(key) {
            self.step_workspace(delta);
            return false;
        }
        let KeyCode::Char(c) = key.code else { return false };
        let review = self.focus == Focus::Review;
        let files = self.focus == Focus::Files;
        match c {
            'w' => self.open_picker(),
            'l' => self.open_issues(),
            'f' if files => self.close_files(),
            'f' if self.no_projects => self.open_picker(),
            'f' => self.open_files(),
            'n' if files => self.new_file(),
            'r' if files => self.start_rename(),
            'd' if files => self.ask_delete(),
            'e' if files => self.edit_file(),
            '/' if files => self.start_search(),
            '/' => self.message = Some(format!("{} f first: search is in Files", self.keymap.label())),
            'n' | 'd' if review && self.editor.is_none() => self.comment_command(c),
            'n' | 't' | 'r' | 'e' if self.no_projects => self.open_picker(),
            'n' => self.open_repo_picker(),
            'X' => self.ask_to_delete(self.current),
            't' if self.offline => self.retry_server(),
            't' if self.active.is_some() && self.focus == Focus::Terminal => self.split_pane(None),
            't' => self.new_terminal(true),
            'x' if self.focus == Focus::Terminal => self.close_terminal_here(),
            '\\' => self.split_pane(Some(false)),
            '-' => self.split_pane(Some(true)),
            'z' if self.focus == Focus::Terminal => self.zoomed = !self.zoomed,
            '[' => self.cycle_pane(-1),
            ']' => self.cycle_pane(1),
            'a' => self.next_waiting(),
            '1'..='9' => self.go_to_workspace(c as usize - '1' as usize),
            'r' if review => self.close_review(),
            'r' => self.open_review(),
            'e' if review => self.edit(),
            'e' => self.message = Some(format!("{} r first: the editor opens on the change you're reading", self.keymap.label())),
            '?' => self.help = true,
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
            Confirm::DeleteFile(path) => self.delete(&path),
            Confirm::DeleteWorkspace(root) => self.remove_workspace(&root),
            Confirm::HideProject(root) => self.hide_project(&root),
            Confirm::CloseTerminal(id) => self.close_terminal(id),
            Confirm::DeleteComment(comment) => {
                if let Some(changes) = &mut self.changes {
                    changes.delete(&comment);
                }
            }
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
        self.discard = false;
        self.reading = false;
        match focus {
            Focus::Terminal => {
                self.retry_server();
                self.keep_a_terminal();
            }
            Focus::Review => {
                if let Some(changes) = &mut self.changes {
                    changes.follow();
                }
            }
            Focus::Files => {}
        }
    }

    fn close_terminal_here(&mut self) {
        let Some(term) = self.term() else { return };
        let (id, label) = (term.id, term.label().to_string());
        match term.info.running.clone() {
            Some(program) => self.ask(format!("{program} is running in {label}: close it?"), Confirm::CloseTerminal(id)),
            None => self.close_terminal(id),
        }
    }

    pub fn project_name(&self, i: usize) -> &str {
        let w = &self.workspaces[i];
        self.workspaces.iter().find(|r| r.root == w.repo).map_or(&w.name, |r| &r.name)
    }

    pub fn picks(&self) -> Vec<Pick> {
        let query = self.picker.as_ref().map_or(String::new(), |p| p.query.trim().to_lowercase());
        let shown = |i: &usize| query.is_empty() || [&self.workspaces[*i].name, self.project_name(*i)].iter().any(|n| n.to_lowercase().contains(&query));
        let mut picks: Vec<Pick> = self.workspace_order().into_iter().filter(shown).map(Pick::Go).collect();
        picks.push(Pick::New);
        picks
    }

    fn open_picker(&mut self) {
        if self.no_projects {
            return self.open_repo_picker();
        }
        self.keep_writing();
        self.picker = Some(Picker { query: String::new(), state: ListState::default().with_selected(Some(0)) });
    }

    fn picker_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        match key.code {
            KeyCode::Esc => self.picker = None,
            KeyCode::Enter => self.pick(),
            KeyCode::Up => self.picker_step(-1),
            KeyCode::Down => self.picker_step(1),
            KeyCode::Char('k' | 'p') if ctrl => self.picker_step(-1),
            KeyCode::Char('j' | 'n') if ctrl => self.picker_step(1),
            KeyCode::Char('x') if ctrl => self.ask_to_remove(),
            _ => {
                if let Some(picker) = &mut self.picker
                    && input::edit(&mut picker.query, key)
                {
                    picker.state.select(Some(0));
                }
            }
        }
    }

    fn picker_step(&mut self, delta: isize) {
        let last = self.picks().len().saturating_sub(1);
        if let Some(picker) = &mut self.picker {
            picker.state.select(Some(picker.state.selected().unwrap_or(0).saturating_add_signed(delta).min(last)));
        }
    }

    pub fn picked(&self) -> Option<Pick> {
        self.picks().get(self.picker.as_ref()?.state.selected()?).cloned()
    }

    fn pick(&mut self) {
        let Some(pick) = self.picked() else { return };
        self.picker = None;
        match pick {
            Pick::Go(i) => {
                if self.set_current(i) {
                    self.set_focus(Focus::Terminal);
                }
            }
            Pick::New => self.open_repo_picker(),
        }
    }

    fn ask_to_remove(&mut self) {
        let Some(Pick::Go(i)) = self.picked() else { return };
        self.picker = None;
        self.ask_to_delete(i);
    }

    fn ask_to_delete(&mut self, i: usize) {
        if self.no_projects {
            return;
        }
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        let (root, name) = (self.workspaces[i].root.clone(), self.workspaces[i].name.clone());
        match self.worktrees.iter().find(|s| s.root == root).cloned() {
            Some(wt) => {
                let lost = worktree::unsaved(&wt);
                let lost = if lost.is_empty() { String::new() } else { format!(", losing {}", lost.join(" and ")) };
                let what = if wt.kind == worktree::Kind::Worktree { "folder" } else { "copy" };
                let kept = worktree::kept_branch(&wt).map(|b| format!("; branch {b} stays")).unwrap_or_default();
                let place = self.place(&wt.root).unwrap_or(wt.name.clone());
                let question = format!("delete workspace {place}: its {what}, containers and database{lost}{}{kept}?", self.stopping(&root));
                self.ask(question, Confirm::DeleteWorkspace(wt.root));
            }
            None if self.workspaces[i].is_worktree() => self.message = Some(format!("{name} is still being made")),
            None => self.ask(format!("hide project {name}: close its terminals{}; its folder stays?", self.stopping(&root)), Confirm::HideProject(root)),
        }
    }

    fn stopping(&self, root: &Path) -> String {
        let mut running: Vec<String> = self.terms.iter().filter(|t| t.info.cwd == root).filter_map(|t| t.info.running.clone()).collect();
        running.sort();
        running.dedup();
        if running.is_empty() { String::new() } else { format!(", stopping {}", running.join(" and ")) }
    }

    fn edit(&mut self) {
        if self.editor.is_some() {
            return;
        }
        match self.changes.as_ref().and_then(Changes::editing_at) {
            Some((path, line)) => self.open_editor(path, line),
            None => self.message = Some("select a file to edit".into()),
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
        self.discard = false;
        if self.focus == Focus::Files {
            self.viewer.close();
            self.viewer.open_at(editor.path.clone(), None);
            self.viewer.scroll = editor.scroll;
            self.reading = false;
        } else {
            self.reload_changes();
        }
    }

    fn open_files(&mut self) {
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        self.editor = None;
        self.set_focus(Focus::Files);
        self.ws_mut().refresh();
        self.preview();
    }

    fn close_files(&mut self) {
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        (self.editor, self.search) = (None, None);
        self.viewer.close();
        self.set_focus(Focus::Terminal);
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
        let page = self.viewer.height.saturating_sub(2).max(1) as isize;
        match key.code {
            KeyCode::Esc if self.reading => self.reading = false,
            KeyCode::Esc => self.close_files(),
            KeyCode::Tab | KeyCode::BackTab => self.reading = !self.reading,
            KeyCode::PageDown if self.reading => self.viewer.scroll_by(page),
            KeyCode::PageUp if self.reading => self.viewer.scroll_by(-page),
            KeyCode::Left | KeyCode::Char('h') if self.reading => self.viewer.scroll_sideways(-8),
            KeyCode::Right | KeyCode::Char('l') if self.reading => self.viewer.scroll_sideways(8),
            KeyCode::Char('0') if self.reading => self.viewer.scroll_sideways(0),
            _ if self.reading => {
                if let Some(rows) = scroll_rows(key, self.viewer.height) {
                    self.viewer.move_mark(rows);
                }
            }
            _ => self.tree_key(key),
        }
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
                Some((false, _)) => self.reading = true,
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

    fn new_file(&mut self) {
        let root = self.ws().root.clone();
        let dir = match self.ws().selected_row() {
            Some(row) if row.is_dir => row.path.clone(),
            Some(row) => row.path.parent().map_or(root.clone(), Path::to_path_buf),
            None => root,
        };
        self.file_input = Some(FileInput { ask: FileAsk::New(dir), text: String::new() });
    }

    fn start_rename(&mut self) {
        let Some(path) = self.selected_path() else { return };
        let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
        self.file_input = Some(FileInput { ask: FileAsk::Rename(path), text: name });
    }

    fn file_input_key(&mut self, key: KeyEvent) {
        let Some(input) = &mut self.file_input else { return };
        match key.code {
            KeyCode::Esc => self.file_input = None,
            KeyCode::Enter => {
                let Some(FileInput { ask, text }) = self.file_input.take() else { return };
                match (ask, text.trim()) {
                    (_, "") => {}
                    (FileAsk::New(dir), name) => self.create(&dir, name),
                    (FileAsk::Rename(path), name) => self.rename(&path, name),
                }
            }
            _ => {
                input::edit(&mut input.text, key);
            }
        }
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
        if folder {
            self.message = Some(format!("created {rel}/"));
        } else {
            self.open_editor(path, None);
        }
    }

    fn edit_file(&mut self) {
        if self.editor.is_some() {
            return;
        }
        match self.selected_path() {
            Some(path) if path.is_file() => {
                let line = matches!(&self.viewer.source, Some(Source::File(p)) if *p == path).then(|| self.viewer.mark.unwrap_or(self.viewer.scroll) + 1);
                self.open_editor(path, line);
            }
            _ => self.message = Some("select a file to edit".into()),
        }
    }

    fn start_search(&mut self) {
        self.search = Some(Search::new(Mode::Files, self.ws().root.clone(), &self.tx));
    }

    pub fn on_search(&mut self, generation: usize, hits: Vec<crate::search::Hit>) {
        if let Some(search) = &mut self.search {
            search.on_results(generation, hits);
        }
    }

    fn search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(search) = &mut self.search else { return };
        match key.code {
            KeyCode::Esc => self.search = None,
            KeyCode::Enter => self.open_hit(),
            KeyCode::Tab | KeyCode::BackTab => search.toggle(&self.tx),
            KeyCode::Up => search.step(-1),
            KeyCode::Down => search.step(1),
            KeyCode::Char('k' | 'p') if ctrl => search.step(-1),
            KeyCode::Char('j' | 'n') if ctrl => search.step(1),
            KeyCode::PageUp => search.step(-10),
            KeyCode::PageDown => search.step(10),
            _ => {
                if input::edit(&mut search.query, key) {
                    search.update(&self.tx);
                }
            }
        }
    }

    fn open_hit(&mut self) {
        let Some(hit) = self.search.as_ref().and_then(Search::selected).cloned() else { return };
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        (self.search, self.editor) = (None, None);
        self.ws_mut().reveal(&hit.path);
        self.viewer.open_at(hit.path, hit.line);
        self.reading = true;
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

    fn open_repo_picker(&mut self) {
        self.open_repo_picker_for(None);
    }

    fn open_repo_picker_for(&mut self, issue: Option<Issue>) {
        self.pending_issue = issue;
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        let mut projects: Vec<PathBuf> = project::registered().into_iter().filter(|r| r.is_dir()).collect();
        projects.sort_by_key(|repo| *repo != self.ws().repo);
        let shown = projects.iter().map(|p| p.strip_prefix(&home).map_or(p.display().to_string(), |rel| format!("~/{}", rel.display()))).collect();
        self.repos = Some(Search::repos(home, shown, &self.tx));
    }

    pub fn on_repos(&mut self, repos: Vec<String>) {
        if let Some(search) = &mut self.repos {
            search.set_repos(repos, &self.tx);
        }
    }

    fn repos_key(&mut self, key: KeyEvent) {
        let Some(search) = &mut self.repos else { return };
        match key.code {
            KeyCode::Esc => self.repos = None,
            KeyCode::Enter => {
                let picked = search.selected().map(|h| h.rel.clone()).or_else(|| (!search.query.is_empty()).then(|| search.query.clone()));
                let typed = picked.as_ref() == Some(&search.query);
                if let Some(repo) = picked.and_then(|picked| self.add_repo(&picked, typed)) {
                    self.repos = None;
                    match self.pending_issue.take() {
                        Some(issue) => self.create_from_issue(repo, issue),
                        None => self.naming = Some(Naming { repo, name: String::new(), branch: String::new(), on_branch: false }),
                    }
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
    }

    fn review_key(&mut self, key: KeyEvent) {
        if self.editor.is_some() {
            return self.editor_key(key);
        }
        let Some(changes) = &mut self.changes else { return };
        // Uppercase letters come with shift too.
        let shift = key.modifiers.contains(KeyModifiers::SHIFT) && matches!(key.code, KeyCode::Up | KeyCode::Down);
        match key.code {
            KeyCode::Esc if self.reading && changes.anchor.is_some() => changes.anchor = None,
            KeyCode::Esc if self.reading => self.reading = false,
            KeyCode::Esc => self.close_review(),
            KeyCode::Tab | KeyCode::BackTab => self.reading = !self.reading,
            KeyCode::PageDown => changes.page(true),
            KeyCode::PageUp => changes.page(false),
            KeyCode::Char(c @ ('n' | 'p')) if self.reading => self.message = changes.next_change(c == 'n', false),
            KeyCode::Char('a') if self.reading => {
                self.message = changes.approve();
                if changes.left() == 0 {
                    self.finish_review();
                }
            }
            KeyCode::Char('c') if self.reading => self.comment_command('n'),
            KeyCode::Char('J') if self.reading => changes.move_cursor(1, true),
            KeyCode::Char('K') if self.reading => changes.move_cursor(-1, true),
            _ if self.reading => {
                if let Some(rows) = scroll_rows(key, changes.height) {
                    changes.move_cursor(rows, shift);
                }
            }
            _ => self.changes_tree_key(key),
        }
    }

    fn changes_tree_key(&mut self, key: KeyEvent) {
        let Some(changes) = &mut self.changes else { return };
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => changes.step(1),
            KeyCode::Up | KeyCode::Char('k') => changes.step(-1),
            KeyCode::Right | KeyCode::Char('l') => changes.fold(true),
            KeyCode::Left | KeyCode::Char('h') => changes.fold(false),
            KeyCode::Enter => match changes.selected().map(|r| (r.is_dir, r.expanded)) {
                Some((true, expanded)) => changes.fold(!expanded),
                Some((false, _)) => self.reading = true,
                None => {}
            },
            _ => {}
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        if matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.prefixed = false;
            // A click on the file is read first: keeping an emptied draft deletes a comment, which shifts the others.
            let on_file = mouse.kind == MouseEventKind::Down(MouseButton::Left) && self.areas.changes_file.contains(Position::new(mouse.column, mouse.row));
            if !on_file {
                self.keep_writing();
            }
        }
        if self.help {
            self.help = !matches!(mouse.kind, MouseEventKind::Down(_));
            return;
        }
        if self.repos.is_some() || self.picker.is_some() || self.search.is_some() {
            self.popup_mouse(mouse);
            return;
        }
        if self.issues.is_some() {
            return;
        }
        if self.file_input.is_some() {
            if matches!(mouse.kind, MouseEventKind::Down(_)) && !self.areas.popup.contains(Position::new(mouse.column, mouse.row)) {
                self.file_input = None;
            }
            return;
        }
        if self.naming.is_some() {
            if matches!(mouse.kind, MouseEventKind::Down(_)) && !self.areas.popup.contains(Position::new(mouse.column, mouse.row)) {
                self.naming = None;
            }
            return;
        }
        if self.asking.is_some() {
            if matches!(mouse.kind, MouseEventKind::Down(_))
                && let Some((_, confirm)) = self.asking.take()
                && self.areas.yes.contains(Position::new(mouse.column, mouse.row))
                && self.confirmed(confirm)
            {
                self.quit = true;
            }
            return;
        }
        let pos = Position::new(mouse.column, mouse.row);
        let areas = self.areas.clone();
        let down = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
        let wheel: i32 = match mouse.kind {
            MouseEventKind::ScrollUp if !shift => -3,
            MouseEventKind::ScrollDown if !shift => 3,
            _ => 0,
        };
        if let Some(selecting) = self.selecting.as_mut().filter(|s| s.dragging)
            && matches!(mouse.kind, MouseEventKind::Drag(MouseButton::Left) | MouseEventKind::Up(MouseButton::Left))
        {
            let Some(&(_, term)) = areas.panes.iter().find(|(id, _)| *id == selecting.id) else {
                self.selecting = None;
                return;
            };
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

        if down && areas.name.contains(pos) {
            self.open_picker();
        } else if down && areas.to_review.contains(pos) {
            self.open_review();
        } else if down && areas.waiting.contains(pos) {
            self.next_waiting();
        } else if let Some(&(row, _)) = areas.sidebar.iter().find(|(_, r)| r.contains(pos)) {
            if down {
                match row {
                    Some(i) if self.set_current(i) => self.set_focus(Focus::Terminal),
                    Some(_) => {}
                    None => self.open_repo_picker(),
                }
            }
        } else if let Some(&(id, area)) = areas.panes.iter().find(|(_, r)| r.contains(pos)) {
            self.mouse_in_terminal(mouse, id, area, pos, wheel);
        } else if areas.editor.contains(pos) {
            let Some(editor) = &mut self.editor else { return };
            if self.focus == Focus::Terminal {
                self.focus = Focus::Review;
            }
            let (r, column) = ((pos.y - areas.editor.y) as usize, (pos.x.saturating_sub(areas.editor.x)) as usize);
            let column = column.saturating_sub(editor.gutter());
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => editor.click(r, column, shift),
                MouseEventKind::Drag(MouseButton::Left) => editor.click(r, column, true),
                MouseEventKind::ScrollLeft => editor.scroll_sideways(-4),
                MouseEventKind::ScrollRight => editor.scroll_sideways(4),
                MouseEventKind::ScrollUp if shift => editor.scroll_sideways(-6),
                MouseEventKind::ScrollDown if shift => editor.scroll_sideways(6),
                _ if wheel != 0 => editor.scroll_by(wheel as isize),
                _ => {}
            }
        } else if areas.files_tree.contains(pos) && self.editor.is_none() {
            if down {
                self.reading = false;
                let w = self.ws_mut();
                let i = (pos.y - areas.files_tree.y) as usize + w.files_state.offset();
                if i < w.tree.rows.len() {
                    let again = w.files_state.selected() == Some(i);
                    w.files_state.select(Some(i));
                    self.preview();
                    match self.ws().selected_row().map(|r| (r.is_dir, r.expanded)) {
                        Some((true, expanded)) if again || double => self.fold_tree(!expanded),
                        Some((false, _)) => self.reading = double,
                        _ => {}
                    }
                }
            } else if wheel != 0 {
                self.tree_step(wheel.signum() as isize);
            }
        } else if areas.files_file.contains(pos) && self.editor.is_none() {
            match mouse.kind {
                MouseEventKind::ScrollLeft => self.viewer.scroll_sideways(-4),
                MouseEventKind::ScrollRight => self.viewer.scroll_sideways(4),
                MouseEventKind::ScrollUp if shift => self.viewer.scroll_sideways(-6),
                MouseEventKind::ScrollDown if shift => self.viewer.scroll_sideways(6),
                _ if wheel != 0 => self.viewer.scroll_by(wheel as isize),
                _ if down => {
                    self.reading = true;
                    let line = self.viewer.scroll + (pos.y - areas.files_file.y) as usize;
                    self.viewer.mark = Some(line.min(self.viewer.row_count().saturating_sub(1)));
                }
                _ => {}
            }
        } else if areas.changes_tree.contains(pos) {
            if down {
                (self.focus, self.reading) = (Focus::Review, false);
            }
            let Some(changes) = &mut self.changes else { return };
            if down {
                let i = (pos.y - areas.changes_tree.y) as usize + changes.state.offset();
                if i < changes.rows.len() {
                    let again = changes.state.selected() == Some(i);
                    changes.state.select(Some(i));
                    changes.follow();
                    match changes.selected().map(|r| (r.is_dir, r.expanded)) {
                        Some((true, expanded)) if again || double => changes.fold(!expanded),
                        Some((false, _)) => self.reading = double,
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
                (self.focus, self.reading) = (Focus::Review, true);
            }
            if let Some(changes) = &mut self.changes {
                let at = (pos.y - areas.changes_file.y) as usize;
                if wheel != 0 {
                    changes.scroll_by(wheel as isize);
                } else if down || mouse.kind == MouseEventKind::Drag(MouseButton::Left) {
                    match changes.shown.get(at) {
                        Some(Shown::Note(None, _) | Shown::Hint | Shown::Fold(_)) => {}
                        Some(Shown::Note(Some(i), _)) => {
                            if down {
                                let comment = changes.comments.get(*i).cloned();
                                self.keep_writing();
                                self.write_comment(comment);
                            }
                        }
                        _ => {
                            // Saved first: saving clears the selection a shift-click would extend.
                            if down {
                                self.keep_writing();
                            }
                            if let Some(changes) = &mut self.changes {
                                changes.click(at, shift || !down);
                            }
                        }
                    }
                }
            }
        } else if down && areas.review.contains(pos) {
            self.focus = Focus::Review;
        }
    }

    fn popup_mouse(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        let (popup, list) = (self.areas.popup, self.areas.popup_list);
        let down = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        if let Some(offset) = self.picker.as_ref().map(|p| p.state.offset()) {
            match mouse.kind {
                MouseEventKind::ScrollUp => self.picker_step(-1),
                MouseEventKind::ScrollDown => self.picker_step(1),
                _ if down && list.contains(pos) => {
                    let at = (pos.y - list.y) as usize + offset;
                    if at < self.picks().len()
                        && let Some(picker) = &mut self.picker
                    {
                        picker.state.select(Some(at));
                        self.pick();
                    }
                }
                _ if down && !popup.contains(pos) => self.picker = None,
                _ => {}
            }
            return;
        }
        let Some(list_of) = self.repos.as_mut().or(self.search.as_mut()) else { return };
        let delta = match mouse.kind {
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::ScrollDown => 1,
            _ if down && list.contains(pos) => (pos.y - list.y) as isize + list_of.state.offset() as isize - list_of.state.selected().unwrap_or(0) as isize,
            _ if down && !popup.contains(pos) => {
                (self.repos, self.search) = (None, None);
                return;
            }
            _ => return,
        };
        let double = down && self.last_click.is_some_and(|(at, when)| at == pos && when.elapsed() < DOUBLE_CLICK);
        if down {
            self.last_click = Some((pos, Instant::now()));
        }
        list_of.step(delta);
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        match (double, self.repos.is_some()) {
            (true, true) => self.repos_key(enter),
            (true, false) => self.search_key(enter),
            _ => {}
        }
    }

    fn mouse_in_terminal(&mut self, mouse: MouseEvent, id: usize, area: Rect, pos: Position, wheel: i32) {
        if mouse.kind == MouseEventKind::Down(MouseButton::Left) && self.active != Some(id) {
            self.select_term(id);
        }
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
        let line = parts.next().and_then(|n| n.parse::<usize>().ok());
        let path = if path.is_absolute() { path } else { cwd.join(path) };
        if !path.is_file() {
            self.message = Some(format!("no link or file at {word:?}"));
            return;
        }
        if self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        self.open_files();
        self.ws_mut().reveal(&path);
        self.open_editor(path, line);
    }

    fn naming_key(&mut self, key: KeyEvent) {
        let Some(naming) = &mut self.naming else { return };
        match key.code {
            KeyCode::Esc => self.naming = None,
            KeyCode::Tab | KeyCode::BackTab => naming.on_branch = !naming.on_branch,
            KeyCode::Enter => match typed_name(&naming.name) {
                _ if naming.name.trim().is_empty() && naming.branch.trim().is_empty() => {
                    let Some(naming) = self.naming.take() else { return };
                    self.open_project(&naming.repo);
                }
                name if name.is_empty() => self.message = Some("type the workspace's name".into()),
                name => {
                    let Some(naming) = self.naming.take() else { return };
                    let branch = Some(naming.branch.trim().to_string()).filter(|b| !b.is_empty());
                    self.create_workspace(naming.repo, name, branch);
                }
            },
            _ => {
                let text = if naming.on_branch { &mut naming.branch } else { &mut naming.name };
                input::edit(text, key);
            }
        }
    }

    fn open_issues(&mut self) {
        if self.no_projects {
            return self.open_picker();
        }
        let key = linear::api_key();
        let missing = "no Linear API key: put one in $LINEAR_API_KEY, or in the file linear in ~/.config/pulpo".to_string();
        let loaded = key.is_none().then_some(Err(missing));
        let from = self.ws().name.clone();
        self.issues = Some(Issues {
            query: String::new(),
            state: ListState::default().with_selected(Some(0)),
            loaded,
            everyone: false,
            reading: false,
            scroll: 0,
            details: HashMap::new(),
            fetching: HashSet::new(),
            resting: None,
            found: None,
            asked: None,
            typed_at: Instant::now(),
            from,
        });
        if let Some(key) = key {
            let tx = self.tx.clone();
            std::thread::spawn(move || {
                let _ = tx.send(Msg::Issues(linear::fetch(&key)));
            });
        }
    }

    pub fn on_issues(&mut self, loaded: Result<linear::Loaded, String>) {
        if let Some(view) = &mut self.issues {
            view.loaded = Some(loaded);
            view.show_own();
        }
    }

    pub fn on_found(&mut self, term: String, found: Result<Vec<Issue>, String>) {
        if let Some(view) = self.issues.as_mut().filter(|view| view.asked.as_deref() == Some(&term)) {
            let current = view.term() == term;
            view.found = Some((term, found));
            if current {
                view.state.select(Some(0));
            }
        }
    }

    /// Linear is asked once the typing has paused, not at every letter.
    fn fetch_search(&mut self) {
        let Some(view) = &mut self.issues else { return };
        if !view.search_due() || view.typed_at.elapsed() < SEARCH_AFTER {
            return;
        }
        let query = view.parsed();
        let term = query.key();
        view.asked = Some(term.clone());
        let Some(key) = linear::api_key() else { return };
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Found(term, linear::search(&key, &query)));
        });
    }

    pub fn on_issue_detail(&mut self, id: String, detail: Result<linear::Detail, String>) {
        if let Some(view) = &mut self.issues {
            view.fetching.remove(&id);
            view.details.insert(id, detail);
        }
    }

    fn read_issue(&mut self) {
        let Some(view) = &mut self.issues else { return };
        let Some(id) = view.selected().map(|i| i.id.clone()) else { return };
        view.reading = true;
        view.scroll = 0;
        if !matches!(view.details.get(&id), Some(Ok(_))) {
            self.fetch_detail(id);
        }
    }

    fn fetch_detail(&mut self, id: String) {
        let Some(key) = linear::api_key() else {
            if let Some(view) = &mut self.issues {
                view.details.insert(id, Err("no Linear API key".into()));
            }
            return;
        };
        if self.issues.as_mut().is_none_or(|view| !view.fetching.insert(id.clone())) {
            return;
        }
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(Msg::IssueDetail(id.clone(), linear::fetch_detail(&key, &id)));
        });
    }

    /// The selected issue is read once the selection has rested a moment, not on every row passed.
    fn fetch_resting_issue(&mut self) {
        let Some(view) = &mut self.issues else { return };
        let Some(id) = view.selected().map(|i| i.id.clone()) else { return };
        let now = Instant::now();
        let since = match &view.resting {
            Some((rested, at)) if *rested == id => *at,
            _ => {
                view.resting = Some((id.clone(), now));
                now
            }
        };
        if since.elapsed() >= DETAIL_AFTER && !view.details.contains_key(&id) {
            self.fetch_detail(id);
        }
    }

    fn start_from_issue(&mut self) {
        if let Some(issue) = self.issues.as_ref().and_then(|view| view.selected().cloned()) {
            self.issues = None;
            self.open_repo_picker_for(Some(issue));
        }
    }

    fn issues_key(&mut self, key: KeyEvent) {
        let Some(view) = &mut self.issues else { return };
        if view.reading {
            let page = 20;
            match key.code {
                KeyCode::Esc => (view.reading, view.scroll) = (false, 0),
                KeyCode::Up | KeyCode::Char('k') => view.scroll = view.scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => view.scroll += 1,
                KeyCode::PageUp | KeyCode::Char('b') => view.scroll = view.scroll.saturating_sub(page),
                KeyCode::PageDown | KeyCode::Char(' ') => view.scroll += page,
                KeyCode::Char('g') => view.scroll = 0,
                KeyCode::Char('o') => {
                    if let Some(url) = view.selected().map(|i| i.url.clone()).filter(|u| u.starts_with("https://")) {
                        let _ = std::process::Command::new("open").arg(url).status();
                    }
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.issues = None,
            KeyCode::Enter => self.read_issue(),
            KeyCode::Up => view.step(-1),
            KeyCode::Down => view.step(1),
            KeyCode::PageUp => view.step(-10),
            KeyCode::PageDown => view.step(10),
            _ => {
                if input::edit(&mut view.query, key) {
                    view.state.select(Some(0));
                    view.typed_at = Instant::now();
                }
            }
        }
    }

    fn create_from_issue(&mut self, repo: PathBuf, issue: Issue) {
        let Some(name) = issue.workspace_name() else {
            self.message = Some(format!("{} isn't an issue number Pulpo can name a workspace after", issue.id));
            return;
        };
        self.starts.insert(worktree::root_of(&repo, &name), issue.id);
        self.create_workspace(repo, name, Some(issue.branch).filter(|b| !b.is_empty()));
    }

    fn open_project(&mut self, repo: &Path) {
        project::register(repo);
        self.load_workspaces();
        if let Some(i) = self.workspaces.iter().position(|w| w.root == repo)
            && self.set_current(i)
        {
            self.set_focus(Focus::Terminal);
        }
    }

    fn create_workspace(&mut self, repo: PathBuf, name: String, branch: Option<String>) {
        let tx = self.tx.clone();
        // Asked twice: the first copy is still being made.
        if !self.copying.insert((repo.clone(), name.clone())) {
            self.message = Some(format!("workspace {name} is being made already"));
            return;
        }
        if !project::registered().contains(&repo) {
            self.new_projects.insert(repo.clone());
        }
        project::register(&repo);
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
        let new_project = self.new_projects.remove(&copy.0);
        let (root, mut notes) = match made {
            Ok(made) => made,
            Err(e) => {
                self.starts.remove(&worktree::root_of(&copy.0, &copy.1));
                if new_project {
                    project::unregister(&copy.0);
                }
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
                self.starts.remove(&root);
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
            Err(e) => {
                self.starts.remove(&root);
                self.message = Some(format!("could not set up workspace {}: {e}", wt.name));
            }
        }
    }

    pub fn on_set_up(&mut self, root: PathBuf, log: PathBuf, ok: bool) {
        self.setting_up.remove(&root);
        let start = self.starts.remove(&root);
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
        let agent = start.filter(|_| ok).map(|id| vec!["-l".into(), "-c".into(), linear::start_command(&shell(), &id)]);
        let token = self.spawn_in(root, &shell(), &agent.unwrap_or_default(), false);
        if ok {
            self.handing_over.insert(token);
        }
    }

    fn remove_workspace(&mut self, root: &Path) {
        let Some(wt) = self.worktrees.iter().find(|s| s.root == root).cloned() else { return };
        self.remove(wt, true);
    }

    fn hide_project(&mut self, root: &Path) {
        // Its workspaces are listed only while it is a project.
        if !self.workspaces.iter().any(|w| w.repo == root && w.is_worktree()) {
            project::unregister(root);
        }
        if self.ws().root == root
            && let Some(next) = self.picks().into_iter().find_map(|p| match p {
                Pick::Go(i) if self.workspaces[i].root != root => Some(i),
                _ => None,
            })
        {
            self.set_current(next);
        }
        self.load_workspaces();
        let ids: Vec<usize> = self.terms.iter().filter(|t| t.info.cwd == root).map(|t| t.id).collect();
        for id in ids {
            self.close_terminal(id);
        }
        self.message = Some(format!("project {} hidden; its folder stays", root.file_name().unwrap_or_default().to_string_lossy()));
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
        if asked && self.ws().root == wt.root {
            let main = self.workspaces.iter().position(|w| w.root == wt.repo);
            if let Some(next) = self.workspace_order().into_iter().find(|&i| self.workspaces[i].root != wt.root).or(main) {
                self.set_current(next);
            }
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
            self.drafts.remove(&root);
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

fn arrow(key: Key) -> Option<Dir> {
    if !key.mods.is_empty() {
        return None;
    }
    match key.code {
        KeyCode::Left => Some(Dir::Left),
        KeyCode::Right => Some(Dir::Right),
        KeyCode::Up => Some(Dir::Up),
        KeyCode::Down => Some(Dir::Down),
        _ => None,
    }
}

fn workspace_step(key: Key) -> Option<isize> {
    match (key.code, key.mods) {
        (KeyCode::Up, KeyModifiers::SHIFT) => Some(-1),
        (KeyCode::Down, KeyModifiers::SHIFT) => Some(1),
        _ => None,
    }
}

fn scroll_rows(key: KeyEvent, height: usize) -> Option<isize> {
    let half = (height / 2).max(1) as isize;
    Some(match key.code {
        KeyCode::Down | KeyCode::Char('j') => 1,
        KeyCode::Up | KeyCode::Char('k') => -1,
        KeyCode::PageDown | KeyCode::Char('d' | ' ') => half,
        KeyCode::PageUp | KeyCode::Char('u') => -half,
        KeyCode::Home | KeyCode::Char('g') => isize::MIN / 2,
        KeyCode::End | KeyCode::Char('G') => isize::MAX / 2,
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

pub fn typed_name(text: &str) -> String {
    let dashed = text.trim().to_lowercase().replace(char::is_whitespace, "-");
    dashed.chars().filter(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_".contains(*c)).collect()
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
    use super::{Issues, rise, typed_name, workspace_step};
    use crate::linear::{self, Issue};
    use ratatui::widgets::ListState;
    use std::collections::{HashMap, HashSet};
    use std::time::Instant;
    use crate::keymap::Key;

    #[test]
    fn names_a_workspace_as_typed() {
        assert_eq!(typed_name("  Fix Login page! "), "fix-login-page");
        assert_eq!(typed_name("FND-12_api"), "fnd-12_api");
        assert_eq!(typed_name("é/../"), "");
    }

    #[test]
    fn steps_workspaces_with_shifted_up_and_down_only() {
        let key = |text: &str| Key::parse(text).unwrap();
        assert_eq!(workspace_step(key("shift-up")), Some(-1));
        assert_eq!(workspace_step(key("shift-down")), Some(1));
        assert_eq!(workspace_step(key("up")), None);
        assert_eq!(workspace_step(key("ctrl-down")), None);
    }

    #[test]
    fn shows_the_issue_a_workspace_is_named_after_whoever_has_it() {
        let issue = |id: &str, assignee: &str| Issue {
            id: id.into(),
            title: String::new(),
            url: String::new(),
            branch: String::new(),
            project: String::new(),
            assignee: assignee.into(),
            state: String::new(),
        };
        let loaded = linear::Loaded::new("Sam".into(), vec![issue("APP-1", "Sam"), issue("WEB-68", "Maria"), issue("WEB-70", "Sam")]);
        let mut view = Issues {
            query: String::new(),
            state: ListState::default().with_selected(Some(0)),
            loaded: Some(Ok(loaded)),
            everyone: false,
            reading: false,
            scroll: 0,
            details: HashMap::new(),
            fetching: HashSet::new(),
            resting: None,
            found: None,
            asked: None,
            typed_at: Instant::now(),
            from: "web-68".into(),
        };
        view.show_own();
        assert_eq!((view.everyone, view.selected().map(|i| i.id.as_str())), (true, Some("WEB-68")));
        view.from = "app-1".into();
        view.everyone = false;
        view.show_own();
        assert_eq!((view.everyone, view.selected().map(|i| i.id.as_str())), (false, Some("APP-1")));
    }

    #[test]
    fn shows_what_linear_found_by_description_and_says_when_it_is_still_asking() {
        let issue = |id: &str, project: &str| Issue {
            id: id.into(),
            title: "nothing like the word".into(),
            url: String::new(),
            branch: String::new(),
            project: project.into(),
            assignee: String::new(),
            state: String::new(),
        };
        let mut view = Issues {
            query: "gym".into(),
            state: ListState::default().with_selected(Some(0)),
            loaded: Some(Ok(linear::Loaded::new("Sam".into(), vec![issue("APP-1", "Web App")]))),
            everyone: false,
            reading: false,
            scroll: 0,
            details: HashMap::new(),
            fetching: HashSet::new(),
            resting: None,
            found: Some(("gym".into(), Ok(vec![issue("APP-7", "Web App"), issue("APP-8", "Mobile")]))),
            asked: Some("gym".into()),
            typed_at: Instant::now(),
            from: String::new(),
        };
        let ids = |view: &Issues| view.visible().iter().map(|i| i.id.clone()).collect::<Vec<_>>();
        assert_eq!((ids(&view), view.search_note()), (vec!["APP-7".to_string(), "APP-8".to_string()], String::new()));
        view.query = "gym x".into();
        assert_eq!(view.search_note(), "searching Linear…");
        assert!(ids(&view).is_empty(), "the old answer is not shown for another term");
    }

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
