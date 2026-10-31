use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{Block, ListState};

use crate::Msg;
use crate::agents::{Agents, State};
use crate::client::{self, Link};
use crate::editor::Editor;
use crate::git;
use crate::layout::{self, Layout};
use crate::procs;
use crate::proto::{PROTOCOL, ToClient, ToServer};
use crate::search::{Mode, Search};
use crate::session;
use crate::status::Status;
use crate::term::Term;
use crate::viewer::{Source, Viewer};
use crate::workspace::{self, GraphItem, Workspace};

/// At most one desktop notification per terminal this often.
const NOTICE_EVERY: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    Workspaces,
    Sessions,
    Changes,
    Graph,
    Viewer,
    Terminal,
    /// The built-in editor (`editor.rs`).
    Editor,
}

/// Where you were before opening a file from the files popup: esc in the
/// viewer comes back to it, terminal zoomed or file previewed alike.
struct Back {
    focus: Focus,
    zoomed: bool,
    viewer: Viewer,
}

/// A file or folder about to be made in `dir`; a name ending in / is a folder.
/// Or, with `session`, a new isolated session of the repository.
pub struct Create {
    pub dir: PathBuf,
    pub name: String,
    pub session: bool,
}

/// Where each panel was drawn, for mouse hit-testing.
#[derive(Default, Clone, Copy)]
pub struct Areas {
    pub workspaces: Rect,
    pub sessions: Rect,
    pub changes: Rect,
    pub graph: Rect,
    pub terminal: Rect,
    pub viewer: Rect,
    /// The editor's text (inside its border).
    pub editor: Rect,
    /// The viewer's overview ruler.
    pub ruler: Rect,
}

pub struct App {
    pub workspaces: Vec<Workspace>,
    /// The isolated sessions among them: copies of a main checkout with
    /// their own Docker project and ports.
    pub isolated: Vec<session::Session>,
    pub current: usize,
    pub workspaces_state: ListState,
    /// Shown when no repository is registered yet.
    scratch: PathBuf,
    pub focus: Focus,
    /// The sidebar list to return to when leaving the terminal or viewer.
    sidebar_focus: Focus,
    pub agents: Agents,
    /// Every terminal, of every session; each belongs to the checkout it
    /// was started in.
    pub terms: Vec<Term>,
    /// The id of the terminal on screen, one of the current session's.
    pub active: Option<usize>,
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
    /// The server that runs the terminals.
    link: Link,
    /// Terminals asked of the server and not shown yet: whether to go into
    /// each when it comes. Its token stands in for its id until then.
    spawning: HashMap<u64, bool>,
    next_token: u64,
    /// Upgrading the server was asked once already.
    upgrade_asked: bool,
    /// Connected again after the server went away (an upgrade, or it
    /// stopped), with this many terminals before; and when.
    reconnected: Option<usize>,
    last_reconnect: Option<Instant>,
    /// Rows and columns available to terminals, set when drawing.
    term_size: (u16, u16),
    pub viewer: Viewer,
    pub sessions_state: ListState,
    pub areas: Areas,
    pub message: Option<String>,
    pub search: Option<Search>,
    /// The full list of shortcuts is shown.
    pub help: bool,
    /// n was pressed: w, s or f next says what to make.
    pub new_menu: bool,
    /// Typing the name of a new file or folder.
    pub creating: Option<Create>,
    /// The terminal editing a file: its id, the file, and the panel to go
    /// back to when the editor quits.
    editing: Option<(usize, PathBuf, Focus)>,
    /// The terminal removing a session; the list is reloaded when it ends.
    removing: Option<usize>,
    /// A file being edited in Pulpo, and the panel to go back to after.
    pub editor: Option<Editor>,
    editor_back: Focus,
    /// Set while a file opened from the files popup is in the viewer.
    files_back: Option<Back>,
    /// What the pending x deletes, so the selection can't change under it.
    deleting: Option<PathBuf>,
    /// The terminal takes the whole right side; the viewer comes back when
    /// something is previewed.
    pub zoomed: bool,
    /// A key that must be pressed again to confirm (quit, close, delete).
    pending: Option<char>,
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
        let mut app = App {
            workspaces: Vec::new(),
            isolated: Vec::new(),
            current: 0,
            workspaces_state: ListState::default().with_selected(Some(0)),
            scratch,
            focus: Focus::Sessions,
            sidebar_focus: Focus::Sessions,
            agents,
            terms: Vec::new(),
            active: None,
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
            term_size: (24, 80),
            viewer: Viewer::default(),
            sessions_state: ListState::default().with_selected(Some(0)),
            areas: Areas::default(),
            message: (!crate::agents::hooks_installed())
                .then(|| "run `pulpo hooks install` to see what Claude runs and asks in each terminal".into()),
            search: None,
            help: false,
            new_menu: false,
            creating: None,
            editing: None,
            editor: None,
            editor_back: Focus::Sessions,
            removing: None,
            deleting: None,
            files_back: None,
            zoomed: false,
            pending: None,
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
        if let Some(i) = start {
            app.set_current(i);
        }
        Ok(app)
    }

    pub fn ws(&self) -> &Workspace {
        &self.workspaces[self.current]
    }

    fn ws_mut(&mut self) -> &mut Workspace {
        &mut self.workspaces[self.current]
    }

    // Workspaces

    /// Rebuild the list from the registry and git, keeping existing state.
    fn load_workspaces(&mut self) {
        let current = self.workspaces.get(self.current).map(|w| w.root.clone());
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
            for s in isolated.iter().filter(|s| s.repo == repo && s.root.is_dir()) {
                self.workspaces.push(reuse(s.root.clone(), &repo));
                self.isolated.push(s.clone());
            }
        }
        if self.workspaces.is_empty() {
            let scratch = self.scratch.clone();
            self.workspaces.push(reuse(scratch.clone(), &scratch));
        }
        self.current = current.and_then(|c| self.workspaces.iter().position(|w| w.root == c)).unwrap_or(0);
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

