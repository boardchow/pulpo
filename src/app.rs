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
use crate::keymap::{Action, Key, Keymap};
use crate::layout::{self, Layout};
use crate::pane::{self, Dir, Node, Pane, SavedTab, Tab};
use crate::picker::{self, Entry, Kind, Picker, Show, Target};
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
/// How long a message stays in the footer, unless it asks something.
const MESSAGE_FOR: Duration = Duration::from_secs(5);
/// How long after the prefix its panel shows, so quick hands never see it.
const PREFIX_PANEL_AFTER: Duration = Duration::from_millis(300);

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
#[derive(Default, Clone)]
pub struct Areas {
    pub workspaces: Rect,
    pub sessions: Rect,
    pub changes: Rect,
    pub graph: Rect,
    /// The panes of the tab on screen, and the tabs in the top bar.
    pub panes: Vec<(Pane, Rect)>,
    pub tabs: Vec<(usize, Rect)>,
    /// The top bar's counts: a click shows those terminals.
    pub counts: Vec<(Show, Rect)>,
    pub viewer: Rect,
    /// The editor's text (inside its border).
    pub editor: Rect,
    /// The viewer's overview ruler.
    pub ruler: Rect,
}

/// A session's tabs, and the one on screen.
#[derive(Default)]
pub struct Tabs {
    pub list: Vec<Tab>,
    pub active: usize,
}

