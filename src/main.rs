mod agents;
mod app;
mod client;
mod doctor;
mod editor;
mod git;
mod input;
mod keymap;
mod layout;
mod review;
mod procs;
mod proto;
mod project;
mod pty;
mod server;
mod worktree;
mod term;
mod theme;
mod search;
mod status;
mod tree;
mod ui;
mod uninstall;
mod viewer;
mod workspace;

use std::io::stdout;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::{Duration, Instant};

use anyhow::Result;
use notify::{RecursiveMode, Watcher};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    self, DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture, Event, KeyEventKind,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;

use app::App;

const USAGE: &str = "usage:
  pulpo [path]   open Pulpo; a repository at path (default: .) joins the project list
  pulpo hooks install|status|remove [--dry-run]
                 add Pulpo's hooks to Claude Code, for what agents run and ask
  pulpo hooks    print those hooks instead
  pulpo keytest  show what reaches Pulpo for each key (⌘p and ⌃b are the prefix)
  pulpo doctor   check that the prefix reaches Pulpo, and the hooks
  pulpo update   build and install the latest Pulpo
  pulpo uninstall
                 stop every terminal, delete the workspaces, the hooks and Pulpo itself
  pulpo emit     record a hook event from stdin (called by the hooks)
  pulpo serve    run the server that keeps terminals running (Pulpo starts it)";

/// So a build writing thousands of files doesn't run `git status` thousands of times.
const FS_THROTTLE: Duration = Duration::from_millis(300);
/// Output streaming in is drawn at most this often; keys and clicks are drawn at once.
const FRAME: Duration = Duration::from_millis(16);
/// So is what a key brings back soon after: its echo, the program's redraw.
const ECHO: Duration = Duration::from_millis(100);

pub enum Msg {
    Input(Event),
    Fs(Vec<PathBuf>),
    Agents,
    Server(proto::ToClient),
    Output(u32, Vec<u8>),
    ServerGone,
    Search(usize, Vec<search::Hit>),
    Repos(Vec<String>),
    Removed(PathBuf, PathBuf, bool),
    Copied((PathBuf, String), Result<(PathBuf, Vec<String>), String>),
    SetUp(PathBuf, PathBuf, bool),
    Changes(Box<review::Read>),
    Status(PathBuf, Box<git::Status>),
    Viewed(Box<viewer::Loaded>),
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("emit") => agents::emit(),
        Some("serve") => server::serve(&args[1..])?,
        // Run by a workspace's removal script.
        Some("forget-workspace") if args.len() == 2 => worktree::forget(std::path::Path::new(&args[1]))?,
        Some("hooks") => agents::hooks(&args[1..])?,
        Some("doctor") => doctor::doctor(&args[1..])?,
        Some("keytest") => doctor::keytest()?,
        Some("update") => update()?,
        Some("uninstall") => uninstall::uninstall()?,
        Some("-h" | "--help") => println!("{USAGE}"),
        arg => {
            let start = arg.map(PathBuf::from).unwrap_or(std::env::current_dir()?).canonicalize()?;
            let launch = git::toplevel(&start);
            // A workspace Pulpo made is its repository's, not a project of its own.
            let copy = launch.as_ref().is_some_and(|top| worktree::all().iter().any(|s| &s.root == top));
            if let Some(repo) = launch.as_deref().filter(|_| !copy).and_then(git::main_repo) {
                project::register(&repo);
            }
            run(launch, start)?;
        }
    }
    Ok(())
}

/// Runs the install script again, so an update gets its Rust and hook steps too.
fn update() -> Result<()> {
    let status = std::process::Command::new("sh").args(["-c", "set -o pipefail; curl -fsSL https://pulpo.sh/install | sh"]).status()?;
    if !status.success() {
        std::process::exit(status.code().unwrap_or(1));
    }
    Ok(())
}