    /// Make session `i` current: its files, changes and graph, and its
    /// terminals, on the one you last had on screen there.
    fn set_current(&mut self, i: usize) {
        if i >= self.workspaces.len() {
            return;
        }
        if i != self.current {
            self.current = i;
            self.viewer.close();
            self.workspaces[i].refresh();
            self.restore_term();
        }
        let ws = &self.workspaces[i];
        self.last_session.insert(ws.repo.clone(), ws.root.clone());
        self.sync_lists();
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
            // An agent changed the file being edited: take it, unless you
            // changed it too.
            if let Some(editor) = &mut self.editor
                && editor.reload_if_changed()
            {
                self.message = Some(format!("{} changed on disk: reloaded", editor.path.display()));
            }
        }
    }

    pub fn refresh(&mut self) {
        self.load_workspaces();
        for w in &mut self.workspaces {
            w.refresh();
        }
        let root = self.ws().root.clone();
        self.viewer.reload(&root);
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

    /// Take the current repository off the list; its files are untouched.
    fn remove_workspace(&mut self) {
        let (repo, name) = (self.ws().repo.clone(), self.ws().name.clone());
        workspace::unregister(&repo);
        self.message = Some(format!("removed {name} from the list (files kept)"));
        self.load_workspaces();
    }

    // Terminals

    pub fn spawn(&mut self, program: &str, focus: bool) {
        self.run(program, &[], focus);
    }

    /// Start `program` in a new terminal. The server shows it shortly; until
    /// then the returned token stands in for its id.
    fn run(&mut self, program: &str, args: &[String], focus: bool) -> Option<usize> {
        let cwd = self.ws().root.clone();
        Some(self.spawn_in(cwd, program, args, focus) as usize)
    }

    /// Start `program` in a new terminal working in `cwd`; its token.
    fn spawn_in(&mut self, cwd: PathBuf, program: &str, args: &[String], focus: bool) -> u64 {
        let token = self.next_token;
        self.next_token += 1;
        let (rows, cols) = self.term_size;
        // An isolated session's terminals get its Compose project and ports.
        let env = self.isolated.iter().find(|s| s.root == cwd).map(session::env).unwrap_or_default();
        self.link.send(&ToServer::Spawn { token, program: program.into(), args: args.to_vec(), cwd, rows, cols, env });
        self.spawning.insert(token, focus);
        token
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
                // Brought back after a restart: it stands for the old one, and
                // the session shows the one you had on screen.
                if let Some(old) = token.and_then(|t| self.reopening.remove(&t)) {
                    for tag in self.last_term.values_mut().filter(|tag| **tag == old) {
                        *tag = self.terms[i].info.tag.clone();
                    }
                    self.spawning.remove(&token.unwrap_or_default());
                    if session.is_none_or(|s| s == self.current) {
                        self.restore_term();
                    }
                    return;
                }
                let Some(focus) = token.and_then(|t| self.spawning.remove(&t)) else {
                    // One that kept running is shown once all are in (`Ready`),
                    // the way you left it. Another Pulpo's, if this session has
                    // nothing on screen.
                    if self.ready && self.active.is_none() && session.is_none_or(|s| s == self.current) {
                        self.restore_term();
                    }
                    return;
                };
                // Ours: what waited for its id gets it.
                let token = token.unwrap_or_default() as usize;
                let waiting = [self.editing.as_mut().map(|e| &mut e.0), self.removing.as_mut()];
                for waiting in waiting.into_iter().flatten() {
                    if *waiting == token {
                        *waiting = id;
                    }
                }
                // Ours: show it, in its session (you may have moved on since).
                if focus && let Some(session) = session {
                    self.set_current(session);
                }
                if focus || session.is_none_or(|s| s == self.current) {
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
                    self.spawn(&shell(), false);
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
            ToClient::Exit { id } => self.on_exit(id as usize),
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
        // Keep what was open on disk until the server says what it still has.
        self.ready = false;
        self.terms.clear();
        self.spawning.clear();
        self.active = None;
        if self.focus == Focus::Terminal {
            self.set_focus(self.sidebar_focus);
        }
        if self.last_reconnect.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
            self.message = Some("the Pulpo server keeps stopping: see its log, ~/.local/state/pulpo/server/log".into());
            return;
        }
        self.last_reconnect = Some(Instant::now());
        match client::connect(self.tx.clone()) {
            Ok(link) => {
                self.link = link;
                self.reconnected = Some(had);
            }
            Err(e) => self.message = Some(format!("{e:#}")),
        }
    }

    fn on_exit(&mut self, id: usize) {
        self.remove_term(id);
        // A session was removed: off the list.
        if self.removing == Some(id) {
            self.removing = None;
            self.load_workspaces();
            self.restore_term();
        }
        // The editor quit: show the file where you were.
        if self.editing.as_ref().is_some_and(|(e, ..)| *e == id)
            && let Some((_, path, back)) = self.editing.take()
        {
            let root = self.ws().root.clone();
            if path.is_file() {
                self.viewer.open(Source::File(path), &root);
                self.viewer.reload(&root);
            }
            self.zoomed = false;
            self.set_focus(back);
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        if let Some(create) = &mut self.creating {
            create.name.push_str(text.trim());
        } else if let Some(search) = &mut self.search {
            search.query.push_str(text.trim());
            search.update(&self.tx);
        } else if self.viewer.find.as_ref().is_some_and(|f| f.typing) {
            self.viewer.edit_find(|q| q.push_str(text.trim()));
        } else if self.focus == Focus::Editor
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

    pub fn resize_terms(&mut self, rows: u16, cols: u16) {
        self.term_size = (rows.max(1), cols.max(1));
        for term in &mut self.terms {
            term.resize(rows.max(1), cols.max(1));
        }
    }

    /// Working while the program prints on its own; then, until you look at
    /// it, waiting with something to read. A program that reports its
    /// status (OSC 7501) is taken at its word. What it runs or asks goes in
    /// the notification, not here.
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
    /// leave something to read, unless you are in that terminal.
    pub fn tick(&mut self) {
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
        self.terms.iter().any(|term| self.term_state(term) == State::Running)
    }

    /// The session a terminal belongs to: the checkout it started in.
    fn session_of(&self, term: &Term) -> Option<usize> {
        self.workspaces.iter().position(|w| w.root == term.info.cwd)
    }

    /// The current session's terminals, as indexes into `terms`. One started
    /// outside every checkout shows in all of them, so it stays reachable.
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

    fn select_term(&mut self, id: usize) {
        self.active = Some(id);
        if let Some(term) = self.terms.iter().find(|t| t.id == id) {
            self.last_term.insert(self.ws().root.clone(), term.info.tag.clone());
        }
    }

    /// Show the terminal this session last had on screen, or its first.
    fn restore_term(&mut self) {
        let terms: Vec<&Term> = self.session_terms().into_iter().map(|i| &self.terms[i]).collect();
        let last = self.last_term.get(&self.ws().root).and_then(|tag| terms.iter().find(|t| &t.info.tag == tag));
        self.active = last.or(terms.first()).map(|t| t.id);
    }

    /// Take a terminal off the list. If it was on screen, its neighbor in
    /// the session takes its place.
    fn remove_term(&mut self, id: usize) {
        let ids: Vec<usize> = self.session_terms().into_iter().map(|i| self.terms[i].id).collect();
        let at = ids.iter().position(|&t| t == id);
        self.terms.retain(|t| t.id != id);
        if self.active == Some(id) {
            let rest: Vec<usize> = ids.into_iter().filter(|&t| t != id).collect();
            self.active = at.and_then(|at| rest.get(at.min(rest.len().saturating_sub(1))).copied());
            if let Some(next) = self.active {
                self.select_term(next);
            }
        }
        if self.active.is_none() && self.focus == Focus::Terminal {
            self.set_focus(self.sidebar_focus);
        }
    }

    // Input

    /// Returns true when the app should quit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if self.help {
            self.help = false;
            return false;
        }
        if self.new_menu {
            self.new_menu = false;
            match key.code {
                KeyCode::Char('w') => self.open_repo_picker(),
                // A new session: a copy of main with its own Docker; a new terminal in this one.
                KeyCode::Char('s') => self.creating = Some(Create { dir: self.ws().repo.clone(), name: String::new(), session: true }),
                KeyCode::Char('t') => self.spawn(&shell(), true),
                KeyCode::Char('f') => self.start_create(),
                _ => {}
            }
            return false;
        }
        if let Some(create) = &mut self.creating {
            match key.code {
                KeyCode::Esc => self.creating = None,
                KeyCode::Enter => self.create(),
                KeyCode::Backspace => {
                    create.name.pop();
                }
                // Session names are lowercase, like Compose projects.
                KeyCode::Char(c) if !ctrl => create.name.push(if create.session { c.to_ascii_lowercase() } else { c }),
                _ => {}
            }
            return false;
        }
        if self.search.is_some() {
            self.search_key(key);
            return false;
        }
        if self.focus == Focus::Viewer && self.viewer.find.as_ref().is_some_and(|f| f.typing) {
            match key.code {
                KeyCode::Esc => self.viewer.clear_find(),
                KeyCode::Enter => self.viewer.finish_find(),
                KeyCode::Backspace => self.viewer.edit_find(|q| {
                    q.pop();
                }),
                KeyCode::Char(c) if !ctrl => self.viewer.edit_find(|q| q.push(c)),
                _ => {}
            }
            return false;
        }
        let pending = self.pending.take();
        if self.focus == Focus::Terminal {
            self.terminal_key(key, pending);
            return false;
        }
        if self.focus == Focus::Editor && self.editor.is_some() {
            self.editor_key(key, pending);
            return false;
        }

        self.message = None;
        match key.code {
            // Panels, by their initial; t goes into the terminal.
            KeyCode::Char('w') => self.set_focus(Focus::Workspaces),
            KeyCode::Char('s') if !ctrl => self.set_focus(Focus::Sessions),
            KeyCode::Char('c') => self.set_focus(Focus::Changes),
            // In the viewer, g goes to the top instead.
            KeyCode::Char('g') if self.focus != Focus::Viewer => self.set_focus(Focus::Graph),
            // The editor, when open, is drawn where the viewer is.
            KeyCode::Char('v') if self.editor.is_some() => self.set_focus(Focus::Editor),
            KeyCode::Char('v') if self.viewer.is_open() => self.set_focus(Focus::Viewer),
            // The files: a tree, filtered as you type; tab searches inside them, as / does.
            KeyCode::Char('f') if !ctrl => self.open_search(Mode::Files),
            // ctrl-s, as inside a terminal: back and forth.
            KeyCode::Char('s') => self.toggle_terminal(),
            KeyCode::Char('t') if self.active.is_some() => self.toggle_terminal(),
            KeyCode::Char('t') => self.spawn(&shell(), true),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => self.cycle_focus(-1),
            KeyCode::Tab => self.cycle_focus(1),
            KeyCode::BackTab => self.cycle_focus(-1),

            // / finds in the file on screen, or else inside all the files.
            KeyCode::Char('/') if self.focus == Focus::Viewer => self.viewer.start_find(),
            KeyCode::Char('/') => self.open_search(Mode::Text),
            // n / p: next, previous search match or change.
            KeyCode::Char('n') if self.focus == Focus::Viewer && (self.viewer.find.is_some() || self.viewer.is_diff()) => {
                self.viewer.next(1)
            }
            KeyCode::Char('p' | 'N')
                if !ctrl && self.focus == Focus::Viewer && (self.viewer.find.is_some() || self.viewer.is_diff()) =>
            {
                self.viewer.next(-1)
            }
            KeyCode::Char('n') => self.new_menu = true,
            // ctrl-f, or z for zoom: the current terminal, full height.
            KeyCode::Char('z') => self.zoom_terminal(),
            KeyCode::Char('f') if ctrl => self.zoom_terminal(),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') => self.refresh(),
            // Terminals keep running in the server; say which, and their ports.
            KeyCode::Char('q' | 'Q') if self.editor.as_ref().is_some_and(|e| e.dirty) => {
                self.message = Some("unsaved changes in the editor: ctrl-s saves them, esc there discards them".into());
            }
            KeyCode::Char('q') => {
                if (self.terms.is_empty() && self.isolated.is_empty()) || pending == Some('q') {
                    return true;
                }
                let mut running = self.still_running();
                // Their containers run on their own, whatever the terminals do.
                let docker: Vec<String> = self.isolated.iter().map(|s| s.name.clone()).collect();
                if !docker.is_empty() {
                    running.push_str(&format!("; Docker of session(s) {} (x x in Sessions removes one)", docker.join(", ")));
                }
                self.confirm('q', format!("press q again to quit; these keep running: {running} (Q stops the terminals)"));
            }
            KeyCode::Char('Q') => {
                if self.terms.is_empty() {
                    return true;
                }
                if pending == Some('Q') {
                    for term in &self.terms {
                        self.link.send(&ToServer::Close { id: term.info.id });
                    }
                    // Stopped on purpose: nothing to bring back next time.
                    layout::save(&Layout { open: Vec::new(), ..self.layout() });
                    return true;
                }
                let n = self.terms.len();
                self.confirm('Q', format!("press Q again to stop {n} terminal(s) and quit (ctrl-c first, then hang up)"));
            }
            KeyCode::Char('x') if self.focus == Focus::Sessions && self.isolated_current().is_some() => self.remove_session(pending),
            KeyCode::Char('x') if self.focus == Focus::Sessions && self.ws().is_worktree() => {
                self.message = Some("this session is a git worktree; Pulpo leaves worktrees to git".into());
            }
            KeyCode::Char('x') if self.focus == Focus::Sessions => {
                self.message = Some("this is the repository itself; x in Workspaces takes it off the list".into());
            }
            KeyCode::Char('x') if self.focus == Focus::Workspaces => {
                if pending == Some('x') {
                    self.remove_workspace();
                } else {
                    let name = self.ws().name.clone();
                    self.confirm('x', format!("press x again to remove {name} from the list (files are kept)"));
                }
            }
            KeyCode::Char('x') => self.close_terminal('x', "x", pending),
            KeyCode::Char('e') => self.edit(),
            KeyCode::Char('y') => self.copy_path(),
            KeyCode::Char(d @ '1'..='9') => self.enter_term(d as usize - '1' as usize),
            KeyCode::Char('[') => self.cycle_term(-1),
            KeyCode::Char(']') => self.cycle_term(1),
            KeyCode::Esc if self.focus == Focus::Viewer && self.viewer.find.is_some() => self.viewer.clear_find(),
            KeyCode::Esc if self.focus == Focus::Viewer && self.files_back.is_some() => self.go_back(),
            KeyCode::Esc if self.focus == Focus::Viewer => self.set_focus(self.sidebar_focus),
            KeyCode::Esc => self.viewer.close(),
            _ if self.focus == Focus::Viewer => self.viewer_key(key),
            _ => self.list_key(key),
        }
        false
    }

    /// In a terminal every key goes to the program, except a few ctrl keys.
    fn terminal_key(&mut self, key: KeyEvent, pending: Option<char>) {
        // The editor gets every key; you come back by quitting it.
        if self.in_editor() {
            let save = key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL);
            if let Some(term) = self.term_mut() {
                term.key(key);
                // In micro, saving also closes it: back to the file in the viewer.
                if save && term.info.name == "micro" {
                    term.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL));
                }
            }
            return;
        }
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if pending.is_some() {
            self.message = None;
        }
        match key.code {
            KeyCode::Char('s') if ctrl => self.toggle_terminal(),
            KeyCode::Char('f') if ctrl => self.zoomed = !self.zoomed,
            // Another shell in this session.
            KeyCode::Char('n') if ctrl => self.spawn(&shell(), true),
            KeyCode::Char('q') if ctrl => self.close_terminal('Q', "ctrl-q", pending),
            KeyCode::Char(d @ '1'..='9') if ctrl => self.enter_term(d as usize - '1' as usize),
            KeyCode::Char('[') if ctrl => self.cycle_term(-1),
            KeyCode::Char(']') if ctrl => self.cycle_term(1),
            _ => {
                if let Some(term) = self.term_mut() {
                    term.key(key);
                }
            }
        }
    }

    /// Focus the viewer at full height; Esc goes back to the split layout.
    fn read_in_viewer(&mut self) {
        self.set_focus(Focus::Viewer);
        self.viewer.expanded = true;
    }

    fn zoom_terminal(&mut self) {
        match self.active {
            Some(_) => self.set_focus(Focus::Terminal),
            None => self.spawn(&shell(), true),
        }
        self.zoomed = self.focus == Focus::Terminal;
    }

    /// Close the active terminal right away if its shell is idle; if a
    /// program runs in it (Claude, a server…), ask for the key again first.
    fn close_terminal(&mut self, confirm_key: char, key_name: &str, pending: Option<char>) {
        let Some(term) = self.term() else { return };
        match term.info.running.clone() {
            Some(program) if pending != Some(confirm_key) => {
                let n = self.session_terms().iter().position(|&i| self.terms[i].id == term.id).unwrap_or(0) + 1;
                self.confirm(confirm_key, format!("{program} is running in terminal {n}: press {key_name} again to close it"));
            }
            _ => self.close_active(),
        }
    }

    /// What keeps running when Pulpo quits: each terminal's program with
    /// the ports it listens on, and Docker, whose ports belong to Docker's
    /// own processes and don't show.
    fn still_running(&self) -> String {
        let table = procs::table();
        let trees: Vec<Vec<&procs::Proc>> = self.terms.iter().map(|t| procs::tree(&table, t.info.pid)).collect();
        let pids: Vec<i32> = trees.iter().flatten().map(|p| p.pid).collect();
        let ports = procs::listening(&pids);
        // By session: "pulpo: claude · 2 shells; backbone/fix: npm :3000".
        let mut sessions: Vec<(String, Vec<String>, usize)> = Vec::new();
        for (term, tree) in self.terms.iter().zip(&trees) {
            let place = self.place(&term.info.cwd).unwrap_or_else(|| term.info.cwd.display().to_string());
            let i = match sessions.iter().position(|(p, ..)| *p == place) {
                Some(i) => i,
                None => {
                    sessions.push((place, Vec::new(), 0));
                    sessions.len() - 1
                }
            };
            let (_, parts, shells) = &mut sessions[i];
            let mut listening: Vec<u16> = ports.iter().filter(|(pid, _)| tree.iter().any(|p| p.pid == *pid)).map(|&(_, port)| port).collect();
            listening.sort_unstable();
            listening.dedup();
            let docker = tree.iter().any(|p| p.name.starts_with("docker"));
            if tree.len() <= 1 && listening.is_empty() {
                *shells += 1;
                continue;
            }
            let mut part = term.label().to_string();
            for port in listening {
                part.push_str(&format!(" :{port}"));
            }
            if docker {
                part.push_str(" (docker)");
            }
            parts.push(part);
        }
        let one = sessions.len() == 1;
        let sessions: Vec<String> = sessions
            .into_iter()
            .map(|(place, mut parts, shells)| {
                match shells {
                    0 => {}
                    1 => parts.push("1 shell".into()),
                    n => parts.push(format!("{n} shells")),
                }
                if one { parts.join(" · ") } else { format!("{place}: {}", parts.join(" · ")) }
            })
            .collect();
        sessions.join("; ")
    }

    /// Go into the session's terminal number `n` (from 0).
    fn enter_term(&mut self, n: usize) {
        if let Some(&i) = self.session_terms().get(n) {
            self.select_term(self.terms[i].id);
            self.set_focus(Focus::Terminal);
        }
    }

    /// The session's next or previous terminal, wrapping around.
    fn cycle_term(&mut self, dir: isize) {
        let ids: Vec<usize> = self.session_terms().into_iter().map(|i| self.terms[i].id).collect();
        if ids.is_empty() {
            return;
        }
        let at = self.active.and_then(|id| ids.iter().position(|&t| t == id)).unwrap_or(0) as isize;
        self.select_term(ids[(at + dir).rem_euclid(ids.len() as isize) as usize]);
    }

    fn close_active(&mut self) {
        let Some(id) = self.active else { return };
        if let Some(term) = self.term() {
            self.link.send(&ToServer::Close { id: term.info.id });
        }
        self.remove_term(id);
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        // A click or scroll can change what a second x would act on.
        if matches!(mouse.kind, MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
            self.pending = None;
        }
        if (self.help || self.new_menu) && matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.help = false;
            self.new_menu = false;
        }
        if self.search.is_some() || self.creating.is_some() || self.help || self.new_menu {
            return;
        }
        let pos = Position::new(mouse.column, mouse.row);
        let areas = self.areas;
        // In the editor: place the cursor, select by dragging, scroll.
        if areas.editor.contains(pos)
            && let Some(editor) = &mut self.editor
        {
            let (row, column) = ((pos.y - areas.editor.y) as usize, (pos.x - areas.editor.x) as usize);
            let column = column.saturating_sub(editor.gutter());
            let shift = mouse.modifiers.contains(KeyModifiers::SHIFT);
            match mouse.kind {
                MouseEventKind::Down(MouseButton::Left) => {
                    editor.click(row, column, shift);
                    self.set_focus(Focus::Editor);
                }
                MouseEventKind::Drag(MouseButton::Left) => editor.click(row, column, true),
                MouseEventKind::ScrollUp => editor.scroll_by(-3),
                MouseEventKind::ScrollDown => editor.scroll_by(3),
                _ => {}
            }
            return;
        }
        let (target, area) = [
            (Focus::Terminal, areas.terminal),
            (Focus::Viewer, areas.viewer),
            (Focus::Workspaces, areas.workspaces),
            (Focus::Sessions, areas.sessions),
            (Focus::Changes, areas.changes),
            (Focus::Graph, areas.graph),
        ]
        .into_iter()
        .find(|(_, area)| area.contains(pos))
        .unwrap_or((self.focus, Rect::default()));

        // Inside the terminal's text (not its border or title), a program
        // that asked for the mouse gets the event itself.
        let inner = Block::bordered().inner(area);
        if target == Focus::Terminal
            && inner.contains(pos)
            && let Some(term) = self.term_mut()
            && term.wants_mouse()
        {
            let (col, row) = (pos.x - inner.x + 1, pos.y - inner.y + 1);
            term.mouse(mouse.kind, col, row, mouse.modifiers);
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.set_focus(Focus::Terminal);
            }
            return;
        }

        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                if area.is_empty() || (target == Focus::Terminal && self.active.is_none()) {
                    return;
                }
                self.set_focus(target);
                if self.areas.ruler.contains(pos) {
                    self.jump_to_ruler(pos);
                    return;
                }
                let Some((state, len)) = self.list(target) else { return };
                let row = (pos.y.saturating_sub(area.y + 1)) as usize + state.offset();
                if pos.y == area.y || row >= len {
                    return;
                }
                state.select(Some(row));
                self.on_select();
                match target {
                    Focus::Sessions if self.active.is_some() => self.set_focus(Focus::Terminal),
                    Focus::Graph => self.toggle_commit(),
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let rows: isize = if mouse.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                match target {
                    Focus::Terminal => {
                        if let Some(term) = self.term_mut() {
                            if term.alternate_screen() {
                                // Like Ghostty: the wheel becomes arrow keys
                                // for full-screen programs (less, man…).
                                let arrow = if rows < 0 { KeyCode::Up } else { KeyCode::Down };
                                for _ in 0..rows.abs() {
                                    term.key(KeyEvent::new(arrow, KeyModifiers::NONE));
                                }
                            } else {
                                term.scroll(-rows);
                            }
                        }
                    }
                    // Shift + wheel scrolls sideways, like most editors.
                    Focus::Viewer if mouse.modifiers.contains(KeyModifiers::SHIFT) => self.viewer.scroll_sideways(rows * 2),
                    Focus::Viewer => self.viewer.scroll_by(rows),
                    _ if !area.is_empty() => {
                        self.set_focus(target);
                        self.step(rows.signum() as i32);
                    }
                    _ => {}
                }
            }
            MouseEventKind::Drag(MouseButton::Left) if self.areas.ruler.contains(pos) => self.jump_to_ruler(pos),
            // Horizontal swipes on a trackpad.
            MouseEventKind::ScrollLeft | MouseEventKind::ScrollRight if target == Focus::Viewer => {
                self.viewer.scroll_sideways(if mouse.kind == MouseEventKind::ScrollLeft { -4 } else { 4 });
            }
            _ => {}
        }
    }

    fn jump_to_ruler(&mut self, pos: Position) {
        let ruler = self.areas.ruler;
        self.viewer.jump_to_ruler((pos.y - ruler.y) as usize, ruler.height as usize);
    }

    // Search

    fn open_search(&mut self, mode: Mode) {
        self.search = Some(Search::new(mode, self.ws().root.clone(), &self.tx));
    }

    pub fn on_search(&mut self, generation: usize, hits: Vec<crate::search::Hit>) {
        if let Some(search) = &mut self.search {
            search.on_results(generation, hits);
        }
    }

    fn search_key(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let pending = self.pending.take();
        if pending.is_some() {
            self.message = None;
        }
        let Some(search) = &self.search else { return };
        let files = search.mode == Mode::Files;
        // With nothing typed, the files are the workspace's own tree.
        let in_tree = files && search.query.is_empty();
        let on_file = matches!(self.file_row(), Some((_, false)));
        match key.code {
            KeyCode::Esc => self.search = None,
            KeyCode::Enter if files => self.open_row(),
            KeyCode::Enter => self.open_hit(),
            KeyCode::Up => self.search_step(-1),
            KeyCode::Down => self.search_step(1),
            KeyCode::Char('k' | 'p') if ctrl => self.search_step(-1),
            KeyCode::Char('j' | 'n') if ctrl => self.search_step(1),
            KeyCode::PageUp => self.search_step(-10),
            KeyCode::PageDown => self.search_step(10),
            KeyCode::Right if in_tree => self.fold_tree(true),
            KeyCode::Left if in_tree => self.fold_tree(false),
            KeyCode::Char('e') if ctrl && files && on_file => {
                self.edit();
                self.search = None;
            }
            KeyCode::Char('e') if ctrl && files => self.message = Some("select a file to edit".into()),
            KeyCode::Char('y') if ctrl && files => self.copy_path(),
            KeyCode::Char('x') if ctrl && files => self.delete(pending),
            _ => self.search_input(key),
        }
    }

    /// Typing in the search popup, and tab between its two kinds of search.
    fn search_input(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(search) = &mut self.search else { return };
        match key.code {
            KeyCode::Tab | KeyCode::BackTab if search.mode != Mode::Repos => {
                search.mode = if search.mode == Mode::Files { Mode::Text } else { Mode::Files };
                search.update(&self.tx);
            }
            KeyCode::Backspace => {
                search.query.pop();
                search.update(&self.tx);
            }
            KeyCode::Char(c) if !ctrl => {
                search.query.push(c);
                search.update(&self.tx);
            }
            _ => {}
        }
    }

    fn open_hit(&mut self) {
        let Some(search) = self.search.take() else { return };
        let Some(hit) = search.selected() else { return };
        if search.mode == Mode::Repos {
            self.add_repo(&hit.rel);
            self.set_focus(Focus::Workspaces);
        } else {
            let path = search.root.join(&hit.rel);
            self.ws_mut().reveal(&path);
            self.remember_back();
            self.viewer.open_at(path, hit.line, &search.root);
            self.read_in_viewer();
        }
    }

    /// The file or folder selected in the files (popup), and whether it is a folder.
    fn file_row(&self) -> Option<(PathBuf, bool)> {
        let search = self.search.as_ref().filter(|s| s.mode == Mode::Files)?;
        let row = if search.query.is_empty() { self.ws().selected_row() } else { search.selected_row() }?;
        Some((row.path.clone(), row.is_dir))
    }

    /// Move in the search popup: in the workspace's tree, or in the results.
    fn search_step(&mut self, delta: isize) {
        match &mut self.search {
            Some(search) if search.mode == Mode::Files && search.query.is_empty() => {
                let w = &mut self.workspaces[self.current];
                let max = w.tree.rows.len().saturating_sub(1) as isize;
                let next = (w.files_state.selected().unwrap_or(0) as isize + delta).clamp(0, max);
                w.files_state.select(Some(next as usize));
            }
            Some(search) => search.step(delta),
            None => {}
        }
    }

    /// Enter in the files: open a file in the viewer, fold or unfold a
    /// folder of the tree. A folder among matches is shown in the tree.
    fn open_row(&mut self) {
        let Some((path, is_dir)) = self.file_row() else { return };
        let in_tree = self.search.as_ref().is_some_and(|s| s.query.is_empty());
        if is_dir && in_tree {
            let open = !self.ws().selected_row().is_some_and(|r| r.expanded);
            return self.fold_tree(open);
        }
        // The tree follows, so it opens where you were next time.
        self.ws_mut().reveal(&path);
        if is_dir {
            self.ws_mut().tree.set_expanded(&path, true);
            if let Some(search) = &mut self.search {
                search.query.clear();
                search.update(&self.tx);
            }
            return;
        }
        self.search = None;
        let root = self.ws().root.clone();
        self.remember_back();
        self.viewer.open_at(path, None, &root);
        self.read_in_viewer();
    }

    /// Before a file from the popup takes the viewer: keep where you were,
    /// unless already in a file opened that way.
    fn remember_back(&mut self) {
        if self.files_back.is_none() {
            let viewer = std::mem::take(&mut self.viewer);
            self.files_back = Some(Back { focus: self.focus, zoomed: self.zoomed, viewer });
        }
    }

    /// Esc in a file opened from the popup: everything as it was before.
    fn go_back(&mut self) {
        let Some(back) = self.files_back.take() else { return };
        self.viewer = back.viewer;
        let root = self.ws().root.clone();
        self.viewer.reload(&root);
        // A terminal closed meanwhile can't take the focus back.
        self.focus = if back.focus == Focus::Terminal && self.active.is_none() { self.sidebar_focus } else { back.focus };
        self.zoomed = back.zoomed;
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
    }

    pub fn open_repo_picker(&mut self) {
        let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
        self.search = Some(Search::new(Mode::Repos, home, &self.tx));
    }

    pub fn on_repos(&mut self, repos: Vec<String>) {
        if let Some(search) = &mut self.search {
            search.set_repos(repos, &self.tx);
        }
    }

    fn toggle_terminal(&mut self) {
        if self.focus == Focus::Terminal {
            self.set_focus(self.sidebar_focus);
        } else if self.active.is_some() {
            self.set_focus(Focus::Terminal);
        }
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        // Gone elsewhere: esc no longer goes back to before the popup.
        if focus != Focus::Viewer {
            self.files_back = None;
        }
        if focus == Focus::Viewer {
            self.zoomed = false;
        }
        if focus != Focus::Viewer {
            self.viewer.expanded = false;
        }
        if matches!(focus, Focus::Workspaces | Focus::Sessions | Focus::Changes | Focus::Graph) {
            self.sidebar_focus = focus;
        }
    }

    fn cycle_focus(&mut self, dir: isize) {
        let mut order = vec![Focus::Workspaces, Focus::Sessions, Focus::Changes, Focus::Graph];
        if self.editor.is_some() {
            order.push(Focus::Editor);
        } else if self.viewer.is_open() {
            order.push(Focus::Viewer);
        }
        let i = order.iter().position(|f| *f == self.focus).unwrap_or(0) as isize;
        let next = (i + dir).rem_euclid(order.len() as isize) as usize;
        self.set_focus(order[next]);
    }

    fn confirm(&mut self, key: char, message: String) {
        self.pending = Some(key);
        self.message = Some(message);
    }

    fn viewer_key(&mut self, key: KeyEvent) {
        let half = (self.viewer.height / 2).max(1) as isize;
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.viewer.scroll_by(1),
            KeyCode::Up | KeyCode::Char('k') => self.viewer.scroll_by(-1),
            KeyCode::PageDown | KeyCode::Char('d' | ' ') => self.viewer.scroll_by(half),
            KeyCode::PageUp | KeyCode::Char('u') => self.viewer.scroll_by(-half),
            KeyCode::Home | KeyCode::Char('g') => self.viewer.scroll_by(isize::MIN),
            KeyCode::End | KeyCode::Char('G') => self.viewer.scroll_by(isize::MAX),
            KeyCode::Left | KeyCode::Char('h') => self.viewer.scroll_sideways(-8),
            KeyCode::Right | KeyCode::Char('l') => self.viewer.scroll_sideways(8),
            KeyCode::Char('0') => self.viewer.scroll_sideways(0),
            KeyCode::Enter => self.viewer.expanded = !self.viewer.expanded,
            _ => {}
        }
    }

    fn list_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Down | KeyCode::Char('j') => self.step(1),
            KeyCode::Up | KeyCode::Char('k') => self.step(-1),
            KeyCode::Home => self.step(i32::MIN),
            KeyCode::End | KeyCode::Char('G') => self.step(i32::MAX),
            KeyCode::Enter => self.activate(),
            KeyCode::Right | KeyCode::Char('l') => self.expand(true),
            KeyCode::Left | KeyCode::Char('h') => self.expand(false),
            _ => {}
        }
    }

    fn list(&mut self, focus: Focus) -> Option<(&mut ListState, usize)> {
        let (repos, sessions) = (self.repos().len(), self.sessions().len());
        let w = &mut self.workspaces[self.current];
        match focus {
            Focus::Workspaces => Some((&mut self.workspaces_state, repos)),
            Focus::Sessions => Some((&mut self.sessions_state, sessions)),
            Focus::Changes => Some((&mut w.changes_state, w.status.changes.len())),
            Focus::Graph => {
                let len = w.graph_len();
                Some((&mut w.graph_state, len))
            }
            _ => None,
        }
    }

    fn step(&mut self, delta: i32) {
        let Some((state, len)) = self.list(self.focus) else { return };
        if len == 0 {
            return;
        }
        let from = state.selected().unwrap_or(0);
        let mut next = (from as i64 + delta as i64).clamp(0, len as i64 - 1) as usize;
        if self.focus == Focus::Graph {
            // Rows where branches fork or merge have no commit: pass them.
            let ws = self.ws();
            while matches!(ws.graph_item(next), Some(GraphItem::Row(row)) if row.commit.is_none()) {
                match next.checked_add_signed(delta.signum() as isize) {
                    Some(n) if n < len => next = n,
                    _ => {
                        next = from;
                        break;
                    }
                }
            }
        }
        if let Some((state, _)) = self.list(self.focus) {
            state.select(Some(next));
        }
        self.on_select();
    }

    /// Follow the selection: switch workspace or terminal, show the diff, or
    /// preview the file.
    fn on_select(&mut self) {
        match self.focus {
            // A repository: back to the session you last used in it.
            Focus::Workspaces => {
                if let Some(main) = self.workspaces_state.selected().and_then(|i| self.repos().get(i).copied()) {
                    let ws = &self.workspaces[main];
                    let last = self.last_session.get(&ws.repo).and_then(|root| self.workspaces.iter().position(|w| &w.root == root));
                    self.set_current(last.unwrap_or(main));
                }
            }
            Focus::Sessions => {
                if let Some(i) = self.sessions_state.selected().and_then(|i| self.sessions().get(i).copied()) {
                    self.set_current(i);
                }
            }
            Focus::Changes => {
                if let Some((path, untracked)) = self.ws().selected_change() {
                    let root = self.ws().root.clone();
                    self.viewer.open(Source::Diff { path, untracked }, &root);
                    self.zoomed = false;
                }
            }
            Focus::Graph => {
                let root = self.ws().root.clone();
                let source = match self.ws().selected_graph_item() {
                    Some(GraphItem::Row(row)) => row.commit.as_ref().map(|c| Source::Commit(c.hash.clone())),
                    Some(GraphItem::File(row, change)) => {
                        row.commit.as_ref().map(|c| Source::CommitFile { hash: c.hash.clone(), path: root.join(&change.path) })
                    }
                    None => None,
                };
                if let Some(source) = source {
                    self.viewer.open(source, &root);
                    self.zoomed = false;
                }
            }
            _ => {}
        }
    }

    fn activate(&mut self) {
        match self.focus {
            Focus::Workspaces => self.set_focus(Focus::Sessions),
            // Into the session's terminal, or a first shell in it.
            Focus::Sessions if self.active.is_some() => self.set_focus(Focus::Terminal),
            Focus::Sessions => self.spawn(&shell(), true),
            Focus::Graph if matches!(self.ws().selected_graph_item(), Some(GraphItem::Row(_))) => self.toggle_commit(),
            Focus::Changes | Focus::Graph => {
                self.on_select();
                if self.viewer.is_open() {
                    self.read_in_viewer();
                }
            }
            _ => {}
        }
    }

    /// Unfold the selected commit to list its files, or fold it.
    fn toggle_commit(&mut self) {
        let ws = self.ws();
        if let Some(GraphItem::Row(row)) = ws.selected_graph_item()
            && let Some(commit) = &row.commit
        {
            let (hash, open) = (commit.hash.clone(), !ws.is_commit_open(&commit.hash));
            self.ws_mut().set_commit_open(&hash, open);
        }
    }

    /// Unfold the selected commit in Graph to list its files, or fold it.
    fn expand(&mut self, open: bool) {
        if self.focus != Focus::Graph {
            return;
        }
        let ws = self.ws();
        let Some(hash) = ws.selected_commit().map(|c| c.hash.clone()) else { return };
        match ws.selected_graph_item() {
            // On a file, h goes back to its commit (and h again folds it).
            Some(GraphItem::File(..)) if !open => self.ws_mut().set_commit_open(&hash, true),
            Some(GraphItem::Row(_)) if ws.is_commit_open(&hash) != open => self.ws_mut().set_commit_open(&hash, open),
            _ => return,
        }
        self.on_select();
    }
}

