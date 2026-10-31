mod agents;
mod app;
mod client;
mod git;
mod procs;
mod proto;
mod pty;
mod server;
mod term;
mod theme;
mod search;
mod status;
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
  pulpo [path]   open Pulpo; a repository at path (default: .) joins the workspace list
  pulpo hooks install|status|remove [--dry-run]
                 add Pulpo's hooks to Claude Code, for what agents run and ask
  pulpo hooks    print those hooks instead
  pulpo emit     record a hook event from stdin (called by the hooks)
  pulpo serve    run the server that keeps terminals running (Pulpo starts it)";

/// Minimum time between two refreshes caused by file changes, so a build
/// writing thousands of files doesn't run `git status` thousands of times.
const FS_THROTTLE: Duration = Duration::from_millis(300);

pub enum Msg {
    Input(Event),
    Fs(Vec<PathBuf>),
    Agents,
    Server(proto::ToClient),
    Output(u32, Vec<u8>),
    ServerGone,
    Search(usize, Vec<search::Hit>),
    Repos(Vec<String>),
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("emit") => agents::emit(),
        Some("serve") => server::serve(&args[1..])?,
        Some("hooks") => agents::hooks(&args[1..])?,
        Some("-h" | "--help") => println!("{USAGE}"),
        arg => {
            let start = arg.map(PathBuf::from).unwrap_or(std::env::current_dir()?).canonicalize()?;
            // Opening Pulpo inside a repository adds it to the workspace list.
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
    let link = client::connect(tx.clone())?;
    let mut app = App::new(launch, scratch, tx, link)?;
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
    // Draw once so the terminal size is known; the server then shows its
    // terminals, or a first shell.
    terminal.draw(|frame| ui::draw(frame, app))?;
    if workspace::registered().is_empty() {
        app.open_repo_picker();
    }

    let mut last_fs_refresh = Instant::now();
    loop {
        app.tick();
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Wake at least twice a second so "busy" indicators fade out, and
        // often enough to turn the spinner while something works.
        let wake = Duration::from_millis(if app.animating() { 100 } else { 500 });
        let first = match rx.recv_timeout(wake) {
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
                Msg::Server(message) => app.on_server(message),
                Msg::Output(id, bytes) => app.on_output(id, &bytes),
                Msg::ServerGone => app.on_server_gone(),
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