fn run(launch: Option<PathBuf>, scratch: PathBuf) -> Result<()> {
    let (tx, rx) = mpsc::channel();

    let state_dir = agents::state_dir();
    std::fs::create_dir_all(&state_dir)?;
    let agent_tx = tx.clone();
    let mut agent_watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if res.is_ok() {
            let _ = agent_tx.send(Msg::Agents);
        }
    })?;
    agent_watcher.watch(&state_dir, RecursiveMode::NonRecursive)?;

    let input_tx = tx.clone();
    std::thread::spawn(move || {
        while let Ok(event) = event::read() {
            if input_tx.send(Msg::Input(event)).is_err() {
                return;
            }
        }
    });

    viewer::warm_up();
    let link = client::connect(tx.clone())?;
    let mut app = App::new(launch, scratch, tx, link)?;
    let mut terminal = ratatui::init();
    // Kitty keyboard protocol, so Ghostty reports keys like Shift+Enter; others ignore it.
    let keys = PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES);
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste, keys)?;
    let result = event_loop(&mut terminal, &mut app, &rx);
    execute!(stdout(), PopKeyboardEnhancementFlags, DisableMouseCapture, DisableBracketedPaste)?;
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, rx: &Receiver<Msg>) -> Result<()> {
    // Draw once so the terminal size is known before the server shows its terminals.
    terminal.draw(|frame| ui::draw(frame, app))?;
    if project::registered().is_empty() {
        app.open_repo_picker();
    }

    let mut last_fs_refresh = Instant::now();
    let (mut drawn, mut typed) = (Instant::now(), Instant::now());
    // Output from a terminal that isn't on screen changes nothing there.
    let mut changed = true;
    loop {
        app.tick();
        let wake = Duration::from_millis(if app.animating() { 100 } else { 500 });
        let held = app.loading();
        if held.is_none() && ((changed && (typed.elapsed() < ECHO || drawn.elapsed() >= FRAME)) || drawn.elapsed() >= wake) {
            terminal.draw(|frame| ui::draw(frame, app))?;
            drawn = Instant::now();
            changed = false;
        }

        let wait = held.unwrap_or_else(|| if changed { FRAME } else { wake }.saturating_sub(drawn.elapsed()));
        let first = match rx.recv_timeout(wait) {
            Ok(msg) => Some(msg),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        for msg in first.into_iter().chain(rx.try_iter()) {
            match &msg {
                Msg::Input(_) => (changed, typed) = (true, Instant::now()),
                Msg::Output(id, _) => changed |= app.shows(*id),
                Msg::Server(proto::ToClient::Info { info }) => changed |= app.changes_view(info),
                Msg::Fs(_) => {}
                _ => changed = true,
            }
            match msg {
                Msg::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if app.on_key(key) {
                        return Ok(());
                    }
                }
                Msg::Input(Event::Mouse(mouse)) => {
                    app.on_mouse(mouse);
                    if app.quit {
                        return Ok(());
                    }
                }
                Msg::Input(Event::Paste(text)) => app.on_paste(&text),
                Msg::Input(_) => {}
                Msg::Fs(paths) => app.on_fs(&paths),
                Msg::Agents => app.agents.load(),
                Msg::Server(message) => app.on_server(message),
                Msg::Output(id, bytes) => app.on_output(id, &bytes),
                Msg::ServerGone => app.on_server_gone(),
                Msg::Search(generation, hits) => app.on_search(generation, hits),
                Msg::Repos(repos) => app.on_repos(repos),
                Msg::Removed(root, log, ok) => app.on_removed(root, log, ok),
                Msg::Copied(copy, made) => app.on_copied(copy, made),
                Msg::SetUp(root, log, ok) => app.on_set_up(root, log, ok),
                Msg::Changes(read) => app.on_changes(*read),
                Msg::Status(root, status) => app.on_status(root, *status),
                Msg::Viewed(loaded) => app.on_viewed(*loaded),
            }
            app.settle();
        }
        if app.fs_pending() && last_fs_refresh.elapsed() >= FS_THROTTLE {
            app.flush_fs();
            last_fs_refresh = Instant::now();
            changed = true;
        }
    }
}