impl App {
    // Isolated sessions

    /// The current session, if it is an isolated one.
    pub fn isolated_current(&self) -> Option<&session::Session> {
        let root = &self.ws().root;
        self.isolated.iter().find(|s| &s.root == root)
    }

    /// Copy the repository's main checkout into a new session, show it, and
    /// set it up in its first terminal: main's images, a copy of its
    /// database, its own containers. The terminal stays as its shell.
    fn create_session(&mut self, repo: &Path, name: &str) {
        let made = session::config(repo).and_then(|(config, warnings)| {
            let session = session::create(repo, name, &config)?;
            Ok((session::setup_script(&session, &config, &warnings), session.root))
        });
        let (script, root) = match made {
            Ok(made) => made,
            Err(e) => {
                self.message = Some(format!("could not make the session: {e:#}"));
                return;
            }
        };
        self.load_workspaces();
        if let Some(i) = self.workspaces.iter().position(|w| w.root == root) {
            self.set_current(i);
            self.set_focus(Focus::Sessions);
        }
        self.run("sh", &["-c".into(), script], true);
    }

    /// x x on an isolated session: stop its terminals, then in a terminal of
    /// main remove its containers and volumes (its database) and the copy.
    /// The first x says what would be lost.
    fn remove_session(&mut self, pending: Option<char>) {
        let Some(session) = self.isolated_current().cloned() else { return };
        if pending != Some('x') || self.deleting.as_ref() != Some(&session.root) {
            self.deleting = Some(session.root.clone());
            let lost = session::unsaved(&session);
            let lost = if lost.is_empty() { String::new() } else { format!(", losing {}", lost.join(" and ")) };
            self.confirm('x', format!("press x again to delete session {}: its copy, containers and database{lost}", session.name));
            return;
        }
        self.deleting = None;
        let Ok(exe) = std::env::current_exe() else { return };
        let config = session::config(&session.repo).map(|(c, _)| c).unwrap_or_default();
        let script = session::teardown_script(&session, &config, &exe);
        // Its terminals stop the usual way (ctrl-c, then hang up).
        let ids: Vec<usize> = self.terms.iter().filter(|t| t.info.cwd == session.root).map(|t| t.id).collect();
        for id in ids {
            self.link.send(&ToServer::Close { id: id as u32 });
            self.remove_term(id);
        }
        // Run from main, with the session's Compose project and ports.
        let Some(main) = self.workspaces.iter().position(|w| w.root == session.repo) else { return };
        self.set_current(main);
        let token = self.next_token;
        self.next_token += 1;
        let (rows, cols) = self.term_size;
        let env = session::env(&session);
        let spawn = ToServer::Spawn { token, program: "sh".into(), args: vec!["-c".into(), script], cwd: session.repo.clone(), rows, cols, env };
        self.link.send(&spawn);
        self.spawning.insert(token, false);
        self.removing = Some(token as usize);
        self.message = Some(format!("removing session {}…", session.name));
    }

