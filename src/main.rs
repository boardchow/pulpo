mod agents;
mod app;
mod git;
mod term;
mod theme;
mod search;
mod tree;
mod ui;
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
  ws [path]   open ws; a repository at path (default: .) joins the workspace list
  ws hooks    print the Claude Code hooks config for detailed agent status
  ws emit     record a hook event from stdin (called by the hooks)";

/// Minimum time between two refreshes caused by file changes, so a build
/// writing thousands of files doesn't run `git status` thousands of times.
const FS_THROTTLE: Duration = Duration::from_millis(300);

pub enum Msg {
    Input(Event),
    Fs(Vec<PathBuf>),
    Agents,
    Pty(usize, Vec<u8>),
    PtyExit(usize),
    Search(usize, Vec<search::Hit>),
    Repos(Vec<String>),
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("emit") => agents::emit(),
        Some("hooks") => agents::print_hooks()?,
        Some("-h" | "--help") => println!("{USAGE}"),
        arg => {
            let start = arg.map(PathBuf::from).unwrap_or(std::env::current_dir()?).canonicalize()?;
            // Opening ws inside a repository adds it to the workspace list.
            let launch = git::toplevel(&start);
            if let Some(repo) = launch.as_deref().and_then(git::main_repo) {
                workspace::register(&repo);
            }
            run(launch, start)?;
        }
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
    let mut app = App::new(launch, scratch, tx)?;
    let mut terminal = ratatui::init();
    // The kitty keyboard protocol lets Ghostty report keys like Shift+Enter
    // that the legacy encoding can't tell apart. Terminals without it ignore this.
    let keys = PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES);
    execute!(stdout(), EnableMouseCapture, EnableBracketedPaste, keys)?;
    let result = event_loop(&mut terminal, &mut app, &rx);
    execute!(stdout(), PopKeyboardEnhancementFlags, DisableMouseCapture, DisableBracketedPaste)?;
    ratatui::restore();
    result
}

fn event_loop(terminal: &mut DefaultTerminal, app: &mut App, rx: &Receiver<Msg>) -> Result<()> {
    // Draw once so the terminal size is known, then open a first shell.
    terminal.draw(|frame| ui::draw(frame, app))?;
    app.spawn(&app::shell(), false);
    if workspace::registered().is_empty() {
        app.open_repo_picker();
    }

    let mut last_fs_refresh = Instant::now();
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Wake at least twice a second so "busy" indicators fade out.
        let first = match rx.recv_timeout(Duration::from_millis(500)) {
            Ok(msg) => Some(msg),
            Err(RecvTimeoutError::Timeout) => None,
            Err(RecvTimeoutError::Disconnected) => return Ok(()),
        };
        for msg in first.into_iter().chain(rx.try_iter()) {
            match msg {
                Msg::Input(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    if app.on_key(key) {
                        return Ok(());
                    }
                }
                Msg::Input(Event::Mouse(mouse)) => app.on_mouse(mouse),
                Msg::Input(Event::Paste(text)) => app.on_paste(&text),
                Msg::Input(_) => {}
                Msg::Fs(paths) => app.on_fs(&paths),
                Msg::Agents => app.agents.load(),
                Msg::Pty(id, bytes) => app.on_pty(id, &bytes),
                Msg::PtyExit(id) => app.on_exit(id),
                Msg::Search(generation, hits) => app.on_search(generation, hits),
                Msg::Repos(repos) => app.on_repos(repos),
            }
        }
        if app.fs_pending() && last_fs_refresh.elapsed() >= FS_THROTTLE {
            app.flush_fs();
            last_fs_refresh = Instant::now();
        }
    }
}
