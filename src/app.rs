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
use crate::proto::{PROTOCOL, ToClient, ToServer};
use crate::review::Changes;
use crate::search::{Mode, Search};
use crate::session;
use crate::status::Status;
use crate::term::Term;
use crate::viewer::{Source, Viewer};
use crate::workspace::{self, Workspace};

/// At most one desktop notification per terminal this often.
const NOTICE_EVERY: Duration = Duration::from_secs(10);
/// How long a message stays on the bottom line, unless it asks something.
const MESSAGE_FOR: Duration = Duration::from_secs(5);
/// Said when unsaved changes in the editor keep you where you are.
const UNSAVED: &str = "unsaved changes in the editor: ⌘s saves them, esc twice throws them away";
/// How long the prefix waits for its key.
const PREFIX_FOR: Duration = Duration::from_secs(3);
/// Two clicks on the same row this close together open it.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// Where the keys go. The prefix then w s t f c moves it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Focus {
    Workspaces,
    Sessions,
    Terminal,
    Files,
    Changes,
}

/// A name typed on the bottom line.
pub enum Ask {
    /// A new task session of the repository.
    Session,
    /// A new file or folder (ending with /) in this folder.
    File(PathBuf),
    /// A new name for this file or folder.
    Rename(PathBuf),
}

pub struct Prompt {
    pub ask: Ask,
    pub text: String,
}

/// What y on the bottom line does.
pub enum Confirm {
    RemoveWorkspace(PathBuf),
    DeleteSession(PathBuf),
    CloseTerminal(usize),
    DeleteFile(PathBuf),
    StopAll,
}

/// Where each part was drawn, for the mouse.
#[derive(Default, Clone)]
pub struct Areas {
    pub counts: Rect,
    /// The rows of the two lists.
    pub workspaces: Rect,
    pub sessions: Rect,
    /// Each tab of the session's terminals, and the + after them.
    pub tabs: Vec<(Option<usize>, Rect)>,
    /// The terminal on screen, inside its frame.
    pub term: Rect,
    pub files_tree: Rect,
    pub files_file: Rect,
    pub changes_tree: Rect,
    pub changes_file: Rect,
    /// The marks down the right of the diff: a click goes there.
    pub changes_ruler: Rect,
    /// The editor's text, without its line numbers' gutter accounted.
    pub editor: Rect,
    /// The borders to drag: the sidebar's, the right side's, and the line
    /// between a tree and its file (with where the tree starts).
    pub left_divider: Rect,
    pub divider: Rect,
    pub tree_dividers: Vec<(Rect, u16)>,
    /// Yes on the bottom line, while it asks.
    pub yes: Rect,
    /// The search popup or the repository picker, and its list.
    pub popup: Rect,
    pub popup_list: Rect,
    /// The whole screen's width, to turn a drag into the right side's.
    pub width: u16,
}

/// A border being dragged.
#[derive(Clone, Copy)]
enum Drag {
    Sidebar,
    Right,
    /// Between a tree and its file; the tree starts at this column.
    Tree(u16),
}

/// Text selected in a terminal with the mouse: its id, the cells (row,
/// column) where the drag started and is now, and whether it still goes on.
pub struct Selecting {
    pub id: usize,
    pub from: (u16, u16),
    pub to: (u16, u16),
    pub dragging: bool,
}

pub struct App {
    pub workspaces: Vec<Workspace>,
    /// The task sessions among them: copies of a main checkout with their
    /// own Docker project and ports.
    pub isolated: Vec<session::Session>,
    pub current: usize,
    pub workspaces_state: ListState,
    pub sessions_state: ListState,
    /// Shown when no repository is registered yet.
    scratch: PathBuf,
    pub focus: Focus,
    pub agents: Agents,
    /// Every terminal, of every session, in the order of their tabs; each
    /// belongs to the checkout it was started in.
    pub terms: Vec<Term>,
    /// The terminal on screen in the current session.
    pub active: Option<usize>,
    /// The prefix was pressed: the next key is Pulpo's, if it comes soon.
    pub prefixed: bool,
    prefixed_at: Option<Instant>,
    /// A click said yes to quitting (Q): the loop ends.
    pub quit: bool,
    /// Every shortcut, in a popup (prefix ?).
    pub help: bool,
    /// The sidebar or the right side hidden (prefix < and >).
    pub left_hidden: bool,
    pub right_hidden: bool,
    pub prompt: Option<Prompt>,
    /// A question on the bottom line, and what y does.
    pub asking: Option<(String, Confirm)>,
    pub message: Option<String>,
    message_since: Option<(String, Instant)>,
    /// Searching the session's files (names, then lines), in the Files panel.
    pub search: Option<Search>,
    /// Picking a repository to add as a workspace.
    pub repos: Option<Search>,
    /// The file selected in Files, and the editor in its place.
    pub viewer: Viewer,
    pub editor: Option<Editor>,
    /// Esc was pressed once in the editor with unsaved changes.
    discard: bool,
    /// The file takes the whole right side while you're in that panel.
    pub files_expanded: bool,
    pub changes_expanded: bool,
    /// In a panel opened wide, the keys go to its file instead of its tree
    /// (⏎ on a file, or tab).
    pub reading: bool,
    /// What the current session changed.
    pub changes: Option<Changes>,
    /// Columns of the sidebar, the right side and the trees in Files and
    /// Changes, once their border was dragged.
    pub left_width: Option<u16>,
    pub right_width: Option<u16>,
    pub tree_width: Option<u16>,
    dragging: Option<Drag>,
    pub selecting: Option<Selecting>,
    /// The last click, to tell a double click.
    last_click: Option<(Position, Instant)>,
    pub keymap: Keymap,
    pub areas: Areas,
    /// Sessions being set up or removed in the background, and those whose
    /// setup failed (with its log).
    /// The setups by session, with their process (its own group).
    pub setting_up: HashMap<PathBuf, u32>,
    pub setup_failed: HashMap<PathBuf, PathBuf>,
    /// Sessions being copied: their repository and name.
    copying: HashSet<(PathBuf, String)>,
    removing: HashSet<PathBuf>,
    /// The terminal (its tag) last on screen in each session, and the
    /// session last used in each repository.
    last_term: HashMap<PathBuf, String>,
    last_session: HashMap<PathBuf, PathBuf>,
    /// Where you were, as last written to disk (`layout.rs`); written again
    /// when it changes, once the server has shown what it has.
    saved: Layout,
    ready: bool,
    /// Terminals being brought back after a restart: token → old tag.
    reopening: HashMap<u64, String>,
    link: Link,
    /// Terminals asked of the server and not shown yet, and whether to go
    /// into them. Its token stands in for its id until then.
    spawning: HashMap<u64, bool>,
    next_token: u64,
    upgrade_asked: bool,
    /// Connected again after the server went away (an upgrade, or it
    /// stopped), with this many terminals before; and when.
    reconnected: Option<usize>,
    last_reconnect: Option<Instant>,
    /// The server stopped twice in a row, or couldn't start: tried again
    /// when you go to the terminal.
    offline: bool,
    /// Rows and columns available to terminals, set when drawing.
    term_size: (u16, u16),
    /// Workspaces with file changes not yet refreshed, and whether the
    /// worktree list itself may have changed.
    fs_dirty: HashSet<PathBuf>,
    reload_list: bool,
    watcher: RecommendedWatcher,
    watched: HashSet<PathBuf>,
    tx: Sender<Msg>,
}