    // Files

    /// Ask for a name, in the folder of the file on screen (or at the top).
    fn start_create(&mut self) {
        let root = self.ws().root.clone();
        let dir = match &self.viewer.source {
            Some(Source::File(path) | Source::Diff { path, .. }) => path.parent().filter(|d| d.starts_with(&root)).map(Path::to_path_buf),
            _ => None,
        };
        let dir = dir.unwrap_or(root);
        self.creating = Some(Create { dir, name: String::new(), session: false });
    }

    fn create(&mut self) {
        let Some(Create { dir, name, session }) = self.creating.take() else { return };
        let name = name.trim();
        if name.is_empty() {
            return;
        }
        if session {
            self.create_session(&dir, name);
            return;
        }
        let path = dir.join(name.trim_end_matches('/'));
        let root = self.ws().root.clone();
        if !path.starts_with(&root) || path.components().any(|c| c == std::path::Component::ParentDir) {
            self.message = Some("new files go inside the workspace".into());
            return;
        }
        let rel = path.strip_prefix(&root).unwrap_or(&path).display().to_string();
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
        // Where the files open next time.
        self.ws_mut().reveal(&path);
        if folder {
            self.message = Some(format!("created {rel}/"));
        } else {
            self.edit_path(path, None);
        }
    }

    /// Delete the file or folder selected in the files, after a second ctrl-x.
    fn delete(&mut self, pending: Option<char>) {
        let Some((path, is_dir)) = self.file_row() else { return };
        let ws = self.ws();
        let rel = path.strip_prefix(&ws.root).unwrap_or(&path).display().to_string();
        if pending != Some('x') || self.deleting.as_ref() != Some(&path) {
            let what = if is_dir { format!("{rel}/ and everything in it") } else { rel };
            let undo = if git::is_tracked(&ws.root, &path) {
                "git can restore the last commit"
            } else {
                "not in git, it can't be recovered"
            };
            self.deleting = Some(path);
            self.confirm('x', format!("press ctrl-x again to delete {what} ({undo})"));
            return;
        }
        self.deleting = None;
        let deleted = if is_dir { std::fs::remove_dir_all(&path) } else { std::fs::remove_file(&path) };
        match deleted {
            Ok(()) => {
                if let Some(Source::File(open) | Source::Diff { path: open, .. }) = &self.viewer.source
                    && open.starts_with(&path)
                {
                    self.viewer.close();
                }
                let w = self.ws_mut();
                w.tree.rebuild();
                w.reselect(None);
                if let Some(search) = &mut self.search {
                    search.rescan(&self.tx);
                }
                self.message = Some(format!("deleted {rel}"));
            }
            Err(e) => self.message = Some(format!("could not delete {rel}: {e}")),
        }
    }