/// A terminal asked of the server: whether to go into it, and the pane to
/// split for it (and whether beside), else it gets a tab of its own.
type Spawning = (bool, Option<(Pane, bool)>);

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
    /// The terminal the keys go to: the one last in focus in the tab on
    /// screen.
    pub active: Option<usize>,
    /// Each session's tabs (`pane.rs`), by its folder.
    pub tabs: HashMap<PathBuf, Tabs>,
    /// The tabs layout.json had are back; until then it keeps them.
    tabs_restored: bool,
    /// Naming the tab on screen.
    pub renaming: Option<String>,
    /// Go to, the menu, or the themes.
    pub picker: Option<Picker>,
    /// The themes the theme picker lists, and the one to put back on esc.
    themes: Vec<String>,
    theme_before: String,
    /// The session and terminal on screen, and the ones before them.
    here: Option<(PathBuf, Option<usize>)>,
    back: Option<(PathBuf, Option<usize>)>,
    pub sidebar_hidden: bool,
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
    /// Terminals asked of the server and not shown yet, and where they go.
    /// Its token stands in for its id until then.
    spawning: HashMap<u64, Spawning>,
    next_token: u64,
    /// Upgrading the server was asked once already.
    upgrade_asked: bool,
    /// Connected again after the server went away (an upgrade, or it
    /// stopped), with this many terminals before; and when.
    reconnected: Option<usize>,
    last_reconnect: Option<Instant>,
    /// The server stopped twice in a row, or couldn't start: tried again on r.
    offline: bool,
    /// Rows and columns available to terminals, set when drawing.
    term_size: (u16, u16),
    pub viewer: Viewer,
    pub sessions_state: ListState,
    pub areas: Areas,
    pub message: Option<String>,
    /// The message on screen, and since when.
    message_since: Option<(String, Instant)>,
    pub search: Option<Search>,
    /// The full list of shortcuts is shown, filtered by what you typed.
    pub help: Option<String>,
    /// Pulpo's keys beyond the panels' own (`keymap.rs`).
    pub keymap: Keymap,
    /// The prefix was pressed, at this time: the next key is Pulpo's.
    pub prefix: Option<Instant>,
    /// The old ctrl keys inside terminals already said what replaces them.
    legacy_hinted: HashSet<char>,
    /// n was pressed: w, s or f next says what to make.
    pub new_menu: bool,
    /// Typing the name of a new file or folder.
    pub creating: Option<Create>,
    /// The terminal editing a file: its id, the file, and the panel to go
    /// back to when the editor quits.
    editing: Option<(usize, PathBuf, Focus)>,
    /// Sessions being removed in the background, already off the list.
    removing: HashSet<PathBuf>,
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
        let (keymap, keys_error) = Keymap::load();
        let theme_error = crate::theme::apply(&crate::theme::chosen()).err().map(|e| format!("theme: {e}; using Linear Dark"));
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
            tabs: HashMap::new(),
            tabs_restored: false,
            renaming: None,
            picker: None,
            themes: Vec::new(),
            theme_before: String::new(),
            here: None,
            back: None,
            sidebar_hidden: false,
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
            viewer: Viewer::default(),
            sessions_state: ListState::default().with_selected(Some(0)),
            areas: Areas::default(),
            message: keys_error.or(theme_error).or_else(|| {
                (!crate::agents::hooks_installed())
                    .then(|| "run `pulpo hooks install` to see what Claude runs and asks in each terminal".into())
            }),
            message_since: None,
            search: None,
            help: None,
            keymap,
            prefix: None,
            legacy_hinted: HashSet::new(),
            new_menu: false,
            creating: None,
            editing: None,
            editor: None,
            editor_back: Focus::Sessions,
            removing: HashSet::new(),
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
            None => self.current = 0,
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
        self.retry_server();
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
        self.spawning.insert(token, (focus, None));
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
        let tabs = if self.tabs_restored {
            let tag = |id: usize| self.terms.iter().find(|t| t.id == id).map(|t| t.info.tag.clone());
            self.tabs
                .iter()
                .map(|(root, tabs)| {
                    let saved = tabs.list.iter().filter_map(|tab| Some(SavedTab { root: tab.root.save(&tag)?, name: tab.name.clone() }));
                    (root.clone(), saved.collect::<Vec<_>>())
                })
                .filter(|(_, saved)| !saved.is_empty())
                .collect()
        } else {
            self.saved.tabs.clone()
        };
        Layout {
            current: Some(self.ws().root.clone()),
            sessions: self.last_session.clone().into_iter().collect(),
            terminals: self.last_term.clone().into_iter().collect(),
            tabs,
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
                    let new = self.terms[i].info.tag.clone();
                    for tag in self.last_term.values_mut().filter(|tag| **tag == old) {
                        *tag = new.clone();
                    }
                    for saved in self.saved.tabs.values_mut().flatten() {
                        saved.root.rename(&old, &new);
                    }
                    self.spawning.remove(&token.unwrap_or_default());
                    // All back: their tabs too.
                    if self.reopening.is_empty() || session.is_none_or(|s| s == self.current) {
                        self.restore_term();
                    }
                    return;
                }
                let Some((focus, split)) = token.and_then(|t| self.spawning.remove(&t)) else {
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
                if let Some((editing, ..)) = &mut self.editing
                    && *editing == token
                {
                    *editing = id;
                }
                // Split off a pane, if that pane is still there.
                if let Some((at, beside)) = split {
                    let root = session.map_or_else(|| self.ws().root.clone(), |s| self.workspaces[s].root.clone());
                    if let Some(tab) = self.tabs.get_mut(&root).and_then(|t| t.list.iter_mut().find(|tab| tab.root.contains(at))) {
                        tab.root.split(at, Pane::Term(id), beside, 50);
                    }
                }
                self.sync_tabs();
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
        // Keep what was open on disk until the server says what it still has,
        // and the tabs by their terminals' tags.
        if self.tabs_restored {
            self.saved.tabs = self.layout().tabs;
        }
        self.tabs.clear();
        self.tabs_restored = false;
        self.ready = false;
        self.terms.clear();
        self.spawning.clear();
        self.active = None;
        if self.focus == Focus::Terminal {
            self.set_focus(self.sidebar_focus);
        }
        if self.last_reconnect.is_some_and(|at| at.elapsed() < Duration::from_secs(2)) {
            self.offline = true;
            self.message = Some("the Pulpo server keeps stopping (its log: ~/.local/state/pulpo/server/log); r tries again".into());
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
                self.message = Some(format!("{e:#}; r tries again"));
            }
        }
    }

    /// On r: if the server was given up on, try it again. Not on every
    /// key: starting it can take seconds.
    fn retry_server(&mut self) {
        if std::mem::take(&mut self.offline) {
            self.last_reconnect = None;
            self.on_server_gone();
        }
    }

    fn on_exit(&mut self, id: usize) {
        self.remove_term(id);
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
        } else if let Some(name) = &mut self.renaming {
            name.push_str(text.trim());
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

    /// The size new terminals start at: the room on the right.
    pub fn set_term_size(&mut self, rows: u16, cols: u16) {
        self.term_size = (rows.max(1), cols.max(1));
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
        self.sync_tabs();
        let now = (self.ws().root.clone(), self.active);
        if self.here.as_ref() != Some(&now) {
            self.back = self.here.replace(now);
        }
        // Messages go after a while; a question stays until answered, and
        // the server being down until r.
        match &self.message {
            None => self.message_since = None,
            Some(m) if self.message_since.as_ref().is_none_or(|(shown, _)| shown != m) => self.message_since = Some((m.clone(), Instant::now())),
            Some(_) if self.pending.is_none() && !self.offline && self.message_since.as_ref().is_some_and(|(_, at)| at.elapsed() >= MESSAGE_FOR) => {
                self.message = None;
            }
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

    /// Something is working, so its spinner needs frequent redraws.
    pub fn animating(&self) -> bool {
        // Or the prefix panel is about to show.
        self.prefix.is_some() || self.terms.iter().any(|term| self.term_state(term) == State::Running)
    }

    /// The prefix was pressed a moment ago and no key followed: show what can.
    pub fn prefix_panel(&self) -> bool {
        self.prefix.is_some_and(|at| at.elapsed() >= PREFIX_PANEL_AFTER)
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

    /// Make terminal `id` the one keys go to, and its tab the one on screen.
    fn select_term(&mut self, id: usize) {
        self.active = Some(id);
        for tabs in self.tabs.values_mut() {
            if let Some(i) = tabs.list.iter().position(|t| t.root.contains(Pane::Term(id))) {
                tabs.active = i;
                tabs.list[i].set_focus(Pane::Term(id));
            }
        }
        if let Some(term) = self.terms.iter().find(|t| t.id == id) {
            self.last_term.insert(self.ws().root.clone(), term.info.tag.clone());
        }
    }

    /// Show the tab with the terminal this session last had on screen.
    fn restore_term(&mut self) {
        self.sync_tabs();
        let root = self.ws().root.clone();
        let terms: Vec<&Term> = self.session_terms().into_iter().map(|i| &self.terms[i]).collect();
        let last = self.last_term.get(&root).and_then(|tag| terms.iter().find(|t| &t.info.tag == tag)).map(|t| t.id);
        if !self.tabs_restored {
            self.active = last.or(terms.first().map(|t| t.id));
            return;
        }
        let tabbed = |id: usize| self.tabs.get(&root).is_some_and(|t| t.list.iter().any(|tab| tab.root.contains(Pane::Term(id))));
        match last.filter(|&id| tabbed(id)) {
            Some(id) => self.select_term(id),
            None => self.active = self.tab().and_then(|t| t.term),
        }
    }

    /// Take a terminal off the list. Another pane of its tab takes its
    /// room, or else the next tab.
    fn remove_term(&mut self, id: usize) {
        self.terms.retain(|t| t.id != id);
        if self.active == Some(id) {
            self.active = None;
        }
        self.sync_tabs();
        if let Some(next) = self.active {
            self.select_term(next);
        }
        if self.active.is_none() && self.focus == Focus::Terminal {
            let file = self.tab().is_some_and(|t| t.root.contains(Pane::File));
            if file { self.focus_pane(Pane::File) } else { self.set_focus(self.sidebar_focus) }
        }
    }

    // Tabs and panes

    /// The current session's tabs.
    pub fn tabs(&self) -> Option<&Tabs> {
        self.tabs.get(&self.ws().root)
    }

    /// The tab on screen.
    pub fn tab(&self) -> Option<&Tab> {
        self.tabs().and_then(|t| t.list.get(t.active))
    }

    fn tab_mut(&mut self) -> Option<&mut Tab> {
        let root = self.ws().root.clone();
        self.tabs.get_mut(&root).and_then(|t| t.list.get_mut(t.active))
    }

    /// The pane shown alone: the file pane when you read or edit it full
    /// height, or the terminal you zoomed.
    pub fn zoomed_pane(&self) -> Option<Pane> {
        let tab = self.tab()?;
        let in_file = matches!(self.focus, Focus::Viewer | Focus::Editor);
        if in_file && self.viewer.expanded && tab.root.contains(Pane::File) {
            return Some(Pane::File);
        }
        self.active.filter(|_| self.zoomed && !in_file).map(Pane::Term)
    }

    /// The pane keys go to, when they go to the right side.
    pub fn focused_pane(&self) -> Option<Pane> {
        match self.focus {
            Focus::Terminal => self.active.map(Pane::Term),
            Focus::Viewer | Focus::Editor => Some(Pane::File),
            _ => None,
        }
    }

    /// Every terminal in a tab of its session, and while a file is open,
    /// the file pane in the tab on screen; the terminal keys go to is that
    /// tab's. Waits until the server has shown what it has, so the tabs
    /// you had come back as they were.
    fn sync_tabs(&mut self) {
        if !self.ready || !self.reopening.is_empty() {
            return;
        }
        if !self.tabs_restored {
            self.restore_tabs();
        }
        let ids: HashSet<usize> = self.terms.iter().map(|t| t.id).collect();
        let current = self.ws().root.clone();
        let file_open = self.viewer.is_open() || self.editor.is_some();

        // What is gone, and empty tabs, go; the tab on screen stays where
        // it was, or its neighbor takes its place.
        for (root, tabs) in &mut self.tabs {
            let (on_screen, mut active) = (tabs.active, tabs.active);
            let mut kept = Vec::new();
            for (i, mut tab) in std::mem::take(&mut tabs.list).into_iter().enumerate() {
                let here = *root == current && i == on_screen;
                let gone: Vec<Pane> = tab
                    .root
                    .panes()
                    .into_iter()
                    .filter(|&p| match p {
                        Pane::Term(id) => !ids.contains(&id),
                        Pane::File => !(file_open && here),
                    })
                    .collect();
                if gone.into_iter().all(|pane| tab.remove(pane)) {
                    kept.push(tab);
                } else if i < on_screen {
                    active -= 1;
                }
            }
            tabs.active = active.min(kept.len().saturating_sub(1));
            tabs.list = kept;
        }
        self.tabs.retain(|_, tabs| !tabs.list.is_empty());

        // New terminals get a tab of their own, in their session.
        let tabbed: HashSet<usize> = self.tabs.values().flat_map(|t| t.list.iter().flat_map(Tab::terms)).collect();
        let new: Vec<(usize, PathBuf)> = self
            .terms
            .iter()
            .filter(|t| !tabbed.contains(&t.id))
            .map(|t| (t.id, self.session_of(t).map_or_else(|| current.clone(), |i| self.workspaces[i].root.clone())))
            .collect();
        for (id, root) in new {
            let tabs = self.tabs.entry(root).or_default();
            match tabs.list.get_mut(tabs.active) {
                // A file open on its own: the terminal goes above it.
                Some(tab) if tab.root == Node::Leaf(Pane::File) => {
                    tab.root = Node::Split { beside: false, ratio: 55, a: Box::new(Node::Leaf(Pane::Term(id))), b: Box::new(Node::Leaf(Pane::File)) };
                    tab.term = Some(id);
                }
                _ => tabs.list.push(Tab::new(Pane::Term(id))),
            }
        }

        // The file pane, under the terminal you were in, as the viewer
        // always was.
        if file_open {
            let tabs = self.tabs.entry(current).or_default();
            match tabs.list.get_mut(tabs.active) {
                Some(tab) if !tab.root.contains(Pane::File) => {
                    let at = tab.term.map_or(tab.focus, Pane::Term);
                    tab.root.split(at, Pane::File, false, 55);
                }
                Some(_) => {}
                None => {
                    tabs.list.push(Tab::new(Pane::File));
                    tabs.active = tabs.list.len() - 1;
                }
            }
        }
        self.active = self.tab().and_then(|t| t.term);
    }

    /// The tabs layout.json had, with the terminals the server still has.
    fn restore_tabs(&mut self) {
        self.tabs_restored = true;
        let ids: HashMap<String, usize> = self.terms.iter().map(|t| (t.info.tag.clone(), t.id)).collect();
        let id = |tag: &str| ids.get(tag).copied();
        for (root, saved) in &self.saved.tabs {
            let list: Vec<Tab> = saved
                .iter()
                .filter_map(|saved| {
                    let mut tab = Tab::new(Pane::File);
                    tab.root = saved.root.load(&id)?;
                    tab.name = saved.name.clone();
                    tab.fix();
                    Some(tab)
                })
                .collect();
            if !list.is_empty() {
                self.tabs.insert(root.clone(), Tabs { list, active: 0 });
            }
        }
    }

    /// Tab `n` (from 0) of the session on screen; with `enter`, or when
    /// you were on the right side, into the pane you were last in there.
    fn show_tab(&mut self, n: usize, enter: bool) {
        let root = self.ws().root.clone();
        let right = self.focused_pane().is_some();
        let Some(tabs) = self.tabs.get_mut(&root).filter(|t| n < t.list.len()) else { return };
        tabs.active = n;
        // The file pane comes along.
        self.sync_tabs();
        if let Some(id) = self.active
            && let Some(term) = self.terms.iter().find(|t| t.id == id)
        {
            self.last_term.insert(root, term.info.tag.clone());
        }
        if (enter || right)
            && let Some(pane) = self.tab().map(|t| t.focus)
        {
            self.focus_pane(pane);
        }
    }

    /// The next or previous tab, wrapping around.
    fn cycle_tab(&mut self, dir: isize) {
        let Some(tabs) = self.tabs().filter(|t| !t.list.is_empty()) else { return };
        let n = (tabs.active as isize + dir).rem_euclid(tabs.list.len() as isize) as usize;
        self.show_tab(n, false);
    }

    fn focus_pane(&mut self, pane: Pane) {
        match pane {
            Pane::Term(id) => {
                self.select_term(id);
                self.set_focus(Focus::Terminal);
            }
            Pane::File => self.set_focus(if self.editor.is_some() { Focus::Editor } else { Focus::Viewer }),
        }
    }

    /// A shell next to the pane you're in: beside it, or under it.
    fn split(&mut self, beside: bool) {
        let at = self.focused_pane().or_else(|| self.tab().map(|t| t.focus));
        let cwd = self.ws().root.clone();
        let token = self.spawn_in(cwd, &shell(), &[], true);
        if let (Some(at), Some(spawning)) = (at, self.spawning.get_mut(&token)) {
            spawning.1 = Some((at, beside));
        }
    }

    fn swap(&mut self, dir: Dir) {
        let Some(from) = self.focused_pane() else { return };
        let Some(to) = pane::neighbor(&self.areas.panes, from, dir) else { return };
        if let Some(tab) = self.tab_mut() {
            tab.root.swap(from, to);
        }
    }

    fn resize_pane(&mut self, dir: Dir) {
        let Some(at) = self.focused_pane() else { return };
        if let Some(tab) = self.tab_mut() {
            tab.root.resize(at, dir, 5);
        }
    }

    /// Close every terminal of the tab on screen once `key_name` is
    /// pressed again.
    fn close_tab(&mut self, pending: Option<char>, key_name: &str) {
        let Some(ids) = self.tab().map(Tab::terms) else { return };
        if ids.is_empty() {
            self.viewer.close();
            self.editor = None;
            return;
        }
        if pending != Some('k') {
            let running: Vec<String> = self.terms.iter().filter(|t| ids.contains(&t.id)).filter_map(|t| t.info.running.clone()).collect();
            let running = if running.is_empty() { String::new() } else { format!(", with {} running", running.join(", ")) };
            self.confirm('k', format!("press {key_name} again to close this tab: {} terminal(s){running}", ids.len()));
            return;
        }
        for id in ids {
            self.link.send(&ToServer::Close { id: id as u32 });
            self.remove_term(id);
        }
    }

    // Input

    /// Returns true when the app should quit.
    pub fn on_key(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(filter) = &mut self.help {
            match key.code {
                KeyCode::Esc => self.help = None,
                KeyCode::Char('?') if filter.is_empty() => self.help = None,
                KeyCode::Backspace => {
                    filter.pop();
                }
                KeyCode::Char('u') if ctrl => filter.clear(),
                KeyCode::Char(c) if !ctrl => filter.push(c),
                _ => {}
            }
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
        if self.picker.is_some() {
            return self.picker_key(key);
        }
        if let Some(name) = &mut self.renaming {
            match key.code {
                KeyCode::Esc => self.renaming = None,
                KeyCode::Enter => {
                    let name = self.renaming.take().unwrap_or_default().trim().to_string();
                    if let Some(tab) = self.tab_mut() {
                        tab.name = (!name.is_empty()).then_some(name);
                    }
                }
                KeyCode::Backspace => {
                    name.pop();
                }
                KeyCode::Char(c) if !ctrl => name.push(c),
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
        // The prefix, then the key that says what to do; or a chord.
        let k = Key::of(key);
        if self.prefix.take().is_some() {
            return self.after_prefix(key, k);
        }
        if self.keymap.is_prefix(k) {
            self.prefix = Some(Instant::now());
            // Hints show the prefix you use.
            self.keymap.prefix.retain(|&p| p != k);
            self.keymap.prefix.insert(0, k);
            return false;
        }
        if let Some(action) = self.keymap.chord(k) {
            let pending = self.pending.take();
            return self.act(action, pending);
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
            KeyCode::Char('?') => self.help = Some(String::new()),
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
                    layout::save(&Layout { open: Vec::new(), tabs: Default::default(), ..self.layout() });
                    return true;
                }
                let n = self.terms.len();
                self.confirm('Q', format!("press Q again to stop {n} terminal(s) and quit (ctrl-c first, then hang up)"));
            }
            KeyCode::Char('x') if self.focus == Focus::Sessions => self.delete_session(pending, "x"),
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
            KeyCode::Char(d @ '1'..='9') => self.show_tab_entering(d as usize - '1' as usize),
            KeyCode::Char('[') => self.cycle_tab(-1),
            KeyCode::Char(']') => self.cycle_tab(1),
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
        // The ctrl keys Pulpo used to take, until they go back to the
        // programs; each says once what replaces it.
        let legacy = ctrl && self.keymap.legacy;
        match key.code {
            KeyCode::Char('s') if legacy => {
                self.legacy_hint('s', &[Action::FocusLeft], "goes to the sidebar");
                self.toggle_terminal();
            }
            KeyCode::Char('f') if legacy => {
                self.legacy_hint('f', &[Action::Zoom], "zooms");
                self.zoomed = !self.zoomed;
            }
            KeyCode::Char('n') if legacy => {
                self.legacy_hint('n', &[Action::NewTab], "opens a shell");
                self.spawn(&shell(), true);
            }
            KeyCode::Char('q') if legacy => {
                self.legacy_hint('q', &[Action::ClosePane], "closes it");
                self.close_terminal('Q', "ctrl-q", pending);
            }
            KeyCode::Char(d @ '1'..='9') if legacy => {
                self.legacy_hint('1', &crate::keymap::TABS, "goes to a tab");
                self.show_tab_entering(d as usize - '1' as usize);
            }
            KeyCode::Char('[') if legacy => {
                self.legacy_hint('[', &[Action::PreviousTab, Action::NextTab], "switch tabs");
                self.cycle_tab(-1);
            }
            KeyCode::Char(']') if legacy => {
                self.legacy_hint('[', &[Action::PreviousTab, Action::NextTab], "switch tabs");
                self.cycle_tab(1);
            }
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

    /// Close the active terminal once the key is pressed again: x always,
    /// ctrl-q only if a program runs in it (Claude, a server…).
    fn close_terminal(&mut self, confirm_key: char, key_name: &str, pending: Option<char>) {
        let Some(term) = self.term() else { return };
        if pending == Some(confirm_key) {
            return self.close_active();
        }
        let label = term.label().to_string();
        match term.info.running.clone() {
            Some(program) => self.confirm(confirm_key, format!("{program} is running in {label}: press {key_name} again to close it")),
            None if confirm_key == 'x' => self.confirm('x', format!("press {key_name} again to close {label}")),
            None => self.close_active(),
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

    fn show_tab_entering(&mut self, n: usize) {
        self.show_tab(n, true);
    }

    fn close_active(&mut self) {
        let Some(id) = self.active else { return };
        if let Some(term) = self.term() {
            self.link.send(&ToServer::Close { id: term.info.id });
        }
        self.remove_term(id);
    }

    /// The key after the prefix: an action; the prefix again, which the
    /// program in the terminal gets; or else what the key does on its own.
    fn after_prefix(&mut self, key: KeyEvent, k: Key) -> bool {
        if k.code == KeyCode::Esc {
            return false;
        }
        if self.keymap.is_prefix(k) {
            if self.focus == Focus::Terminal
                && let Some(term) = self.term_mut()
            {
                term.key(key);
            }
            return false;
        }
        match self.keymap.after_prefix(k) {
            Some(action) => {
                let pending = self.pending.take();
                self.act(action, pending)
            }
            None => self.on_key(key),
        }
    }

    /// Do what a prefix key or a chord says. True to quit.
    fn act(&mut self, action: Action, pending: Option<char>) -> bool {
        self.message = None;
        let key_name = self.keymap.hint(&[action]);
        match action {
            Action::FocusLeft => self.focus_toward(Dir::Left),
            Action::FocusUp => self.focus_toward(Dir::Up),
            Action::FocusRight => self.focus_toward(Dir::Right),
            Action::FocusDown => self.focus_toward(Dir::Down),
            Action::SwapLeft => self.swap(Dir::Left),
            Action::SwapUp => self.swap(Dir::Up),
            Action::SwapRight => self.swap(Dir::Right),
            Action::SwapDown => self.swap(Dir::Down),
            Action::ResizeLeft => self.resize_pane(Dir::Left),
            Action::ResizeUp => self.resize_pane(Dir::Up),
            Action::ResizeRight => self.resize_pane(Dir::Right),
            Action::ResizeDown => self.resize_pane(Dir::Down),
            Action::SplitBeside => self.split(true),
            Action::SplitBelow => self.split(false),
            Action::Zoom => self.zoom(),
            Action::ClosePane if self.focus == Focus::Editor => self.close_editor(pending),
            Action::ClosePane if self.focus == Focus::Viewer => {
                self.viewer.close();
                self.set_focus(self.sidebar_focus);
            }
            Action::ClosePane => self.close_terminal('x', &key_name, pending),
            Action::CloseTab => self.close_tab(pending, &key_name),
            Action::RenameTab if self.tab().is_some() => self.renaming = Some(self.tab().and_then(|t| t.name.clone()).unwrap_or_default()),
            Action::RenameTab => {}
            Action::NewTab => self.spawn(&shell(), true),
            Action::Tab(n) => self.show_tab_entering(n as usize - 1),
            Action::NextTab => self.cycle_tab(1),
            Action::PreviousTab => self.cycle_tab(-1),
            Action::NewSession => self.creating = Some(Create { dir: self.ws().repo.clone(), name: String::new(), session: true }),
            Action::Sessions => self.set_focus(Focus::Sessions),
            Action::NextSession => self.step_session(1),
            Action::PreviousSession => self.step_session(-1),
            Action::DeleteSession => self.delete_session(pending, &key_name),
            Action::AddWorkspace => self.open_repo_picker(),
            Action::Detach if self.editor.as_ref().is_some_and(|e| e.dirty) => {
                self.message = Some("unsaved changes in the editor: ctrl-s saves them, esc there discards them".into());
            }
            // Terminals keep running in the server, as with q.
            Action::Detach => return true,
            Action::Goto => self.open_goto(Show::All),
            Action::NextWaiting => self.next_waiting(),
            Action::LastPlace => self.last_place(),
            Action::Menu => self.open_menu(),
            Action::Theme => self.open_themes(),
            Action::Files => self.open_search(Mode::Files),
            Action::SearchFiles => self.open_search(Mode::Text),
            Action::Sidebar => {
                self.sidebar_hidden = !self.sidebar_hidden;
                if self.sidebar_hidden && self.focused_pane().is_none() {
                    match self.tab().map(|t| t.focus) {
                        Some(pane) => self.focus_pane(pane),
                        None => self.sidebar_hidden = false,
                    }
                }
            }
            Action::Help => self.help = Some(String::new()),
        }
        false
    }

    // Go to

    /// Every terminal, by repository and session, with what its agent asks
    /// or does; on the one you're in.
    fn open_goto(&mut self, show: Show) {
        let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
        let short = |path: &Path| match path.strip_prefix(&home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => path.display().to_string(),
        };
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
        let mut entries = Vec::new();
        for main in self.repos() {
            let repo = &self.workspaces[main];
            let group = format!("{}  {}", repo.name, short(&repo.repo));
            for (s, ws) in self.workspaces.iter().enumerate().filter(|(_, w)| w.repo == repo.repo) {
                // In tab order, then any not in a tab yet.
                let tabs = self.tabs.get(&ws.root);
                let mut ids: Vec<usize> = tabs.map(|t| t.list.iter().flat_map(Tab::terms).collect()).unwrap_or_default();
                let untabbed: Vec<usize> = self.terms.iter().filter(|t| self.session_of(t) == Some(s) && !ids.contains(&t.id)).map(|t| t.id).collect();
                ids.extend(untabbed);
                for id in ids {
                    let Some(term) = self.terms.iter().find(|t| t.id == id) else { continue };
                    let state = self.term_state(term);
                    let agent = self.agents.for_tab(&term.info.tag);
                    let said = agent.map(|a| a.detail.clone()).filter(|d| !d.is_empty());
                    let detail = match state {
                        State::Waiting => said.unwrap_or_else(|| "waiting for you".into()),
                        State::Done => said.map_or_else(|| "finished".into(), |d| format!("finished: {d}")),
                        State::Running => said.unwrap_or_else(|| "working".into()),
                        State::Idle => String::new(),
                    };
                    let age = agent.filter(|_| state != State::Idle).map(|a| picker::age(now.saturating_sub(a.ts))).unwrap_or_default();
                    let tab = tabs.and_then(|t| t.list.iter().position(|tab| tab.root.contains(Pane::Term(id))));
                    let place = match tab {
                        Some(n) => format!("{} · tab {}", self.session_name(s), n + 1),
                        None => self.session_name(s).to_string(),
                    };
                    let mut path = short(&term.info.cwd);
                    if let Some(branch) = &ws.status.branch {
                        path.push_str(&format!("  ·  branch {branch}"));
                    }
                    entries.push(Entry { group: group.clone(), text: term.label().to_string(), place, detail, age, state: Some(state), path, target: Target::Term(id) });
                }
            }
        }
        self.picker = Some(Picker::new(Kind::Goto, entries, show, self.active.map(Target::Term)));
    }

    /// Every action by name, with its key.
    fn open_menu(&mut self) {
        let entries = crate::keymap::MENU
            .iter()
            .map(|&(group, action, name)| Entry {
                group: group.title().to_string(),
                text: name.to_string(),
                place: self.keymap.hint(&[action]),
                detail: String::new(),
                age: String::new(),
                state: None,
                path: String::new(),
                target: Target::Action(action),
            })
            .collect();
        self.picker = Some(Picker::new(Kind::Menu, entries, Show::All, None));
    }

    /// Following Ghostty, Linear Dark, Omarchy's themes as Ghostty has
    /// them, yours, then every theme Ghostty comes with.
    fn open_themes(&mut self) {
        let following = crate::theme::ghostty_theme().unwrap_or_else(|| "none set, so Linear Dark".into());
        let mut groups: Vec<(&str, Vec<(String, String)>)> = vec![(
            "PULPO",
            vec![
                (crate::theme::FOLLOW.into(), format!("Follow Ghostty  (now {following})")),
                (crate::theme::LINEAR.into(), crate::theme::LINEAR.into()),
            ],
        )];
        let bundled = crate::theme::ghostty_themes();
        let omarchy = crate::theme::OMARCHY.iter().filter(|t| bundled.iter().any(|b| b == *t)).map(|t| (t.to_string(), t.to_string()));
        groups.push(("OMARCHY'S", omarchy.collect()));
        groups.push(("YOURS", crate::theme::user_themes().into_iter().map(|t| (t.clone(), t)).collect()));
        groups.push(("GHOSTTY'S", bundled.into_iter().map(|t| (t.clone(), t)).collect()));
        self.theme_before = crate::theme::chosen();
        self.themes.clear();
        let mut entries = Vec::new();
        let mut on = None;
        for (group, themes) in groups {
            for (name, text) in themes {
                let target = Target::Theme(self.themes.len());
                if name == self.theme_before && on.is_none() {
                    on = Some(target);
                }
                let place = if name == self.theme_before { "on".to_string() } else { String::new() };
                self.themes.push(name);
                entries.push(Entry { group: group.into(), text, place, detail: String::new(), age: String::new(), state: None, path: String::new(), target });
            }
        }
        self.picker = Some(Picker::new(Kind::Theme, entries, Show::All, on));
    }

    /// Put a theme on screen: the panels, the terminals, and the file in
    /// the viewer or the editor, colored again.
    fn show_theme(&mut self, name: &str) {
        if let Err(e) = crate::theme::apply(name) {
            self.message = Some(e);
            return;
        }
        let root = self.ws().root.clone();
        self.viewer.reload(&root);
        if let Some(editor) = &mut self.editor {
            editor.recolor();
        }
    }

    /// Keys in a picker: type to filter, tab for which terminals, ⏎ to go.
    /// Moving through the themes shows each one.
    fn picker_key(&mut self, key: KeyEvent) -> bool {
        let quit = self.picker_input(key);
        if let Some(picker) = &self.picker
            && picker.kind == Kind::Theme
            && let Some(Target::Theme(i)) = picker.selected().map(|e| e.target)
            && let Some(name) = self.themes.get(i).cloned()
        {
            self.show_theme(&name);
        }
        quit
    }

    fn picker_input(&mut self, key: KeyEvent) -> bool {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let Some(picker) = &mut self.picker else { return false };
        match key.code {
            KeyCode::Esc if picker.kind == Kind::Theme => {
                self.picker = None;
                let before = self.theme_before.clone();
                self.show_theme(&before);
            }
            KeyCode::Esc => self.picker = None,
            KeyCode::Enter => {
                let target = picker.selected().map(|e| e.target);
                self.picker = None;
                match target {
                    Some(Target::Term(id)) => self.go_to_term(id),
                    Some(Target::Action(action)) => return self.act(action, None),
                    Some(Target::Theme(i)) => {
                        let name = self.themes.get(i).cloned().unwrap_or_default();
                        self.show_theme(&name);
                        self.message = Some(match crate::theme::choose(&name) {
                            Ok(path) => format!("theme: {name} (kept in {})", path.display()),
                            Err(e) => format!("could not keep the theme: {e}"),
                        });
                    }
                    None => {}
                }
            }
            KeyCode::Up => picker.step(-1),
            KeyCode::Down => picker.step(1),
            KeyCode::Char('k' | 'p') if ctrl => picker.step(-1),
            KeyCode::Char('j' | 'n') if ctrl => picker.step(1),
            KeyCode::PageUp => picker.step(-10),
            KeyCode::PageDown => picker.step(10),
            KeyCode::Tab if picker.kind == Kind::Goto => picker.next_show(1),
            KeyCode::BackTab if picker.kind == Kind::Goto => picker.next_show(-1),
            KeyCode::Backspace => {
                picker.query.pop();
                picker.update();
            }
            KeyCode::Char('u') if ctrl => {
                picker.query.clear();
                picker.update();
            }
            KeyCode::Char(c) if !ctrl => {
                picker.query.push(c);
                picker.update();
            }
            _ => {}
        }
        false
    }

    /// Into a terminal, wherever it is: its session, its tab, its pane.
    fn go_to_term(&mut self, id: usize) {
        let Some(term) = self.terms.iter().find(|t| t.id == id) else { return };
        if let Some(i) = self.session_of(term) {
            self.set_current(i);
        }
        self.select_term(id);
        self.set_focus(Focus::Terminal);
    }

    /// Into the terminal that has waited longest with something to read:
    /// those that ask before those that finished. It's read once you're
    /// in, so again goes to the next one.
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
        match waiting.first() {
            Some(&(.., id)) => self.go_to_term(id),
            None => self.message = Some("no terminal has anything to read".into()),
        }
    }

    /// The tab or session you were on before this one.
    fn last_place(&mut self) {
        let Some((root, term)) = self.back.clone() else {
            self.message = Some("nowhere to go back to yet".into());
            return;
        };
        match term.filter(|id| self.terms.iter().any(|t| t.id == *id)) {
            Some(id) => self.go_to_term(id),
            None => {
                if let Some(i) = self.workspaces.iter().position(|w| w.root == root) {
                    self.set_current(i);
                }
            }
        }
    }

    /// Go to the pane that way. The sidebar is left of every pane, its
    /// lists one above the other.
    fn focus_toward(&mut self, dir: Dir) {
        const SIDEBAR: [Focus; 4] = [Focus::Workspaces, Focus::Sessions, Focus::Changes, Focus::Graph];
        if let Some(i) = SIDEBAR.iter().position(|&f| f == self.focus) {
            match dir {
                Dir::Up if i > 0 => self.set_focus(SIDEBAR[i - 1]),
                Dir::Down if i + 1 < SIDEBAR.len() => self.set_focus(SIDEBAR[i + 1]),
                Dir::Right => match self.tab().map(|t| t.focus) {
                    Some(pane) => self.focus_pane(pane),
                    None => self.spawn(&shell(), true),
                },
                _ => {}
            }
            return;
        }
        let Some(from) = self.focused_pane() else { return };
        match pane::neighbor(&self.areas.panes, from, dir) {
            Some(pane) => self.focus_pane(pane),
            None if dir == Dir::Left => self.set_focus(self.sidebar_focus),
            None => {}
        }
    }

    /// The focused terminal or file alone, or back; from the sidebar, into
    /// the terminal, full height.
    fn zoom(&mut self) {
        match self.focus {
            Focus::Terminal => self.zoomed = !self.zoomed,
            Focus::Viewer => self.viewer.expanded = !self.viewer.expanded,
            _ => self.zoom_terminal(),
        }
    }

    /// The previous or next session of this repository, wrapping around.
    fn step_session(&mut self, delta: isize) {
        let sessions = self.sessions();
        if sessions.len() < 2 {
            return;
        }
        let at = sessions.iter().position(|&i| i == self.current).unwrap_or(0) as isize;
        let next = (at + delta).rem_euclid(sessions.len() as isize) as usize;
        self.sessions_state.select(Some(next));
        self.set_current(sessions[next]);
        // No terminal there to be in.
        if self.focus == Focus::Terminal && self.active.is_none() {
            self.set_focus(Focus::Sessions);
        }
    }

    /// Delete the current session once `key_name` is pressed again; only a
    /// task session can be.
    fn delete_session(&mut self, pending: Option<char>, key_name: &str) {
        if self.isolated_current().is_some() {
            self.remove_session(pending, key_name);
        } else if self.ws().is_worktree() {
            self.message = Some("this session is a git worktree; Pulpo leaves worktrees to git".into());
        } else {
            self.message = Some("this is the repository itself; x in Workspaces takes it off the list".into());
        }
    }

    /// Say once what replaces one of the old ctrl keys in terminals.
    fn legacy_hint(&mut self, c: char, actions: &[Action], what: &str) {
        if self.legacy_hinted.insert(c) {
            let instead = self.keymap.hint(actions);
            self.message = Some(format!("ctrl-{c} goes back to the programs in the next release: {instead} {what}"));
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        // A click or scroll can change what a second x would act on.
        if matches!(mouse.kind, MouseEventKind::Down(_) | MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
            self.pending = None;
        }
        if (self.help.is_some() || self.new_menu || self.picker.is_some()) && matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.help = None;
            self.new_menu = false;
            self.picker = None;
        }
        if self.search.is_some() || self.creating.is_some() || self.renaming.is_some() || self.picker.is_some() || self.help.is_some() || self.new_menu {
            return;
        }
        let pos = Position::new(mouse.column, mouse.row);
        let areas = self.areas.clone();
        if let Some(&(show, _)) = areas.counts.iter().find(|(_, r)| r.contains(pos)) {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.open_goto(show);
            }
            return;
        }
        if let Some(&(n, _)) = areas.tabs.iter().find(|(_, r)| r.contains(pos)) {
            if matches!(mouse.kind, MouseEventKind::Down(MouseButton::Left)) {
                self.show_tab(n, true);
            }
            return;
        }
        // A terminal pane: a program that asked for the mouse gets it
        // inside its text; else a click goes into it, the wheel scrolls it.
        if let Some(&(Pane::Term(id), rect)) = areas.panes.iter().find(|(p, r)| matches!(p, Pane::Term(_)) && r.contains(pos)) {
            let inner = Block::bordered().inner(rect);
            let Some(term) = self.terms.iter_mut().find(|t| t.id == id) else { return };
            if inner.contains(pos) && term.wants_mouse() {
                term.mouse(mouse.kind, pos.x - inner.x + 1, pos.y - inner.y + 1, mouse.modifiers);
            } else if matches!(mouse.kind, MouseEventKind::ScrollUp | MouseEventKind::ScrollDown) {
                scroll_term(term, if mouse.kind == MouseEventKind::ScrollUp { -3 } else { 3 });
            }
            if matches!(mouse.kind, MouseEventKind::Down(_)) {
                self.focus_pane(Pane::Term(id));
            }
            return;
        }
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
            (Focus::Viewer, areas.viewer),
            (Focus::Workspaces, areas.workspaces),
            (Focus::Sessions, areas.sessions),
            (Focus::Changes, areas.changes),
            (Focus::Graph, areas.graph),
        ]
        .into_iter()
        .find(|(_, area)| area.contains(pos))
        .unwrap_or((self.focus, Rect::default()));

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
        if !matches!(focus, Focus::Viewer | Focus::Editor) {
            self.viewer.expanded = false;
        }
        if matches!(focus, Focus::Workspaces | Focus::Sessions | Focus::Changes | Focus::Graph) {
            self.sidebar_focus = focus;
            self.sidebar_hidden = false;
        }
        if let Some(pane) = self.focused_pane()
            && let Some(tab) = self.tab_mut()
            && tab.root.contains(pane)
        {
            tab.set_focus(pane);
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

    /// x x on an isolated session: stop its terminals, take it off the list,
    /// and in the background remove its containers and volumes (its
    /// database) and the copy. The first x says what would be lost.
    fn remove_session(&mut self, pending: Option<char>, key_name: &str) {
        let Some(session) = self.isolated_current().cloned() else { return };
        if pending != Some('x') || self.deleting.as_ref() != Some(&session.root) {
            self.deleting = Some(session.root.clone());
            let lost = session::unsaved(&session);
            let lost = if lost.is_empty() { String::new() } else { format!(", losing {}", lost.join(" and ")) };
            self.confirm('x', format!("press {key_name} again to delete session {}: its copy, containers and database{lost}", session.name));
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
        // In the background, from main, with the session's Compose project
        // and ports; its own process group, so it outlives Pulpo.
        let log = crate::agents::state_dir().join(format!("remove-{}.log", session.project));
        let started = std::fs::File::create(&log).and_then(|out| {
            use std::os::unix::process::CommandExt;
            std::process::Command::new("sh")
                .args(["-c", &script])
                .current_dir(&session.repo)
                .envs(session::env(&session))
                .stdin(std::process::Stdio::null())
                .stdout(out.try_clone()?)
                .stderr(out)
                .process_group(0)
                .spawn()
        });
        let mut child = match started {
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
                self.viewer.expanded = true;
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