impl App {
    /// `launch` is the checkout Pulpo was started in, if any; `scratch` the
    /// directory to show when no repository is registered.
    pub fn new(launch: Option<PathBuf>, scratch: PathBuf, tx: Sender<Msg>, link: Link) -> anyhow::Result<Self> {
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
        let theme_error = crate::theme::apply(&crate::theme::chosen()).err().map(|e| format!("theme: {e}; using Linear Dark"));
        let mut app = App {
            workspaces: Vec::new(),
            isolated: Vec::new(),
            current: 0,
            workspaces_state: ListState::default().with_selected(Some(0)),
            sessions_state: ListState::default().with_selected(Some(0)),
            scratch,
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
            message: keys_error.or(theme_error).or_else(|| {
                (!crate::agents::hooks_installed())
                    .then(|| "run `pulpo hooks install` to see what Claude runs and asks in each terminal".into())
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
            left_width: None,
            right_width: None,
            tree_width: None,
            dragging: None,
            selecting: None,
            last_click: None,
            keymap,
            areas: Areas::default(),
            setting_up: HashMap::new(),
            setup_failed: HashMap::new(),
            copying: HashSet::new(),
            removing: HashSet::new(),
            last_term: layout.terminals.clone().into_iter().collect(),
            last_session: layout.sessions.clone().into_iter().collect(),
            saved: layout.clone(),
            ready: false,
            reopening: HashMap::new(),
            link,
            spawning: HashMap::new(),
            // Far above any terminal id, so the two never mix.
            next_token: 1 << 40,
            upgrade_asked: false,
            reconnected: None,
            last_reconnect: None,
            offline: false,
            term_size: (24, 80),
            fs_dirty: HashSet::new(),
            reload_list: false,
            watcher,
            watched: HashSet::new(),
            tx,
        };
        app.load_workspaces();
        // Where you were: the session last used in the repository Pulpo was
        // opened in (or in a session's own folder, that one); opened
        // elsewhere, the session on screen last time.
        let at = |root: &Path| app.workspaces.iter().position(|w| w.root == root);
        let start = match &launch {
            Some(dir) => at(dir).map(|i| match &app.workspaces[i] {
                w if !w.is_worktree() => app.last_session.get(&w.repo).and_then(|s| at(s)).unwrap_or(i),
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

    // Workspaces and sessions

    /// Rebuild the list from the registry and git, keeping existing state.
    fn load_workspaces(&mut self) {
        let (current, repo) = self.workspaces.get(self.current).map(|w| (w.root.clone(), w.repo.clone())).unzip();
        let mut old = std::mem::take(&mut self.workspaces);
        let mut reuse = |root: PathBuf, repo: &Path| match old.iter().position(|w| w.root == root) {
            Some(i) => old.swap_remove(i),
            None => Workspace::new(root, repo.to_path_buf()),
        };
        let isolated = session::all();
        self.isolated.clear();
        for repo in workspace::registered().into_iter().filter(|r| r.is_dir()) {
            let mut roots = git::worktrees(&repo);
            if roots.is_empty() {
                roots.push(repo.clone());
            }
            for root in roots {
                self.workspaces.push(reuse(root, &repo));
            }
            for s in isolated.iter().filter(|s| s.repo == repo && s.root.is_dir() && !self.removing.contains(&s.root)) {
                self.workspaces.push(reuse(s.root.clone(), &repo));
                self.isolated.push(s.clone());
            }
        }
        if self.workspaces.is_empty() {
            let scratch = self.scratch.clone();
            self.workspaces.push(reuse(scratch.clone(), &scratch));
        }
        match current.as_ref().map(|c| self.workspaces.iter().position(|w| &w.root == c)) {
            None => self.current = self.current.min(self.workspaces.len() - 1),
            Some(Some(i)) => self.current = i,
            // Gone: its repository's main checkout, or the first, as if picked.
            Some(None) => {
                let i = repo.and_then(|r| self.workspaces.iter().position(|w| w.repo == r)).unwrap_or(0);
                self.current = usize::MAX;
                self.set_current(i);
            }
        }
        self.sync_lists();
        self.sync_watches();
    }

    fn sync_watches(&mut self) {
        let wanted: HashSet<PathBuf> = self.workspaces.iter().map(|w| w.root.clone()).collect();
        for path in self.watched.difference(&wanted) {
            let _ = self.watcher.unwatch(path);
        }
        for path in wanted.difference(&self.watched) {
            let _ = self.watcher.watch(path, RecursiveMode::Recursive);
        }
        self.watched = wanted;
    }

    /// Make session `i` current: its terminals, on the one you last had on
    /// screen there, its files and its changes. Not with unsaved changes in
    /// the editor.
    /// False when it stays where it was (unsaved changes).
    fn set_current(&mut self, i: usize) -> bool {
        if i >= self.workspaces.len() {
            return false;
        }
        if i != self.current {
            // The session on screen is still there to keep its editor.
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
            let base = self.isolated.iter().find(|s| s.root == self.workspaces[i].root).and_then(|s| s.base.clone());
            self.changes = Some(Changes::new(&self.workspaces[i].root, base));
            self.reload_changes();
            self.restore_term();
            self.preview();
            if let Some(log) = self.setup_failed.get(&self.workspaces[i].root) {
                self.message = Some(format!("this session's setup stopped: see {}", log.display()));
            }
        }
        let ws = &self.workspaces[i];
        self.last_session.insert(ws.repo.clone(), ws.root.clone());
        self.sync_lists();
        true
    }

    /// The editor has changes not saved yet.
    fn editing(&self) -> bool {
        self.editor.as_ref().is_some_and(|e| e.dirty)
    }

    /// The repositories: the first checkout of each (its main one), in order.
    pub fn repos(&self) -> Vec<usize> {
        let mut out: Vec<usize> = Vec::new();
        for (i, w) in self.workspaces.iter().enumerate() {
            if !out.iter().any(|&o| self.workspaces[o].repo == w.repo) {
                out.push(i);
            }
        }
        out
    }

    /// The sessions of the current repository, its main checkout first.
    pub fn sessions(&self) -> Vec<usize> {
        let repo = &self.ws().repo;
        (0..self.workspaces.len()).filter(|&i| &self.workspaces[i].repo == repo).collect()
    }

    /// The session's name: `main` for the repository itself.
    pub fn session_name(&self, i: usize) -> &str {
        let ws = &self.workspaces[i];
        if ws.is_worktree() { &ws.name } else { "main" }
    }

    /// Where a terminal works, for messages: `backbone`, `backbone/fix-login`.
    pub fn place(&self, dir: &Path) -> Option<String> {
        let ws = self.workspaces.iter().find(|w| w.root == dir)?;
        let repo = self.workspaces.iter().find(|w| w.repo == ws.repo && !w.is_worktree()).map_or(&ws.name, |r| &r.name);
        Some(if ws.is_worktree() { format!("{repo}/{}", ws.name) } else { repo.clone() })
    }

    /// The lists show the current repository and session.
    fn sync_lists(&mut self) {
        let repo = &self.ws().repo;
        let in_repos = self.repos().iter().position(|&i| &self.workspaces[i].repo == repo);
        let in_sessions = self.sessions().iter().position(|&i| i == self.current);
        self.workspaces_state.select(in_repos);
        self.sessions_state.select(in_sessions);
    }

    pub fn on_fs(&mut self, paths: &[PathBuf]) {
        for path in paths {
            for w in &self.workspaces {
                if w.is_affected(path) {
                    self.fs_dirty.insert(w.root.clone());
                }
            }
            // A worktree was added or removed (by an agent or with git).
            let added_or_removed = self.workspaces.iter().any(|w| {
                path.strip_prefix(w.repo.join(".git/worktrees")).is_ok_and(|rel| rel.components().count() == 1)
            });
            self.reload_list |= added_or_removed;
        }
    }

    pub fn fs_pending(&self) -> bool {
        !self.fs_dirty.is_empty() || self.reload_list
    }

    pub fn flush_fs(&mut self) {
        if std::mem::take(&mut self.reload_list) {
            self.load_workspaces();
        }
        let dirty = std::mem::take(&mut self.fs_dirty);
        for w in &mut self.workspaces {
            if dirty.contains(&w.root) {
                w.refresh();
            }
        }
        if dirty.contains(&self.ws().root) {
            let root = self.ws().root.clone();
            self.viewer.reload(&root);
            // Changes follow what the agents do.
            self.reload_changes();
            if let Some(search) = &mut self.search {
                search.rescan(&self.tx);
            }
            // An agent changed the file being edited: take it, unless you
            // changed it too.
            if let Some(editor) = &mut self.editor
                && editor.reload_if_changed()
            {
                self.message = Some(format!("{} changed on disk: reloaded", editor.path.display()));
            }
        }
    }

    /// Read what the session changed again, in the background (`on_changes`).
    fn reload_changes(&mut self) {
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

    fn add_repo(&mut self, input: &str) {
        let path = match input.strip_prefix("~/") {
            Some(rest) => PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(rest),
            None => PathBuf::from(input),
        };
        match git::toplevel(&path).and_then(|top| git::main_repo(&top)) {
            Some(repo) => {
                workspace::register(&repo);
                self.load_workspaces();
                if let Some(i) = self.workspaces.iter().position(|w| w.root == repo) {
                    self.set_current(i);
                }
            }
            None => self.message = Some(format!("not a git repository: {}", path.display())),
        }
    }

    // Terminals

    /// Start `program` in a new terminal working in `cwd`; its token. The
    /// server shows it shortly; until then the token stands in for its id.
    fn spawn_in(&mut self, cwd: PathBuf, program: &str, args: &[String], focus: bool) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        let (rows, cols) = self.term_size;
        // A task session's terminals get its Compose project and ports.
        let env = self.isolated.iter().find(|s| s.root == cwd).map(session::env).unwrap_or_default();
        self.link.send(&ToServer::Spawn { token, program: program.into(), args: args.to_vec(), cwd, rows, cols, env });
        self.spawning.insert(token, focus);
        token
    }

    /// A shell in a new tab of the current session.
    fn new_terminal(&mut self, focus: bool) {
        let cwd = self.ws().root.clone();
        self.spawn_in(cwd, &shell(), &[], focus);
    }

    /// The server came back with no terminals (a reboot, or it stopped):
    /// open the ones you had again, each in its session, resuming the
    /// Claude session that was alive in it.
    fn reopen(&mut self) {
        let open = std::mem::take(&mut self.saved.open);
        let (mut terminals, mut agents) = (0, 0);
        for was in open.iter().filter(|o| o.cwd.is_dir()) {
            let args = layout::reopen(was, &shell());
            agents += usize::from(!args.is_empty());
            let token = self.spawn_in(was.cwd.clone(), &shell(), &args, false);
            self.reopening.insert(token, was.tag.clone());
            terminals += 1;
        }
        if terminals > 0 {
            let agents = if agents > 0 { format!(", resuming {agents} Claude session(s)") } else { String::new() };
            self.message = Some(format!("reopened {terminals} terminal(s) from before the restart{agents}"));
        }
    }

    /// Where you are now, to write down.
    fn layout(&self) -> Layout {
        let open = self
            .terms
            .iter()
            .map(|t| layout::Open { cwd: t.info.cwd.clone(), tag: t.info.tag.clone(), claude: self.agents.session_for_tab(&t.info.tag).map(String::from) })
            .collect();
        Layout {
            current: Some(self.ws().root.clone()),
            sessions: self.last_session.clone().into_iter().collect(),
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
                self.terms.push(Term::new(info, (rows, cols), self.link.clone()));
                let i = self.terms.len() - 1;
                self.terms[i].resize(self.term_size.0, self.term_size.1);
                let session = self.session_of(&self.terms[i]);
                // Brought back after a restart: it stands for the old one.
                if let Some(old) = token.and_then(|t| self.reopening.remove(&t)) {
                    let new = self.terms[i].info.tag.clone();
                    for tag in self.last_term.values_mut().filter(|tag| **tag == old) {
                        *tag = new.clone();
                    }
                    self.spawning.remove(&token.unwrap_or_default());
                    if self.reopening.is_empty() || session.is_none_or(|s| s == self.current) {
                        self.restore_term();
                    }
                    return;
                }
                let Some(focus) = token.and_then(|t| self.spawning.remove(&t)) else {
                    // One that kept running is shown once all are in (`Ready`).
                    if self.ready && self.active.is_none() && session.is_none_or(|s| s == self.current) {
                        self.restore_term();
                    }
                    return;
                };
                // Ours: show it, in its session (you may have moved on since).
                // Refused (unsaved changes): it waits in its own session.
                if focus && let Some(session) = session
                    && !self.set_current(session)
                {
                    return;
                }
                if focus || (self.active.is_none() && session.is_none_or(|s| s == self.current)) {
                    self.select_term(id);
                }
                if focus {
                    self.set_focus(Focus::Terminal);
                }
            }
            ToClient::Hello { protocol, exe, built, .. } => self.check_server_build(protocol, &exe, built),
            // Everything the server had is shown: start with a shell if it had none.
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
                if self.session_terms().is_empty() && self.spawning.is_empty() {
                    self.new_terminal(false);
                }
                self.ready = true;
                self.restore_term();
            }
            ToClient::Info { info } => {
                if let Some(term) = self.terms.iter_mut().find(|t| t.id == info.id as usize) {
                    term.set_info(info);
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

    pub fn on_output(&mut self, id: u32, bytes: &[u8]) {
        if let Some(term) = self.terms.iter_mut().find(|t| t.id == id as usize) {
            term.feed(bytes);
        }
    }

    /// A server runs an older build of this Pulpo: have it become this one.
    /// It keeps its terminals; the connection drops and comes back.
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

    /// The connection to the server dropped: it upgraded, or it stopped and
    /// the terminals with it. Connect again; what comes back tells which.
    pub fn on_server_gone(&mut self) {
        let had = self.terms.len();
        self.ready = false;
        self.terms.clear();
        self.spawning.clear();
        self.active = None;
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

    /// If the server was given up on, try it again. Not on every key:
    /// starting it can take seconds.
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

    /// The size new terminals start at: the room in the middle.
    pub fn set_term_size(&mut self, rows: u16, cols: u16) {
        self.term_size = (rows.max(1), cols.max(1));
    }

    /// Working while the program prints on its own; then, until you look at
    /// it, waiting with something to read. A program that reports its
    /// status (OSC 7501) is taken at its word.
    pub fn term_state(&self, term: &Term) -> State {
        match term.status().map(|(status, _)| status) {
            Some(Status::Working) => State::Running,
            Some(Status::Blocked) => State::Waiting,
            None if term.working() => State::Running,
            _ if term.unread() => State::Waiting,
            _ => State::Idle,
        }
    }

    /// Before each draw: runs that ended and agents that stopped or asked
    /// leave something to read, unless that terminal is on screen.
    pub fn tick(&mut self) {
        if self.prefixed_at.is_some_and(|at| at.elapsed() >= PREFIX_FOR) {
            self.prefixed = false;
            self.prefixed_at = None;
        }
        // Messages go after a while; a question stays until answered, and
        // the server being down until it's back.
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

    /// Something is working, so its spinner needs frequent redraws.
    pub fn animating(&self) -> bool {
        !self.setting_up.is_empty() || self.terms.iter().any(|term| self.term_state(term) == State::Running)
    }

    /// The session a terminal belongs to: the checkout it started in.
    fn session_of(&self, term: &Term) -> Option<usize> {
        self.workspaces.iter().position(|w| w.root == term.info.cwd)
    }

    /// The current session's terminals, in tab order, as indexes into
    /// `terms`. One started outside every checkout shows in all of them, so
    /// it stays reachable.
    pub fn session_terms(&self) -> Vec<usize> {
        (0..self.terms.len()).filter(|&i| self.session_of(&self.terms[i]).is_none_or(|s| s == self.current)).collect()
    }

    /// How a session (`i`) is doing, from its terminals: waiting beats
    /// working beats quiet. None without terminals.
    pub fn session_state(&self, i: usize) -> Option<State> {
        let root = &self.workspaces[i].root;
        let states = self.terms.iter().filter(|t| &t.info.cwd == root).map(|t| self.term_state(t));
        states.max_by_key(|state| state.urgency())
    }

    pub fn term(&self) -> Option<&Term> {
        self.active.and_then(|id| self.terms.iter().find(|t| t.id == id))
    }

    fn term_mut(&mut self) -> Option<&mut Term> {
        self.active.and_then(|id| self.terms.iter_mut().find(|t| t.id == id))
    }

    /// Make terminal `id` the tab on screen.
    fn select_term(&mut self, id: usize) {
        self.active = Some(id);
        self.selecting = None;
        if let Some(term) = self.terms.iter().find(|t| t.id == id) {
            self.last_term.insert(self.ws().root.clone(), term.info.tag.clone());
        }
    }

    /// Show the terminal this session last had on screen, or its first.
    fn restore_term(&mut self) {
        let root = self.ws().root.clone();
        let terms: Vec<&Term> = self.session_terms().into_iter().map(|i| &self.terms[i]).collect();
        let last = self.last_term.get(&root).and_then(|tag| terms.iter().find(|t| &t.info.tag == tag));
        self.active = last.or(terms.first()).map(|t| t.id);
    }

    /// Take a terminal off the list; the tab after it (or before) shows.
    fn remove_term(&mut self, id: usize) {
        let tabs: Vec<usize> = self.session_terms().into_iter().map(|i| self.terms[i].id).collect();
        self.terms.retain(|t| t.id != id);
        if self.active == Some(id) {
            let at = tabs.iter().position(|&t| t == id).unwrap_or(0);
            let next = tabs.get(at + 1).or(at.checked_sub(1).and_then(|p| tabs.get(p))).copied();
            self.active = None;
            if let Some(next) = next {
                self.select_term(next);
            }
        }
    }

    /// The next or previous tab, wrapping around, and into it.
    fn cycle_tab(&mut self, dir: isize) {
        let tabs: Vec<usize> = self.session_terms().into_iter().map(|i| self.terms[i].id).collect();
        if tabs.is_empty() {
            return;
        }
        let at = self.active.and_then(|id| tabs.iter().position(|&t| t == id)).unwrap_or(0);
        let next = tabs[(at as isize + dir).rem_euclid(tabs.len() as isize) as usize];
        self.select_term(next);
        self.set_focus(Focus::Terminal);
    }

    /// Into the terminal that has waited longest with something to read:
    /// those that ask before those that finished.
    fn next_waiting(&mut self) {
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
        if let Some(i) = self.terms.iter().find(|t| t.id == id).and_then(|t| self.session_of(t))
            && !self.set_current(i)
        {
            return;
        }
        self.select_term(id);
        self.set_focus(Focus::Terminal);
    }

    /// Close a terminal: the program in front gets Ctrl-C, then hung up.
    fn close_terminal(&mut self, id: usize) {
        self.link.send(&ToServer::Close { id: id as u32 });
        self.remove_term(id);
    }

    // Keys

    /// Returns true when the app should quit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        let pressed = Key::of(key);
        // The prefix works from anywhere: it closes a popup, a prompt or a
        // question first.
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
            self.repos_key(key);
            return false;
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
            if matches!(key.code, KeyCode::Char('y' | 'Y')) {
                return self.confirmed(confirm);
            }
            return false;
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
            Focus::Workspaces | Focus::Sessions => self.list_key(key),
            Focus::Files => self.files_key(key),
            Focus::Changes => self.changes_key(key),
        }
        false
    }

    /// The key after the prefix. True to quit.
    fn command(&mut self, key: Key) -> bool {
        let KeyCode::Char(c) = key.code else { return false };
        match c {
            'w' => self.set_focus(Focus::Workspaces),
            's' => self.set_focus(Focus::Sessions),
            't' => self.set_focus(Focus::Terminal),
            'f' => self.set_focus(Focus::Files),
            'c' => self.set_focus(Focus::Changes),
            'n' => self.new_here(),
            'x' => self.close_here(),
            '[' => self.cycle_tab(-1),
            ']' => self.cycle_tab(1),
            'a' => self.next_waiting(),
            '?' => self.help = true,
            '<' => {
                self.left_hidden = !self.left_hidden;
                if self.left_hidden && matches!(self.focus, Focus::Workspaces | Focus::Sessions) {
                    self.set_focus(Focus::Terminal);
                }
            }
            '>' => {
                self.right_hidden = !self.right_hidden;
                if self.right_hidden && matches!(self.focus, Focus::Files | Focus::Changes) {
                    self.set_focus(Focus::Terminal);
                }
            }
            '/' => self.start_search(),
            'd' | 'e' | 'r' if self.focus != Focus::Files => self.message = Some(format!("{} f first: that's for files", self.keymap.label())),
            'd' => self.ask_delete(),
            'e' => self.edit(),
            'r' => self.start_rename(),
            'q' | 'Q' if self.editing() => {
                self.message = Some(UNSAVED.into());
            }
            'q' => return true,
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

    /// The question was answered y. True to quit.
    fn confirmed(&mut self, confirm: Confirm) -> bool {
        match confirm {
            Confirm::RemoveWorkspace(repo) => {
                let name = repo.file_name().unwrap_or_default().to_string_lossy().into_owned();
                workspace::unregister(&repo);
                self.message = Some(format!("removed {name} from the list (files kept)"));
                self.load_workspaces();
            }
            Confirm::DeleteSession(root) => self.remove_session(&root),
            Confirm::CloseTerminal(id) => self.close_terminal(id),
            Confirm::DeleteFile(path) => self.delete(&path),
            Confirm::StopAll => {
                // Setups still running stop too: they're what Q means by everything.
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

    /// Move the focus; into the terminal, a shell when the session has none.
    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        // Going to a hidden side shows it again.
        match focus {
            Focus::Workspaces | Focus::Sessions => self.left_hidden = false,
            Focus::Files | Focus::Changes => self.right_hidden = false,
            Focus::Terminal => {}
        }
        self.discard = false;
        self.reading = false;
        match focus {
            Focus::Terminal => {
                self.retry_server();
                if self.active.is_none() && self.ready {
                    self.new_terminal(true);
                }
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

    /// Prefix n: a new one of what has the focus.
    fn new_here(&mut self) {
        match self.focus {
            Focus::Workspaces => self.open_repo_picker(),
            Focus::Sessions => self.prompt = Some(Prompt { ask: Ask::Session, text: String::new() }),
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
        }
    }

    /// Prefix x: close what has the focus, asking first when something
    /// would be lost.
    fn close_here(&mut self) {
        if matches!(self.focus, Focus::Workspaces | Focus::Sessions) && self.editing() {
            self.message = Some(UNSAVED.into());
            return;
        }
        match self.focus {
            Focus::Workspaces => {
                let (repo, name) = (self.ws().repo.clone(), self.ws().name.clone());
                if workspace::registered().contains(&repo) {
                    self.ask(format!("remove {name} from the list? its files stay."), Confirm::RemoveWorkspace(repo));
                }
            }
            Focus::Sessions => match self.isolated.iter().find(|s| s.root == self.ws().root).cloned() {
                Some(session) => {
                    let lost = session::unsaved(&session);
                    let lost = if lost.is_empty() { String::new() } else { format!(", losing {}", lost.join(" and ")) };
                    let question = format!("delete session {}: its copy, containers and database{lost}?", session.name);
                    self.ask(question, Confirm::DeleteSession(session.root));
                }
                None if self.ws().is_worktree() => self.message = Some("this session is a git worktree; Pulpo leaves worktrees to git".into()),
                None => self.message = Some(format!("this is the repository itself: {p} w, then {p} x takes it off the list", p = self.keymap.label())),
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
            Focus::Changes => {}
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(prompt) = &mut self.prompt else { return };
        match key.code {
            KeyCode::Esc => self.prompt = None,
            KeyCode::Enter => {
                if let Some(Prompt { ask, text }) = self.prompt.take() {
                    let text = text.trim().to_string();
                    if !text.is_empty() {
                        match ask {
                            Ask::Session => self.create_session(&text),
                            Ask::File(dir) => self.create(&dir, &text),
                            Ask::Rename(path) => self.rename(&path, &text),
                        }
                    }
                }
            }
            // Session names are lowercase, like Compose projects.
            KeyCode::Char(c) if !ctrl && matches!(prompt.ask, Ask::Session) => prompt.text.push(c.to_ascii_lowercase()),
            _ => {
                input::edit(&mut prompt.text, key);
            }
        }
    }

    /// Keys in Workspaces and Sessions: moving shows what's selected.
    fn list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Home | KeyCode::Char('g') => self.step(i32::MIN),
            KeyCode::End | KeyCode::Char('G') => self.step(i32::MAX),
            KeyCode::Enter | KeyCode::Right if self.focus == Focus::Workspaces => self.set_focus(Focus::Sessions),
            KeyCode::Enter | KeyCode::Right => self.set_focus(Focus::Terminal),
            KeyCode::Left if self.focus == Focus::Sessions => self.set_focus(Focus::Workspaces),
            _ => {}
        }
    }

    fn step(&mut self, delta: i32) {
        let (repos, sessions) = (self.repos().len(), self.sessions().len());
        let (state, len) = match self.focus {
            Focus::Workspaces => (&mut self.workspaces_state, repos),
            _ => (&mut self.sessions_state, sessions),
        };
        let from = state.selected().unwrap_or(0) as i64;
        let next = (from + delta as i64).clamp(0, len.saturating_sub(1) as i64) as usize;
        state.select(Some(next));
        self.on_select();
    }

    /// Follow the selection: a repository goes back to the session you last
    /// used in it.
    fn on_select(&mut self) {
        match self.focus {
            Focus::Workspaces => {
                if let Some(main) = self.workspaces_state.selected().and_then(|i| self.repos().get(i).copied()) {
                    let ws = &self.workspaces[main];
                    let last = self.last_session.get(&ws.repo).and_then(|root| self.workspaces.iter().position(|w| &w.root == root));
                    self.set_current(last.unwrap_or(main));
                }
            }
            _ => {
                if let Some(i) = self.sessions_state.selected().and_then(|i| self.sessions().get(i).copied()) {
                    self.set_current(i);
                }
            }
        }
    }

    // Files

    /// The file selected in the tree, in the file pane.
    fn preview(&mut self) {
        if self.editor.is_some() {
            return;
        }
        let root = self.ws().root.clone();
        match self.ws().selected_row().filter(|r| !r.is_dir).map(|r| r.path.clone()) {
            Some(path) => self.viewer.open(Source::File(path), &root),
            None => self.viewer.close(),
        }
    }

    fn files_key(&mut self, key: KeyEvent) {
        if self.editor.is_some() {
            return self.editor_key(key);
        }
        // Opened wide: the tree still moves, tab goes to the file and back,
        // page up and down scroll it from either.
        if self.files_expanded {
            let half = (self.viewer.height / 2).max(1) as isize;
            match key.code {
                // Out of the file to the tree, then back to the three columns.
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
        self.tree_key(key);
    }

    /// Keys in the Files tree: move, fold, and ⏎ to open a file wide, the
    /// keys on it (esc goes back to the tree).
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

    /// Unfold or fold the selected folder of the tree; on a folded one (or a
    /// file), folding goes to its parent.
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

    /// The file or folder selected in the tree.
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

    /// A path for `name` typed in `dir`, if it stays inside the session.
    fn inside(&mut self, dir: &Path, name: &str) -> Option<PathBuf> {
        let path = dir.join(name.trim_end_matches('/'));
        let inside = path.starts_with(&self.ws().root) && !path.components().any(|c| c == std::path::Component::ParentDir);
        if !inside {
            self.message = Some("files stay inside the session".into());
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

    /// A new file, opened in the editor; or with a / at the end, a folder.
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

    /// Prefix e: the selected file in the editor, the whole right side.
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

    /// Back to the file in the viewer, where the cursor was.
    fn close_editor(&mut self) {
        let Some(editor) = self.editor.take() else { return };
        let root = self.ws().root.clone();
        self.viewer.close();
        self.viewer.open_at(editor.path.clone(), None, &root);
        self.viewer.scroll = editor.scroll;
        // Back to the tree, still wide: esc again for the three columns.
        self.files_expanded = true;
        self.reading = false;
        self.discard = false;
    }

    /// Keys in the editor: the usual ones (VS Code's). Esc closes it.
    fn editor_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let cmd = key.modifiers.contains(KeyModifiers::SUPER);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let Some(editor) = &mut self.editor else { return };
        // Typing what to find.
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
                    // Look again from where this match starts.
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
                self.message = Some("unsaved changes: esc again throws them away, ⌘s saves them".into());
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

    // Search

    /// Prefix /: a popup over whatever is on screen: the session's files as
    /// a tree, filtered as you type; tab searches inside them instead.
    fn start_search(&mut self) {
        self.search = Some(Search::new(Mode::Files, self.ws().root.clone(), &self.tx));
    }

    /// No repository yet: pick one first.
    pub fn open_repo_picker(&mut self) {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        self.repos = Some(Search::new(Mode::Repos, home, &self.tx));
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
        // With nothing typed, the files are the session's own tree.
        let in_tree = search.mode == Mode::Files && search.query.is_empty();
        match key.code {
            KeyCode::Esc => self.search = None,
            KeyCode::Enter => self.open_hit(),
            KeyCode::Up => self.search_step(-1),
            KeyCode::Down => self.search_step(1),
            KeyCode::Char('k' | 'p') if ctrl => self.search_step(-1),
            KeyCode::Char('j' | 'n') if ctrl => self.search_step(1),
            KeyCode::PageUp => self.search_step(-10),
            KeyCode::PageDown => self.search_step(10),
            KeyCode::Right if in_tree => self.fold_tree(true),
            KeyCode::Left if in_tree => self.fold_tree(false),
            KeyCode::Tab | KeyCode::BackTab => {
                search.mode = if search.mode == Mode::Files { Mode::Text } else { Mode::Files };
                search.update(&self.tx);
            }
            _ => {
                if input::edit(&mut search.query, key) {
                    search.update(&self.tx);
                }
            }
        }
    }

    /// Move in the popup: in the session's tree, or in the results.
    fn search_step(&mut self, delta: isize) {
        match &mut self.search {
            Some(search) if search.mode == Mode::Files && search.query.is_empty() => {
                let w = &mut self.workspaces[self.current];
                let max = w.tree.rows.len().saturating_sub(1) as isize;
                let next = (w.files_state.selected().unwrap_or(0) as isize).saturating_add(delta).clamp(0, max);
                w.files_state.select(Some(next as usize));
                self.preview();
            }
            Some(search) => search.step(delta),
            None => {}
        }
    }

    /// ⏎ in the popup: a folder folds or unfolds (among matches, it shows
    /// in the tree); a file opens in Files, full screen, on the line found.
    fn open_hit(&mut self) {
        let Some(search) = &self.search else { return };
        let picked = match search.mode {
            Mode::Files if search.query.is_empty() => self.ws().selected_row().map(|r| (r.path.clone(), None, r.is_dir)),
            Mode::Files => search.selected_row().map(|r| (r.path.clone(), None, r.is_dir)),
            _ => search.selected().map(|h| (search.root.join(&h.rel), h.line, false)),
        };
        let Some((path, line, is_dir)) = picked else { return };
        if is_dir {
            if search.query.is_empty() {
                let open = !self.ws().selected_row().is_some_and(|r| r.expanded);
                return self.fold_tree(open);
            }
            self.ws_mut().tree.set_expanded(&path, true);
            self.ws_mut().reveal(&path);
            if let Some(search) = &mut self.search {
                search.query.clear();
                search.update(&self.tx);
            }
            return;
        }
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

    /// A file in Files, shown in the tree and in the file pane, on `line`.
    fn show_file(&mut self, path: PathBuf, line: Option<usize>) {
        self.ws_mut().reveal(&path);
        let root = self.ws().root.clone();
        self.viewer.open_at(path, line, &root);
    }

    fn repos_key(&mut self, key: KeyEvent) {
        let Some(search) = &mut self.repos else { return };
        match key.code {
            KeyCode::Esc => self.repos = None,
            KeyCode::Enter => {
                let picked = search.selected().map(|h| h.rel.clone()).or_else(|| (!search.query.is_empty()).then(|| search.query.clone()));
                self.repos = None;
                if let Some(picked) = picked {
                    self.add_repo(&picked);
                }
            }
            KeyCode::Up => search.step(-1),
            KeyCode::Down => search.step(1),
            KeyCode::PageUp => search.step(-10),
            KeyCode::PageDown => search.step(10),
            _ => {
                if input::edit(&mut search.query, key) {
                    search.update(&self.tx);
                }
            }
        }
    }

    // Changes

    fn changes_key(&mut self, key: KeyEvent) {
        // As in Files: opened wide, the tree still moves, tab goes to the
        // diff and back, page up and down scroll it from either.
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

    // Mouse

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
                // Yes on the bottom line answers; a click anywhere else is no.
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
            // Shift + wheel scrolls sideways, like most editors.
            MouseEventKind::ScrollUp if shift => (0, -6),
            MouseEventKind::ScrollDown if shift => (0, 6),
            MouseEventKind::ScrollUp => (-3, 0),
            MouseEventKind::ScrollDown => (3, 0),
            // Horizontal swipes on a trackpad.
            MouseEventKind::ScrollLeft => (0, -4),
            MouseEventKind::ScrollRight => (0, 4),
            _ => (0, 0),
        };
        // Dragging a border: the sidebar's, the right side's, a tree's.
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
        // Selecting text in a terminal: copied when the button comes up.
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
        // A click elsewhere ends what was selected.
        if down {
            self.selecting = None;
        }
        // At the same place: a column off still counts, another panel doesn't.
        let double = down && self.last_click.is_some_and(|(at, when)| at.y == pos.y && at.x.abs_diff(pos.x) <= 1 && when.elapsed() < DOUBLE_CLICK);
        if down {
            self.last_click = Some((pos, Instant::now()));
        }
        let row = |area: Rect, offset: usize| (pos.y - area.y) as usize + offset;
        // Only a click moves the focus: hovering or scrolling a panel leaves it.

        if down && areas.counts.contains(pos) {
            self.next_waiting();
        } else if down && areas.workspaces.contains(pos) {
            self.set_focus(Focus::Workspaces);
            let i = row(areas.workspaces, self.workspaces_state.offset());
            if i < self.repos().len() {
                self.workspaces_state.select(Some(i));
                self.on_select();
            }
        } else if down && areas.sessions.contains(pos) {
            self.set_focus(Focus::Sessions);
            // Two rows per session.
            let i = (pos.y - areas.sessions.y) as usize / 2 + self.sessions_state.offset();
            if i < self.sessions().len() {
                self.sessions_state.select(Some(i));
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
        }
    }

    /// In the search popup or the repository picker: a click picks a row
    /// (twice: opens it), the wheel moves, a click outside closes it.
    fn popup_mouse(&mut self, mouse: MouseEvent) {
        let pos = Position::new(mouse.column, mouse.row);
        let (popup, list) = (self.areas.popup, self.areas.popup_list);
        let down = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        let delta = match mouse.kind {
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::ScrollDown => 1,
            _ if down && list.contains(pos) => {
                // From the row selected to the one clicked.
                let offset_and_at = match (&self.search, &self.repos) {
                    (Some(s), _) if s.mode == Mode::Files && s.query.is_empty() => {
                        let state = &self.ws().files_state;
                        (state.offset(), state.selected().unwrap_or(0))
                    }
                    (Some(s), _) | (None, Some(s)) => (s.state.offset(), s.state.selected().unwrap_or(0)),
                    (None, None) => return,
                };
                (pos.y - list.y) as isize + offset_and_at.0 as isize - offset_and_at.1 as isize
            }
            _ if down && !popup.contains(pos) => {
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

    /// A program that asked for the mouse gets it (unless shift is down);
    /// else the wheel scrolls back, a drag selects to copy, and ctrl-click
    /// opens a link or a path.
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

    /// A link opens in the browser; a path (with `:line`, as tools print
    /// them) shows in Files, there.
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

    // Task sessions

    /// Copy the repository's main checkout into a new session, in the
    /// background (`on_copied` goes on).
    fn create_session(&mut self, name: &str) {
        let (repo, name, tx) = (self.ws().repo.clone(), name.to_string(), self.tx.clone());
        let place = self.place(&repo).unwrap_or_default();
        // Asked twice: the first copy is still being made.
        if !self.copying.insert((repo.clone(), name.clone())) {
            self.message = Some(format!("session {name} is being made already"));
            return;
        }
        self.message = Some(format!("copying {place} into session {name}…"));
        std::thread::spawn(move || {
            let made = session::config(&repo).and_then(|(config, _)| session::create(&repo, &name, &config));
            let _ = tx.send(Msg::Copied((repo, name), made.map(|s| s.root).map_err(|e| format!("could not make the session: {e:#}"))));
        });
    }

    /// The copy is made: show it with a shell in it, yours at once, and set
    /// it up in the background: main's images, a copy of its database, its
    /// own containers.
    pub fn on_copied(&mut self, copy: (PathBuf, String), made: Result<PathBuf, String>) {
        self.copying.remove(&copy);
        let root = match made {
            Ok(root) => root,
            Err(e) => {
                self.message = Some(e);
                return;
            }
        };
        self.load_workspaces();
        let Some(session) = self.isolated.iter().find(|s| s.root == root).cloned() else { return };
        if let Some(i) = self.workspaces.iter().position(|w| w.root == root) {
            self.set_current(i);
        }
        self.spawn_in(root.clone(), &shell(), &[], true);
        let (config, warnings) = match session::config(&session.repo) {
            Ok(config) => config,
            Err(e) => {
                self.message = Some(format!("session {}: could not read its settings: {e:#}", session.name));
                return;
            }
        };
        let script = session::setup_script(&session, &config, &warnings);
        let log = logs().join(format!("setup-{}.log", session.project));
        match run_in_background(&script, &root, &session, &log) {
            Ok(mut child) => {
                self.setting_up.insert(root.clone(), child.id());
                let tx = self.tx.clone();
                std::thread::spawn(move || {
                    let ok = child.wait().is_ok_and(|status| status.success());
                    let _ = tx.send(Msg::SetUp(root, log, ok));
                });
                self.message = Some(format!("session {} is yours; setting it up in the background", session.name));
            }
            Err(e) => self.message = Some(format!("could not set up session {}: {e}", session.name)),
        }
    }

    pub fn on_set_up(&mut self, root: PathBuf, log: PathBuf, ok: bool) {
        self.setting_up.remove(&root);
        let name = root.file_name().unwrap_or_default().to_string_lossy().into_owned();
        self.message = Some(if ok { format!("session {name} is set up") } else { format!("the setup of session {name} stopped: see {}", log.display()) });
        // Marked in Sessions until it's gone, and said again on going there.
        if !ok {
            self.setup_failed.insert(root, log);
        }
    }

    /// Stop the session's terminals, take it off the list, and in the
    /// background remove its containers and volumes (its database) and the
    /// copy.
    fn remove_session(&mut self, root: &Path) {
        let Some(session) = self.isolated.iter().find(|s| s.root == root).cloned() else { return };
        let Ok(exe) = std::env::current_exe() else { return };
        let config = session::config(&session.repo).map(|(c, _)| c).unwrap_or_default();
        let script = session::teardown_script(&session, &config, &exe);
        // Its terminals stop the usual way (ctrl-c, then hang up).
        let ids: Vec<usize> = self.terms.iter().filter(|t| t.info.cwd == session.root).map(|t| t.id).collect();
        for id in ids {
            self.close_terminal(id);
        }
        // From main, with the session's Compose project and ports.
        let log = logs().join(format!("remove-{}.log", session.project));
        let mut child = match run_in_background(&script, &session.repo, &session, &log) {
            Ok(child) => child,
            Err(e) => {
                self.message = Some(format!("could not delete session {}: {e}", session.name));
                return;
            }
        };
        let (tx, root) = (self.tx.clone(), session.root.clone());
        std::thread::spawn(move || {
            let ok = child.wait().is_ok_and(|status| status.success());
            let _ = tx.send(Msg::Removed(root, log, ok));
        });
        // Off the list now, as if gone.
        if let Some(main) = self.workspaces.iter().position(|w| w.root == session.repo) {
            self.set_current(main);
        }
        self.removing.insert(session.root.clone());
        self.load_workspaces();
        self.message = Some(format!("session {} deleted", session.name));
    }

    /// A session's removal ended: if it failed, the session is back.
    pub fn on_removed(&mut self, root: PathBuf, log: PathBuf, ok: bool) {
        self.removing.remove(&root);
        self.load_workspaces();
        if ok {
            let _ = std::fs::remove_file(&log);
        } else {
            let name = root.file_name().unwrap_or_default().to_string_lossy();
            self.message = Some(format!("could not delete session {name}; see {}", log.display()));
        }
    }
}

/// Where setup and removal logs go: not in the state folder itself, which
/// is watched for the agents' events.
fn logs() -> PathBuf {
    let dir = crate::agents::state_dir().join("logs");
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// `script` run by sh in `dir` with the session's environment, its output
/// in `log`; in its own process group, so it outlives Pulpo.
fn run_in_background(script: &str, dir: &Path, session: &session::Session, log: &Path) -> std::io::Result<std::process::Child> {
    use std::os::unix::process::CommandExt;
    let out = std::fs::File::create(log)?;
    std::process::Command::new("sh")
        .args(["-c", script])
        .current_dir(dir)
        .envs(session::env(session))
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

/// Reading keys in a file at full height: down, up, a half page, top,
/// bottom, sideways.
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

/// The wheel over a terminal: its history, or, like Ghostty, arrow keys
/// for full-screen programs (less, man…).
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

/// A desktop notification from the terminal Pulpo runs in (OSC 9, which
/// Ghostty shows as a macOS notification).
fn notify(text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
    crate::term::to_host(format!("\x1b]9;{text}\x07").as_bytes());
}

/// Put `text` on the system clipboard.
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

/// What is on the system clipboard.
fn clipboard() -> Option<String> {
    let out = std::process::Command::new("pbpaste").output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}