    /// Edit the file in the viewer, or the one selected in the files or Changes.
    fn edit(&mut self) {
        match self.selected_path() {
            Some((path, line)) if path.is_file() => self.edit_path(path, line),
            Some((path, _)) if path.is_dir() => self.message = Some("select a file to edit".into()),
            Some(_) => self.message = Some("that file is not in the working tree".into()),
            None => self.message = Some("select a file to edit".into()),
        }
    }

    /// Edit a file in your own editor ($PULPO_EDITOR), or else here.
    fn edit_path(&mut self, path: PathBuf, line: Option<usize>) {
        if external_editor().is_some() {
            self.open_editor(path, line);
        } else {
            self.edit_here(path, line);
        }
    }

    /// The file (or folder, in the files) selected, and in the viewer the
    /// line on screen.
    fn selected_path(&self) -> Option<(PathBuf, Option<usize>)> {
        if self.search.is_some() {
            return self.file_row().map(|(path, _)| (path, None));
        }
        let ws = self.ws();
        match self.focus {
            Focus::Viewer => match &self.viewer.source {
                Some(Source::File(path)) => Some((path.clone(), Some(self.viewer.mark.unwrap_or(self.viewer.scroll) + 1))),
                Some(Source::Diff { path, .. } | Source::CommitFile { path, .. }) => Some((path.clone(), None)),
                _ => None,
            },
            Focus::Changes => ws.selected_change().map(|(path, _)| (path, None)),
            Focus::Graph => match ws.selected_graph_item() {
                Some(GraphItem::File(_, change)) => Some((ws.root.join(&change.path), None)),
                _ => None,
            },
            _ => None,
        }
    }

