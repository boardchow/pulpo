use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use ratatui::widgets::{Block, ListState};

use crate::Msg;
use crate::agents::{Agents, State};
use crate::git;
use crate::search::{Mode, Search};
use crate::term::Term;
use crate::viewer::{Source, Viewer};
use crate::workspace::{self, GraphItem, Workspace};

#[derive(Clone, Copy, PartialEq)]
pub enum Focus {
    Workspaces,
    Sessions,
    Changes,
    Graph,
    Files,
    Viewer,
    Terminal,
}

/// Where each panel was drawn, for mouse hit-testing.
#[derive(Default, Clone, Copy)]
pub struct Areas {
    pub workspaces: Rect,
    pub sessions: Rect,
    pub changes: Rect,
    pub graph: Rect,
    pub files: Rect,
    pub terminal: Rect,
    pub viewer: Rect,
    /// The viewer's overview ruler.
    pub ruler: Rect,
}

pub struct App {
    pub workspaces: Vec<Workspace>,
    pub current: usize,
    pub workspaces_state: ListState,
    /// Shown when no repository is registered yet.
    scratch: PathBuf,
    pub focus: Focus,
    /// The sidebar list to return to when leaving the terminal or viewer.
    sidebar_focus: Focus,
    pub agents: Agents,
    pub terms: Vec<Term>,
    pub active: usize,
    next_id: usize,
    /// Rows and columns available to terminals, set when drawing.
    term_size: (u16, u16),
    pub viewer: Viewer,
    pub sessions_state: ListState,
    pub areas: Areas,
    pub message: Option<String>,
    pub search: Option<Search>,
    /// The full list of shortcuts is shown.
    pub help: bool,
    /// n was pressed: w or s next says what to make.
    pub new_menu: bool,
    /// The terminal takes the whole right side; the viewer comes back when
    /// something is previewed.
    pub zoomed: bool,
    /// Terminal tab labels in the tab bar: (first column, last column, tab).
    pub tab_hits: Vec<(u16, u16, usize)>,
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
    pub fn new(launch: Option<PathBuf>, scratch: PathBuf, tx: Sender<Msg>) -> anyhow::Result<Self> {
        let fs_tx = tx.clone();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                let _ = fs_tx.send(Msg::Fs(event.paths));
            }
        })?;
        let mut agents = Agents::default();
        agents.load();
        let mut app = App {
            workspaces: Vec::new(),
            current: 0,
            workspaces_state: ListState::default().with_selected(Some(0)),
            scratch,
            focus: Focus::Files,
            sidebar_focus: Focus::Files,
            agents,
            terms: Vec::new(),
            active: 0,
            next_id: 0,
            term_size: (24, 80),
            viewer: Viewer::default(),
            sessions_state: ListState::default().with_selected(Some(0)),
            areas: Areas::default(),
            message: None,
            search: None,
            help: false,
            new_menu: false,
            zoomed: false,
            tab_hits: Vec::new(),
            pending: None,
            fs_dirty: HashSet::new(),
            reload_list: false,
            watcher,
            watched: HashSet::new(),
            tx,
        };
        app.load_workspaces();
        if let Some(i) = launch.and_then(|l| app.workspaces.iter().position(|w| w.root == l)) {
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
        for repo in workspace::registered().into_iter().filter(|r| r.is_dir()) {
            let mut roots = git::worktrees(&repo);
            if roots.is_empty() {
                roots.push(repo.clone());
            }
            for root in roots {
                self.workspaces.push(reuse(root, &repo));
            }
        }
        if self.workspaces.is_empty() {
            let scratch = self.scratch.clone();
            self.workspaces.push(reuse(scratch.clone(), &scratch));
        }
        self.current = current.and_then(|c| self.workspaces.iter().position(|w| w.root == c)).unwrap_or(0);
        self.workspaces_state.select(Some(self.current));
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

    fn set_current(&mut self, i: usize) {
        if i >= self.workspaces.len() {
            return;
        }
        self.workspaces_state.select(Some(i));
        if i != self.current {
            self.current = i;
            self.viewer.close();
            self.workspaces[i].refresh();
        }
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
        let cwd = self.ws().root.clone();
        match Term::spawn(self.next_id, program, &cwd, self.term_size, self.tx.clone()) {
            Ok(term) => {
                self.next_id += 1;
                self.terms.push(term);
                self.select_term(self.terms.len() - 1);
                if focus {
                    self.set_focus(Focus::Terminal);
                }
            }
            Err(e) => self.message = Some(format!("could not start {program}: {e}")),
        }
    }

    pub fn on_pty(&mut self, id: usize, bytes: &[u8]) {
        if let Some(term) = self.terms.iter_mut().find(|t| t.id == id) {
            term.feed(bytes);
        }
    }

    pub fn on_exit(&mut self, id: usize) {
        if let Some(i) = self.terms.iter().position(|t| t.id == id) {
            self.terms.remove(i);
            self.after_close();
        }
    }

    pub fn on_paste(&mut self, text: &str) {
        if let Some(search) = &mut self.search {
            search.query.push_str(text.trim());
            search.update(&self.tx);
        } else if self.viewer.find.as_ref().is_some_and(|f| f.typing) {
            self.viewer.edit_find(|q| q.push_str(text.trim()));
        } else if self.focus == Focus::Terminal
            && let Some(term) = self.terms.get_mut(self.active)
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
    /// it, waiting with something to read (what it asks, with hooks).
    pub fn term_state(&self, term: &Term) -> (State, String) {
        let agent = self.agents.for_tab(&term.tag);
        let detail = |state: State| agent.filter(|a| a.state == state).map(|a| a.detail.clone()).unwrap_or_default();
        if term.working() {
            (State::Running, detail(State::Running))
        } else if term.unread {
            (State::Waiting, detail(State::Waiting))
        } else {
            (State::Idle, String::new())
        }
    }

    /// Before each draw: runs that ended and agents that stopped or asked
    /// leave something to read, unless you are in that terminal.
    pub fn tick(&mut self) {
        let viewing = (self.focus == Focus::Terminal).then_some(self.active);
        for (i, term) in self.terms.iter_mut().enumerate() {
            term.settle();
            let reported = self.agents.for_tab(&term.tag).map_or(State::Idle, |a| a.state);
            if reported != term.reported {
                term.unread |= matches!(reported, State::Waiting | State::Done);
                term.reported = reported;
            }
            if viewing == Some(i) {
                term.unread = false;
            }
        }
    }

    /// Something is working, so its spinner needs frequent redraws.
    pub fn animating(&self) -> bool {
        self.terms.iter().any(|term| self.term_state(term).0 == State::Running)
    }

    pub fn workspace_name(&self, dir: &Path) -> Option<&str> {
        self.workspaces.iter().find(|w| w.root == dir).map(|w| w.name.as_str())
    }

    fn select_term(&mut self, i: usize) {
        self.active = i;
        self.sessions_state.select(Some(i));
    }

    fn after_close(&mut self) {
        self.select_term(self.active.min(self.terms.len().saturating_sub(1)));
        if self.terms.is_empty() && self.focus == Focus::Terminal {
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
                KeyCode::Char('s') => self.spawn(&shell(), true),
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

        self.message = None;
        match key.code {
            // Panels, by their initial.
            KeyCode::Char('w') => self.set_focus(Focus::Workspaces),
            KeyCode::Char('t') => self.set_focus(Focus::Sessions),
            KeyCode::Char('c') => self.set_focus(Focus::Changes),
            // In the viewer, g goes to the top instead.
            KeyCode::Char('g') if self.focus != Focus::Viewer => self.set_focus(Focus::Graph),
            KeyCode::Char('f') if !ctrl => self.set_focus(Focus::Files),
            KeyCode::Char('v') if self.viewer.is_open() => self.set_focus(Focus::Viewer),
            KeyCode::Char('s') if ctrl || !self.terms.is_empty() => self.toggle_terminal(),
            KeyCode::Char('s') => self.spawn(&shell(), true),
            KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => self.cycle_focus(-1),
            KeyCode::Tab => self.cycle_focus(1),
            KeyCode::BackTab => self.cycle_focus(-1),

            KeyCode::Char('/') if self.focus == Focus::Viewer => self.viewer.start_find(),
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
            // x acts on the panel: workspaces there, terminals elsewhere.
            // File names first; tab in the popup searches inside files.
            KeyCode::Char('/') => self.open_search(Mode::Files),
            // ctrl-f, or z for zoom: the current terminal, full height.
            KeyCode::Char('f' | 'z') => self.zoom_terminal(),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('q') => {
                if self.terms.is_empty() || pending == Some('q') {
                    return true;
                }
                self.confirm('q', format!("{} terminal(s) open: press q again to quit Pulpo", self.terms.len()));
            }
            KeyCode::Char('x') if self.focus == Focus::Workspaces && self.ws().is_worktree() => {
                self.message = Some("this is a git worktree of the workspace above; Pulpo leaves worktrees to git".into());
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
            KeyCode::Char(d @ '1'..='9') => self.enter_term(d as usize - '1' as usize),
            KeyCode::Char(c @ ('[' | ']')) if !self.terms.is_empty() => {
                let n = self.terms.len();
                self.select_term(if c == ']' { (self.active + 1) % n } else { (self.active + n - 1) % n });
                self.follow_term();
            }
            KeyCode::Esc if self.focus == Focus::Viewer && self.viewer.find.is_some() => self.viewer.clear_find(),
            KeyCode::Esc if self.focus == Focus::Viewer => self.set_focus(self.sidebar_focus),
            KeyCode::Esc => self.viewer.close(),
            _ if self.focus == Focus::Viewer => self.viewer_key(key),
            _ => self.list_key(key),
        }
        false
    }

    /// In a terminal every key goes to the program, except a few ctrl keys.
    fn terminal_key(&mut self, key: KeyEvent, pending: Option<char>) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if pending.is_some() {
            self.message = None;
        }
        match key.code {
            KeyCode::Char('s') if ctrl => self.toggle_terminal(),
            KeyCode::Char('f') if ctrl => self.zoomed = !self.zoomed,
            // The new shell starts in the workspace of the one you are in.
            KeyCode::Char('n') if ctrl => {
                self.follow_term();
                self.spawn(&shell(), true);
            }
            KeyCode::Char('q') if ctrl => self.close_terminal('Q', "ctrl-q", pending),
            KeyCode::Char(d @ '1'..='9') if ctrl => self.enter_term(d as usize - '1' as usize),
            _ => {
                if let Some(term) = self.terms.get_mut(self.active) {
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
        if self.terms.is_empty() {
            self.spawn(&shell(), true);
        } else {
            self.enter_term(self.active);
        }
        self.zoomed = self.focus == Focus::Terminal;
    }

    /// Close the active terminal right away if its shell is idle; if a
    /// program runs in it (Claude, a server…), ask for the key again first.
    fn close_terminal(&mut self, confirm_key: char, key_name: &str, pending: Option<char>) {
        let Some(term) = self.terms.get_mut(self.active) else { return };
        term.refresh_foreground();
        match term.running.clone() {
            Some(program) if pending != Some(confirm_key) => {
                let n = self.active + 1;
                self.confirm(confirm_key, format!("{program} is running in terminal {n}: press {key_name} again to close it"));
            }
            _ => self.close_active(),
        }
    }

    /// Tab names follow what runs in each terminal.
    pub fn refresh_foreground(&mut self) {
        for term in &mut self.terms {
            term.refresh_foreground();
        }
    }

    fn enter_term(&mut self, i: usize) {
        if i < self.terms.len() {
            self.select_term(i);
            self.follow_term();
            self.set_focus(Focus::Terminal);
        }
    }

    fn close_active(&mut self) {
        if self.active < self.terms.len() {
            self.terms.remove(self.active);
            self.after_close();
        }
    }

    pub fn on_mouse(&mut self, mouse: MouseEvent) {
        if (self.help || self.new_menu) && matches!(mouse.kind, MouseEventKind::Down(_)) {
            self.help = false;
            self.new_menu = false;
        }
        if self.search.is_some() || self.help || self.new_menu {
            return;
        }
        let pos = Position::new(mouse.column, mouse.row);
        let areas = self.areas;
        let (target, area) = [
            (Focus::Terminal, areas.terminal),
            (Focus::Viewer, areas.viewer),
            (Focus::Workspaces, areas.workspaces),
            (Focus::Sessions, areas.sessions),
            (Focus::Changes, areas.changes),
            (Focus::Graph, areas.graph),
            (Focus::Files, areas.files),
        ]
        .into_iter()
        .find(|(_, area)| area.contains(pos))
        .unwrap_or((self.focus, Rect::default()));

        // Inside the terminal's text (not its border or tab bar), a program
        // that asked for the mouse gets the event itself.
        let inner = Block::bordered().inner(area);
        if target == Focus::Terminal
            && inner.contains(pos)
            && let Some(term) = self.terms.get_mut(self.active)
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
                if area.is_empty() || (target == Focus::Terminal && self.terms.is_empty()) {
                    return;
                }
                // A click on a tab label switches terminal.
                if target == Focus::Terminal
                    && pos.y == area.y
                    && let Some(&(_, _, i)) = self.tab_hits.iter().find(|(a, b, _)| (*a..=*b).contains(&pos.x))
                {
                    self.select_term(i);
                    self.follow_term();
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
                    Focus::Sessions => self.set_focus(Focus::Terminal),
                    Focus::Files => {
                        if let Some(row) = self.ws().selected_row().filter(|r| r.is_dir) {
                            let (path, open) = (row.path.clone(), !row.expanded);
                            self.ws_mut().tree.set_expanded(&path, open);
                            self.ws_mut().reselect(Some(path));
                        }
                    }
                    Focus::Graph => self.toggle_commit(),
                    _ => {}
                }
            }
            MouseEventKind::ScrollUp | MouseEventKind::ScrollDown => {
                let rows: isize = if mouse.kind == MouseEventKind::ScrollUp { -3 } else { 3 };
                match target {
                    Focus::Terminal => {
                        if let Some(term) = self.terms.get_mut(self.active) {
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
        let Some(search) = &mut self.search else { return };
        match key.code {
            KeyCode::Esc => self.search = None,
            KeyCode::Enter => self.open_hit(),
            KeyCode::Up => search.step(-1),
            KeyCode::Down => search.step(1),
            KeyCode::Char('k' | 'p') if ctrl => search.step(-1),
            KeyCode::Char('j' | 'n') if ctrl => search.step(1),
            KeyCode::PageUp => search.step(-10),
            KeyCode::PageDown => search.step(10),
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
            self.viewer.open_at(search.root.join(&hit.rel), hit.line, &search.root);
            self.read_in_viewer();
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
        } else if !self.terms.is_empty() {
            self.select_term(self.active);
            self.set_focus(Focus::Terminal);
        }
    }

    fn set_focus(&mut self, focus: Focus) {
        self.focus = focus;
        if focus == Focus::Viewer {
            self.zoomed = false;
        }
        if focus != Focus::Viewer {
            self.viewer.expanded = false;
        }
        if matches!(focus, Focus::Workspaces | Focus::Sessions | Focus::Changes | Focus::Graph | Focus::Files) {
            self.sidebar_focus = focus;
        }
    }

    fn cycle_focus(&mut self, dir: isize) {
        let mut order = vec![Focus::Workspaces, Focus::Sessions, Focus::Changes, Focus::Graph, Focus::Files];
        if self.viewer.is_open() {
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
        let count = self.workspaces.len();
        let w = &mut self.workspaces[self.current];
        match focus {
            Focus::Workspaces => Some((&mut self.workspaces_state, count)),
            Focus::Sessions => Some((&mut self.sessions_state, self.terms.len())),
            Focus::Changes => Some((&mut w.changes_state, w.status.changes.len())),
            Focus::Graph => {
                let len = w.graph_len();
                Some((&mut w.graph_state, len))
            }
            Focus::Files => Some((&mut w.files_state, w.tree.rows.len())),
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

    /// Follow the selection: switch workspace or tab, show the diff, or
    /// preview the file.
    fn on_select(&mut self) {
        match self.focus {
            Focus::Workspaces => {
                if let Some(i) = self.workspaces_state.selected() {
                    self.set_current(i);
                    // Bring up a terminal working in this workspace, if any.
                    let root = &self.ws().root;
                    if self.terms.get(self.active).is_some_and(|t| &t.cwd != root)
                        && let Some(t) = self.terms.iter().position(|t| &t.cwd == root)
                    {
                        self.select_term(t);
                    }
                }
            }
            Focus::Sessions => {
                if let Some(i) = self.sessions_state.selected().filter(|&i| i < self.terms.len()) {
                    self.select_term(i);
                    self.follow_term();
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
            Focus::Files => {
                if let Some(path) = self.ws().selected_row().filter(|r| !r.is_dir).map(|r| r.path.clone()) {
                    let root = self.ws().root.clone();
                    self.viewer.open(Source::File(path), &root);
                    self.zoomed = false;
                }
            }
            _ => {}
        }
    }

    /// Show the workspace the active terminal works in.
    fn follow_term(&mut self) {
        let cwd = self.terms.get(self.active).map(|t| t.cwd.clone());
        if let Some(i) = cwd.and_then(|cwd| self.workspaces.iter().position(|w| w.root == cwd)) {
            self.set_current(i);
        }
    }

    fn activate(&mut self) {
        match self.focus {
            Focus::Workspaces => self.set_focus(Focus::Files),
            Focus::Sessions if !self.terms.is_empty() => self.set_focus(Focus::Terminal),
            Focus::Files if self.ws().selected_row().is_some_and(|r| r.is_dir) => {
                let expanded = self.ws().selected_row().is_some_and(|r| r.expanded);
                self.expand(!expanded);
            }
            Focus::Graph if matches!(self.ws().selected_graph_item(), Some(GraphItem::Row(_))) => self.toggle_commit(),
            Focus::Changes | Focus::Graph | Focus::Files => {
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

    fn expand(&mut self, open: bool) {
        if self.focus == Focus::Graph {
            let ws = self.ws();
            let Some(hash) = ws.selected_commit().map(|c| c.hash.clone()) else { return };
            match ws.selected_graph_item() {
                // On a file, h goes back to its commit (and h again folds it).
                Some(GraphItem::File(..)) if !open => self.ws_mut().set_commit_open(&hash, true),
                Some(GraphItem::Row(_)) if ws.is_commit_open(&hash) != open => self.ws_mut().set_commit_open(&hash, open),
                _ => return,
            }
            self.on_select();
            return;
        }
        if self.focus != Focus::Files {
            return;
        }
        let w = self.ws_mut();
        let Some(row) = w.selected_row() else { return };
        let path = row.path.clone();
        if row.is_dir && row.expanded != open {
            w.tree.set_expanded(&path, open);
            w.reselect(Some(path));
        } else if !open && path.parent() != Some(&w.root) {
            // Already collapsed (or a file): jump to the parent directory.
            w.reselect(path.parent().map(Path::to_path_buf));
        }
    }
}

pub fn shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
}