    /// Copy the selected path, relative to the workspace, to the clipboard.
    fn copy_path(&mut self) {
        let Some((path, _)) = self.selected_path() else {
            self.message = Some("select a file to copy its path".into());
            return;
        };
        let rel = path.strip_prefix(&self.ws().root).unwrap_or(&path).display().to_string();
        self.message = Some(if set_clipboard(&rel) { format!("copied {rel}") } else { "could not copy to the clipboard".into() });
    }

    /// Edit `path` in Pulpo, on the right, looking exactly like the viewer.
    fn edit_here(&mut self, path: PathBuf, line: Option<usize>) {
        // Already open, or unsaved changes there: back to it instead.
        if let Some(editor) = &self.editor
            && (editor.path == path || editor.dirty)
        {
            if editor.path != path {
                self.message = Some("unsaved changes in the editor: ctrl-s saves them, esc discards them".into());
            }
            self.set_focus(Focus::Editor);
            return;
        }
        match Editor::open(path, line) {
            Ok(editor) => {
                self.editor_back = if matches!(self.focus, Focus::Terminal | Focus::Editor) { self.sidebar_focus } else { self.focus };
                self.editor = Some(editor);
                self.set_focus(Focus::Editor);
            }
            Err(e) => self.message = Some(e),
        }
    }

    /// Close the editor; with unsaved changes, only when asked twice. The
    /// viewer then shows the file where the cursor was.
    fn close_editor(&mut self, pending: Option<char>) {
        let Some(editor) = &self.editor else { return };
        if editor.dirty && pending != Some('E') {
            self.confirm('E', "unsaved changes: ctrl-s saves them, esc again closes without saving".into());
            return;
        }
        let Some(editor) = self.editor.take() else { return };
        let root = self.ws().root.clone();
        self.viewer.open_at(editor.path.clone(), Some(editor.cursor.line + 1), &root);
        self.viewer.reload(&root);
        self.set_focus(self.editor_back);
    }

    /// Keys in the editor: the usual ones (micro, VS Code).
    fn editor_key(&mut self, key: KeyEvent, pending: Option<char>) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        self.message = None;
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
                KeyCode::Char(c) if !ctrl => {
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
        match key.code {
            KeyCode::Esc if editor.find.is_some() => editor.find = None,
            KeyCode::Esc => return self.close_editor(pending),
            KeyCode::Char('q') if ctrl => return self.close_editor(pending),
            KeyCode::Char('s') if ctrl => {
                let message = match editor.save() {
                    Ok(()) => format!("saved {}", editor.path.file_name().unwrap_or_default().to_string_lossy()),
                    Err(e) => e,
                };
                self.message = Some(message);
                return;
            }
            KeyCode::Char('z') if ctrl && shift => editor.redo(),
            KeyCode::Char('Z') if ctrl => editor.redo(),
            KeyCode::Char('z') if ctrl => editor.undo(),
            KeyCode::Char('y') if ctrl => editor.redo(),
            KeyCode::Char('a') if ctrl => editor.select_all(),
            KeyCode::Char('c') if ctrl => {
                let text = editor.copy();
                set_clipboard(&text);
            }
            KeyCode::Char('x') if ctrl => {
                let text = editor.cut();
                set_clipboard(&text);
            }
            KeyCode::Char('v') if ctrl => {
                if let Some(text) = clipboard() {
                    editor.insert(&text);
                }
            }
            KeyCode::Char('f') if ctrl => {
                editor.find_typing = true;
                editor.find.get_or_insert_default();
            }
            KeyCode::Char('g') if ctrl => {
                editor.find_next(!shift);
            }
            KeyCode::Left if alt || ctrl => editor.word(false, shift),
            KeyCode::Right if alt || ctrl => editor.word(true, shift),
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
            KeyCode::Char(c) if !ctrl => editor.insert(&c.to_string()),
            _ => {}
        }
        editor.follow_cursor();
    }

    /// Open `path` in your own editor ($PULPO_EDITOR), in a terminal, full
    /// height.
    fn open_editor(&mut self, path: PathBuf, line: Option<usize>) {
        let Some(mut args) = external_editor() else { return };
        let program = args.remove(0);
        let back = if self.focus == Focus::Terminal { self.sidebar_focus } else { self.focus };
        args.extend(editor_look(&program));
        let file = path.display().to_string();
        match line {
            Some(n) if program.ends_with("hx") => args.push(format!("{file}:{n}")),
            Some(n) => args.extend([format!("+{n}"), file]),
            None => args.push(file),
        }
        if let Some(id) = self.run(&program, &args, true) {
            self.editing = Some((id, path, back));
            self.zoomed = true;
        }
    }

    /// The terminal on screen is a file editor.
    pub fn in_editor(&self) -> bool {
        self.editing.as_ref().is_some_and(|(id, ..)| self.active == Some(*id))
    }

}

/// A desktop notification from the terminal Pulpo runs in (OSC 9, which
/// Ghostty shows as a macOS notification).
fn notify(text: &str) {
    let text: String = text.chars().filter(|c| !c.is_control()).take(200).collect();
    crate::term::to_host(format!("\x1b]9;{text}\x07").as_bytes());
}

/// Your own terminal editor, if you set $PULPO_EDITOR; otherwise Pulpo
/// edits files itself. $EDITOR is not used, as it is often a GUI app.
fn external_editor() -> Option<Vec<String>> {
    let cmd = std::env::var("PULPO_EDITOR").ok().filter(|c| !c.trim().is_empty())?;
    Some(cmd.split_whitespace().map(String::from).collect())
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

/// Arguments giving the editor Pulpo's colors, unless you configured it
/// yourself. vim and nvim also get a few defaults (macOS vim otherwise
/// starts without syntax highlighting).
fn editor_look(program: &str) -> Vec<String> {
    let look = || -> Option<Vec<String>> {
        let home = PathBuf::from(std::env::var_os("HOME")?);
        let config = std::env::var_os("XDG_CONFIG_HOME").map_or_else(|| home.join(".config"), PathBuf::from);
        match Path::new(program).file_name()?.to_str()? {
            "micro" => {
                let dir = std::env::var_os("MICRO_CONFIG_HOME").map_or_else(|| config.join("micro"), PathBuf::from);
                let settings = std::fs::read_to_string(dir.join("settings.json")).unwrap_or_default();
                if settings.contains("\"colorscheme\"") {
                    return None;
                }
                // Only for this session: micro doesn't save options given as flags.
                std::fs::create_dir_all(dir.join("colorschemes")).ok()?;
                std::fs::write(dir.join("colorschemes/pulpo.micro"), include_str!("editor.micro")).ok()?;
                Some(vec!["-colorscheme".into(), "pulpo".into()])
            }
            name @ ("vim" | "vi" | "nvim") => {
                let own = match name {
                    "nvim" => vec![config.join("nvim/init.lua"), config.join("nvim/init.vim")],
                    _ => vec![home.join(".vimrc"), home.join(".vim/vimrc"), config.join("vim/vimrc")],
                };
                if own.iter().any(|f| f.exists()) {
                    return None;
                }
                let script = std::env::temp_dir().join("pulpo-editor.vim");
                std::fs::write(&script, include_str!("editor.vim")).ok()?;
                Some(vec!["-S".into(), script.display().to_string()])
            }
            _ => None,
        }
    };
    look().unwrap_or_default()
}

pub fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}
