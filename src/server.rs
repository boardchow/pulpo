//! Owns every terminal; upgrades in place (`execve`) so their programs and ptys survive.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

use crate::proto::{self, Frame, Info, ToClient, ToServer};
use crate::procs;
use crate::pty::{self, Pty};
use crate::status::Records;
use crate::term::{self, Events};

/// Bumped when `Resume` changes so that an older server can't read it.
const RESUME: u32 = 1;

const IDLE: Duration = Duration::from_secs(60);
const TICK: Duration = Duration::from_millis(250);
const BACKLOG: usize = 4096;
/// Output chunks in flight; when full, a program printing faster waits, as in any terminal.
const OUTPUT: usize = 64;
const INPUT: usize = 256;

const BUSY_FOR: Duration = Duration::from_millis(1500);
/// Output this soon after input is the program answering you (echo, redraw), not work.
const ECHO: Duration = Duration::from_millis(400);
const BLIP: Duration = Duration::from_millis(300);
const TASK: Duration = Duration::from_secs(1);

/// A Pulpo started inside a Pulpo terminal gets its own server, so it doesn't show itself.
pub fn dir() -> PathBuf {
    let base = crate::agents::state_dir();
    if std::env::var_os("PULPO_TAB").is_some() { base.join("server-nested") } else { base.join("server") }
}

pub fn socket() -> PathBuf {
    dir().join("sock")
}

enum Event {
    Connected(UnixStream),
    Request(u64, Frame),
    Disconnected(u64),
    Output(u32, Vec<u8>),
    Eof(u32),
}

/// Ctrl-C to the front program first (what `docker compose up` stops cleanly on), then hang-up, terminate, kill.
struct Stopping {
    /// Taken at once: the processes move to launchd when the shell exits.
    front: Option<i32>,
    groups: Vec<i32>,
    stage: Stage,
    since: Instant,
    /// Claude and Codex quit on a second ctrl-c instead of waiting out the grace.
    agent: Option<Instant>,
}

#[derive(Clone, Copy, PartialEq)]
enum Stage {
    Interrupt,
    HangUp,
    Terminate,
    Kill,
}

impl Stopping {
    fn advance(&mut self) -> bool {
        let alive: Vec<i32> = self.groups.iter().copied().filter(|&g| unsafe { libc::kill(-g, 0) } == 0).collect();
        self.groups = alive;
        if self.groups.is_empty() {
            return false;
        }
        let front_alive = self.front.is_some_and(|g| self.groups.contains(&g));
        if let (Some(group), Some(sent)) = (self.front, self.agent)
            && front_alive
            && self.stage == Stage::Interrupt
            && sent.elapsed() >= Duration::from_millis(500)
        {
            unsafe { libc::kill(-group, libc::SIGINT) };
            self.agent = Some(Instant::now());
        }
        let next = match self.stage {
            Stage::Interrupt if front_alive && self.since.elapsed() < Duration::from_secs(10) => return true,
            Stage::Interrupt => Stage::HangUp,
            Stage::HangUp if self.since.elapsed() < Duration::from_secs(3) => return true,
            Stage::HangUp => Stage::Terminate,
            Stage::Terminate if self.since.elapsed() < Duration::from_secs(3) => return true,
            Stage::Terminate => Stage::Kill,
            // Unkillable (stuck in the kernel): nothing more to do.
            Stage::Kill if self.since.elapsed() >= Duration::from_secs(5) => return false,
            Stage::Kill => Stage::Kill,
        };
        self.signal(next);
        true
    }

    fn signal(&mut self, stage: Stage) {
        let (signal, groups) = match stage {
            Stage::Interrupt => (libc::SIGINT, self.front.into_iter().collect()),
            Stage::HangUp => (libc::SIGHUP, self.groups.clone()),
            Stage::Terminate => (libc::SIGTERM, self.groups.clone()),
            Stage::Kill => (libc::SIGKILL, self.groups.clone()),
        };
        for group in groups {
            unsafe { libc::kill(-group, signal) };
        }
        if stage != self.stage {
            self.stage = stage;
            self.since = Instant::now();
        }
    }
}

struct Client {
    id: u64,
    frames: SyncSender<Vec<u8>>,
    stream: UnixStream,
    watching: Option<u32>,
}

struct Terminal {
    id: u32,
    tag: String,
    name: String,
    cwd: PathBuf,
    title: String,
    running: Option<String>,
    unread: bool,
    pty: Pty,
    /// Written by its own thread: a program that doesn't read would otherwise block every terminal.
    input: SyncSender<Vec<u8>>,
    parser: vt100::Parser<Events>,
    last_activity: Instant,
    working_since: Option<Instant>,
    last_input: Instant,
    eof: bool,
    exited: Option<Instant>,
    sent: Option<Info>,
}

impl Terminal {
    fn info(&self) -> Info {
        Info {
            id: self.id,
            tag: self.tag.clone(),
            name: self.name.clone(),
            running: self.running.clone(),
            cwd: self.cwd.clone(),
            title: self.title.clone(),
            pid: self.pty.pid,
            working: self.working(),
            unread: self.unread,
            status: self.parser.callbacks().status.summary(),
        }
    }

    fn size(&self) -> (u16, u16) {
        let (rows, cols) = self.parser.screen().size();
        (rows.get(), cols.get())
    }

    fn feed(&mut self, bytes: &[u8]) {
        let before = self.parser.callbacks().status.summary().map(|(status, _)| status);
        term::feed(&mut self.parser, bytes);
        if self.last_input.elapsed() > ECHO {
            self.settle();
            if !self.active() {
                self.working_since = Some(Instant::now());
            }
            self.last_activity = Instant::now();
        }
        let events = self.parser.callbacks_mut();
        let replies = std::mem::take(&mut events.replies);
        if let Some(title) = events.title.take() {
            self.title = title;
        }
        self.unread |= std::mem::take(&mut events.bell);
        // The clipboard goes through to Pulpo with the output.
        events.host.clear();
        if !replies.is_empty() {
            let _ = self.input.try_send(replies);
        }
        if let Some((now, _)) = self.parser.callbacks().status.summary()
            && Some(now) != before
            && matches!(now, crate::status::Status::Blocked | crate::status::Status::Error | crate::status::Status::Done)
        {
            self.unread = true;
        }
    }

    fn input(&mut self, bytes: &[u8]) {
        self.last_input = Instant::now();
        let _ = self.input.try_send(bytes.to_vec());
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        self.last_input = Instant::now();
        self.pty.resize(rows, cols);
        self.parser.set_size(term::nz(rows), term::nz(cols));
    }

    fn working(&self) -> bool {
        self.active() && self.working_since.is_some_and(|since| self.last_activity - since >= BLIP)
    }

    fn active(&self) -> bool {
        self.last_activity.elapsed() < BUSY_FOR
    }

    fn settle(&mut self) {
        if !self.active()
            && let Some(since) = self.working_since.take()
            && self.last_activity - since >= TASK
        {
            self.unread = true;
        }
    }

    fn refresh_foreground(&mut self) {
        self.running = match self.pty.foreground() {
            Some(group) if group != self.pty.pid => term::process_name(group as u32),
            _ => None,
        };
    }
}

struct Server {
    terminals: Vec<Terminal>,
    clients: Vec<Client>,
    next_terminal: u32,
    next_client: u64,
    events: SyncSender<Event>,
    idle_since: Instant,
    idle: Duration,
    /// Its own build, from when it started (the file may since be newer).
    built: u64,
    /// Held while the server runs; handed on with the listener on upgrade.
    lock: File,
    listener: RawFd,
    readers: Vec<JoinHandle<()>>,
    wake: (OwnedFd, OwnedFd),
    upgrade: Option<(u64, PathBuf)>,
    stopping: Vec<Stopping>,
}

#[derive(Serialize, Deserialize)]
struct Resume {
    version: u32,
    lock: RawFd,
    listener: RawFd,
    next_terminal: u32,
    terminals: Vec<Saved>,
}

#[derive(Serialize, Deserialize)]
struct Saved {
    id: u32,
    tag: String,
    name: String,
    cwd: PathBuf,
    title: String,
    unread: bool,
    master: RawFd,
    pid: i32,
    rows: u16,
    cols: u16,
    screen: String,
    status: Records,
}

pub fn serve(args: &[String]) -> Result<()> {
    match args {
        [flag, file] if flag == "--check-resume" => read_resume(Path::new(file)).map(drop),
        [flag, file] if flag == "--resume" => resume(Path::new(file)),
        [] => start(),
        _ => bail!("usage: pulpo serve"),
    }
}

fn start() -> Result<()> {
    let dir = dir();
    fs::create_dir_all(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
    // One server per account: the lock is held as long as the server runs.
    let lock = File::create(dir.join("lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return Ok(());
    }
    let path = socket();
    let _ = fs::remove_file(&path);
    let listener = UnixListener::bind(&path).with_context(|| format!("listening on {}", path.display()))?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    eprintln!("pulpo server started, pid {}", std::process::id());
    let (mut server, rx) = Server::new(lock, listener)?;
    server.serve(&rx);
    Ok(())
}

fn resume(file: &Path) -> Result<()> {
    let saved = read_resume(file)?;
    let _ = fs::remove_file(file);
    let (lock, listener) = unsafe { (File::from_raw_fd(saved.lock), UnixListener::from_raw_fd(saved.listener)) };
    pty::set_cloexec(lock.as_raw_fd(), true)?;
    pty::set_cloexec(listener.as_raw_fd(), true)?;
    let (mut server, rx) = Server::new(lock, listener)?;
    server.next_terminal = saved.next_terminal;
    for t in saved.terminals {
        // Only this server's own readers may hold a terminal's end.
        pty::set_cloexec(t.master, true)?;
        let mut parser = term::parser(t.rows, t.cols, Events::with_status(t.status));
        term::feed(&mut parser, t.screen.as_bytes());
        parser.callbacks_mut().replies.clear();
        let pty = unsafe { Pty::from_raw(t.master, t.pid) };
        server.readers.push(reader(t.id, &pty.master, server.wake.0.as_raw_fd(), server.events.clone())?);
        let input = writer(&pty.master)?;
        server.terminals.push(Terminal {
            id: t.id,
            tag: t.tag,
            name: t.name,
            cwd: t.cwd,
            title: t.title,
            running: None,
            unread: t.unread,
            pty,
            input,
            parser,
            last_activity: Instant::now(),
            working_since: None,
            last_input: Instant::now(),
            eof: false,
            exited: None,
            sent: None,
        });
    }
    eprintln!("pulpo server upgraded, pid {}, {} terminal(s) kept", std::process::id(), server.terminals.len());
    server.serve(&rx);
    Ok(())
}

fn read_resume(file: &Path) -> Result<Resume> {
    let saved: Resume = serde_json::from_slice(&fs::read(file)?).context("reading the upgrade state")?;
    if saved.version > RESUME {
        bail!("upgrade state version {} is newer than {RESUME}", saved.version);
    }
    Ok(saved)
}

fn reader(id: u32, master: &File, wake: RawFd, tx: SyncSender<Event>) -> io::Result<JoinHandle<()>> {
    let mut master = master.try_clone()?;
    Ok(std::thread::spawn(move || {
        let mut buf = [0u8; 16 * 1024];
        while readable(master.as_raw_fd(), wake) {
            match master.read(&mut buf) {
                Ok(n @ 1..) if tx.send(Event::Output(id, buf[..n].to_vec())).is_ok() => {}
                _ => {
                    let _ = tx.send(Event::Eof(id));
                    return;
                }
            }
        }
    }))
}

fn writer(master: &File) -> io::Result<SyncSender<Vec<u8>>> {
    let mut master = master.try_clone()?;
    let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(INPUT);
    std::thread::spawn(move || {
        for bytes in rx {
            if master.write_all(&bytes).is_err() {
                return;
            }
        }
    });
    Ok(tx)
}

/// `select`, because macOS doesn't support `poll` or kqueue on terminals.
fn readable(fd: RawFd, wake: RawFd) -> bool {
    loop {
        let mut set: libc::fd_set = unsafe { std::mem::zeroed() };
        unsafe {
            libc::FD_ZERO(&mut set);
            libc::FD_SET(fd, &mut set);
            libc::FD_SET(wake, &mut set);
        }
        let n = unsafe { libc::select(fd.max(wake) + 1, &mut set, std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut()) };
        if n < 0 && io::Error::last_os_error().kind() == io::ErrorKind::Interrupted {
            continue;
        }
        return n > 0 && !unsafe { libc::FD_ISSET(wake, &set) };
    }
}

fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = [0; 2];
    if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let (read, write) = unsafe { (OwnedFd::from_raw_fd(fds[0]), OwnedFd::from_raw_fd(fds[1])) };
    pty::set_cloexec(read.as_raw_fd(), true)?;
    pty::set_cloexec(write.as_raw_fd(), true)?;
    Ok((read, write))
}

impl Server {
    fn new(lock: File, listener: UnixListener) -> Result<(Server, Receiver<Event>)> {
        let (tx, rx) = mpsc::sync_channel(OUTPUT);
        let fd = listener.as_raw_fd();
        let accept_tx = tx.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                if accept_tx.send(Event::Connected(stream)).is_err() {
                    return;
                }
            }
        });
        let idle = std::env::var("PULPO_SERVER_IDLE_SECS").ok().and_then(|s| s.parse().ok()).map_or(IDLE, Duration::from_secs);
        let server = Server {
            terminals: Vec::new(),
            clients: Vec::new(),
            next_terminal: 1,
            next_client: 1,
            events: tx,
            idle_since: Instant::now(),
            idle,
            built: built(&std::env::current_exe().unwrap_or_default()),
            lock,
            listener: fd,
            readers: Vec::new(),
            wake: pipe()?,
            upgrade: None,
            stopping: Vec::new(),
        };
        Ok((server, rx))
    }

    fn serve(&mut self, rx: &Receiver<Event>) {
        self.run(rx);
        let _ = fs::remove_file(socket());
        eprintln!("pulpo server stopped: idle");
    }

    fn run(&mut self, rx: &Receiver<Event>) {
        let mut last_foreground = Instant::now();
        loop {
            match rx.recv_timeout(TICK) {
                Ok(event) => self.handle(event),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            for event in rx.try_iter().collect::<Vec<_>>() {
                self.handle(event);
            }
            // Not while terminals are being stopped: that would stop with it.
            if self.stopping.is_empty()
                && let Some((client, exe)) = self.upgrade.take()
            {
                // Only returns if the upgrade failed; then carry on as before.
                let error = self.upgrade(&exe, rx);
                eprintln!("upgrade to {} failed: {error:#}", exe.display());
                self.send(client, &ToClient::Error { message: format!("the Pulpo server could not upgrade: {error:#}") });
            }
            if last_foreground.elapsed() >= Duration::from_secs(1) {
                for terminal in &mut self.terminals {
                    terminal.refresh_foreground();
                }
                last_foreground = Instant::now();
            }
            self.reap();
            self.push_info();
            self.stopping.retain_mut(Stopping::advance);
            if !self.terminals.is_empty() || !self.clients.is_empty() || !self.stopping.is_empty() {
                self.idle_since = Instant::now();
            } else if self.idle_since.elapsed() >= self.idle {
                return;
            }
        }
    }

    fn handle(&mut self, event: Event) {
        match event {
            Event::Connected(stream) => self.connect(stream),
            Event::Disconnected(client) => self.clients.retain(|c| c.id != client),
            Event::Output(id, bytes) => {
                if let Some(terminal) = self.terminal(id) {
                    terminal.feed(&bytes);
                    let frame = proto::output(id, &bytes);
                    for i in (0..self.clients.len()).rev() {
                        if self.clients[i].watching == Some(id) {
                            self.deliver(i, frame.clone());
                        }
                    }
                }
            }
            Event::Eof(id) => {
                if let Some(terminal) = self.terminal(id) {
                    terminal.eof = true;
                }
            }
            Event::Request(client, Frame::Input(id, bytes)) => {
                let _ = client;
                if let Some(terminal) = self.terminal(id) {
                    terminal.input(&bytes);
                }
            }
            Event::Request(client, Frame::Json(json)) => match serde_json::from_slice::<ToServer>(&json) {
                Ok(request) => self.request(client, request),
                Err(e) => self.send(client, &ToClient::Error { message: format!("bad request: {e}") }),
            },
            Event::Request(_, Frame::Output(..)) => {}
        }
    }

    fn request(&mut self, client: u64, request: ToServer) {
        match request {
            ToServer::Hello { protocol } => {
                let exe = std::env::current_exe().unwrap_or_default();
                let hello = ToClient::Hello { protocol: proto::PROTOCOL, pid: std::process::id(), exe, built: self.built };
                self.send(client, &hello);
                // A Pulpo that speaks another protocol only gets to upgrade.
                if protocol == proto::PROTOCOL {
                    for i in 0..self.terminals.len() {
                        self.attach(client, i, None);
                    }
                    self.send(client, &ToClient::Ready);
                }
            }
            ToServer::Spawn { token, program, args, cwd, rows, cols, env } => match self.spawn(&program, &args, &cwd, &env, (rows, cols)) {
                Ok(()) => {
                    let i = self.terminals.len() - 1;
                    let others: Vec<u64> = self.clients.iter().map(|c| c.id).filter(|&c| c != client).collect();
                    self.attach(client, i, Some(token));
                    for other in others {
                        self.attach(other, i, None);
                    }
                }
                Err(e) => self.send(client, &ToClient::Error { message: format!("could not start {program}: {e}") }),
            },
            ToServer::Resize { id, rows, cols } => {
                if let Some(terminal) = self.terminal(id)
                    && terminal.size() != (rows, cols)
                {
                    terminal.resize(rows, cols);
                    self.broadcast(&proto::encode(&ToClient::Resized { id, rows, cols }));
                }
            }
            ToServer::Watch { id } => self.watch(client, id),
            ToServer::Seen { id } => {
                if let Some(terminal) = self.terminal(id) {
                    terminal.unread = false;
                }
            }
            ToServer::Close { id } => {
                if let Some(terminal) = self.terminal(id) {
                    let (shell, front) = (terminal.pty.pid, terminal.pty.foreground());
                    let table = procs::table();
                    let mut groups: Vec<i32> = procs::tree(&table, shell).iter().map(|p| p.pgid).filter(|&g| g > 1).collect();
                    groups.sort_unstable();
                    groups.dedup();
                    // A shell at its prompt has nothing to interrupt.
                    let front = front.filter(|&g| g != shell && groups.contains(&g));
                    let agent = front.and_then(|g| term::process_name(g as u32)).filter(|name| crate::agents::KINDS.iter().any(|k| k.name == name)).map(|_| Instant::now());
                    let mut stopping = Stopping { front, groups, stage: Stage::Interrupt, since: Instant::now(), agent };
                    stopping.signal(if front.is_some() { Stage::Interrupt } else { Stage::HangUp });
                    // With a program to interrupt, the shell is hung up after
                    // the grace, with the rest: now, it would pass the hang-up on.
                    if front.is_none() {
                        terminal.pty.hang_up();
                    }
                    self.stopping.push(stopping);
                }
            }
            ToServer::Upgrade { exe } => self.upgrade = Some((client, exe)),
        }
    }

    fn spawn(&mut self, program: &str, args: &[String], cwd: &Path, extra: &[(String, String)], (rows, cols): (u16, u16)) -> Result<()> {
        let id = self.next_terminal;
        let tag = format!("{}-{id}", std::process::id());
        let mut env = vec![
            ("TERM", "xterm-256color"),
            ("COLORTERM", "truecolor"),
            // micro only uses 24-bit color when told.
            ("MICRO_TRUECOLOR", "1"),
            ("PULPO_TAB", tag.as_str()),
        ];
        env.extend(extra.iter().map(|(k, v)| (k.as_str(), v.as_str())));
        // Don't let programs assume Ghostty features this emulator lacks.
        let pty = pty::spawn(program, args, cwd, &env, &["TERM_PROGRAM", "TERM_PROGRAM_VERSION"], (rows, cols))?;
        let input = writer(&pty.master)?;
        self.readers.push(reader(id, &pty.master, self.wake.0.as_raw_fd(), self.events.clone())?);
        self.next_terminal += 1;
        let name = Path::new(program).file_name().unwrap_or_default().to_string_lossy().into_owned();
        self.terminals.push(Terminal {
            id,
            tag,
            name,
            cwd: cwd.to_path_buf(),
            title: String::new(),
            running: None,
            unread: false,
            pty,
            input,
            parser: term::parser(rows, cols, Events::default()),
            last_activity: Instant::now(),
            working_since: None,
            last_input: Instant::now(),
            eof: false,
            exited: None,
            sent: None,
        });
        Ok(())
    }

    /// Returns only if the upgrade fails, with the server as it was.
    fn upgrade(&mut self, exe: &Path, rx: &Receiver<Event>) -> anyhow::Error {
        // Stop the readers, then take in what they read, so no output is lost across the exec.
        let _ = File::from(self.wake.1.try_clone().expect("a duplicate")).write_all(b"x");
        // A reader may be waiting for room in the channel: keep taking.
        let mut later = Vec::new();
        loop {
            let stopped = self.readers.iter().all(JoinHandle::is_finished);
            for event in rx.try_iter().collect::<Vec<_>>() {
                match event {
                    Event::Output(..) | Event::Eof(_) => self.handle(event),
                    other => later.push(other),
                }
            }
            if stopped {
                break;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        let Err(error) = self.hand_over(exe);
        self.wake = pipe().expect("a pipe");
        for terminal in &self.terminals {
            if let Ok(reader) = reader(terminal.id, &terminal.pty.master, self.wake.0.as_raw_fd(), self.events.clone()) {
                self.readers.push(reader);
            }
        }
        for event in later {
            self.handle(event);
        }
        error
    }

    fn hand_over(&mut self, exe: &Path) -> Result<std::convert::Infallible> {
        let saved = Resume {
            version: RESUME,
            lock: self.lock.as_raw_fd(),
            listener: self.listener,
            next_terminal: self.next_terminal,
            terminals: self
                .terminals
                .iter()
                .filter(|t| t.exited.is_none())
                .map(|t| {
                    let (rows, cols) = t.size();
                    Saved {
                        id: t.id,
                        tag: t.tag.clone(),
                        name: t.name.clone(),
                        cwd: t.cwd.clone(),
                        title: t.title.clone(),
                        unread: t.unread,
                        master: t.pty.master.as_raw_fd(),
                        pid: t.pty.pid,
                        rows,
                        cols,
                        screen: String::from_utf8_lossy(&term::snapshot(&t.parser, &t.title)).into_owned(),
                        status: t.parser.callbacks().status.clone(),
                    }
                })
                .collect(),
        };
        let file = dir().join("upgrade.json");
        fs::write(&file, serde_json::to_vec(&saved)?)?;
        fs::set_permissions(&file, fs::Permissions::from_mode(0o600))?;
        // A build that can't read it would lose every terminal: check first.
        let mut check = Command::new(exe).args(["serve", "--check-resume"]).arg(&file).spawn()?;
        let started = Instant::now();
        let status = loop {
            if let Some(status) = check.try_wait()? {
                break status;
            }
            if started.elapsed() > Duration::from_secs(5) {
                let _ = check.kill();
                bail!("{} did not answer", exe.display());
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        if !status.success() {
            bail!("{} can't take over these terminals ({status})", exe.display());
        }
        let handed: Vec<RawFd> = saved.terminals.iter().map(|t| t.master).chain([saved.lock, saved.listener]).collect();
        for &fd in &handed {
            pty::set_cloexec(fd, false)?;
        }
        eprintln!("upgrading to {}", exe.display());
        let error = Command::new(exe).args(["serve", "--resume"]).arg(&file).exec();
        for &fd in &handed {
            let _ = pty::set_cloexec(fd, true);
        }
        let _ = fs::remove_file(&file);
        Err(error.into())
    }

    fn attach(&mut self, client: u64, i: usize, token: Option<u64>) {
        let terminal = &mut self.terminals[i];
        let info = terminal.info();
        terminal.sent = Some(info.clone());
        let (rows, cols) = terminal.size();
        self.send(client, &ToClient::Attach { info, rows, cols, token });
    }

    fn watch(&mut self, client: u64, id: Option<u32>) {
        let Some(c) = self.clients.iter_mut().find(|c| c.id == client) else { return };
        c.watching = id;
        let Some(terminal) = id.and_then(|id| self.terminals.iter().find(|t| t.id == id)) else { return };
        let (rows, cols) = terminal.size();
        let snapshot = proto::output(terminal.id, &term::snapshot(&terminal.parser, &terminal.title));
        self.send(client, &ToClient::Snapshot { id: terminal.id, rows, cols });
        self.send_frame(client, snapshot);
    }

    /// Waits on the process, not only for EOF: a background job can keep the terminal open after its shell exits.
    fn reap(&mut self) {
        let mut ended = Vec::new();
        for terminal in &mut self.terminals {
            if terminal.exited.is_none() && terminal.pty.exited() {
                terminal.exited = Some(Instant::now());
            }
            if terminal.exited.is_some_and(|at| terminal.eof || at.elapsed() >= Duration::from_secs(1)) {
                ended.push(terminal.id);
            }
        }
        for id in ended {
            self.terminals.retain(|t| t.id != id);
            self.broadcast(&proto::encode(&ToClient::Exit { id }));
        }
    }

    fn push_info(&mut self) {
        let mut changed = Vec::new();
        for terminal in &mut self.terminals {
            terminal.settle();
            let info = terminal.info();
            if terminal.sent.as_ref() != Some(&info) {
                terminal.sent = Some(info.clone());
                changed.push(info);
            }
        }
        for info in changed {
            self.broadcast(&proto::encode(&ToClient::Info { info }));
        }
    }

    fn terminal(&mut self, id: u32) -> Option<&mut Terminal> {
        self.terminals.iter_mut().find(|t| t.id == id)
    }

    fn connect(&mut self, stream: UnixStream) {
        let (Ok(mut reader), Ok(mut writer)) = (stream.try_clone(), stream.try_clone()) else { return };
        let id = self.next_client;
        self.next_client += 1;
        let (frames, queue) = mpsc::sync_channel::<Vec<u8>>(BACKLOG);
        std::thread::spawn(move || {
            for frame in queue {
                if writer.write_all(&frame).is_err() {
                    return;
                }
            }
        });
        let tx = self.events.clone();
        std::thread::spawn(move || {
            while let Ok(frame) = proto::read_frame(&mut reader) {
                if tx.send(Event::Request(id, frame)).is_err() {
                    return;
                }
            }
            let _ = tx.send(Event::Disconnected(id));
        });
        self.clients.push(Client { id, frames, stream, watching: None });
    }

    fn send(&mut self, client: u64, message: &ToClient) {
        self.send_frame(client, proto::encode(message));
    }

    fn send_frame(&mut self, client: u64, frame: Vec<u8>) {
        if let Some(i) = self.clients.iter().position(|c| c.id == client) {
            self.deliver(i, frame);
        }
    }

    fn broadcast(&mut self, frame: &[u8]) {
        for i in (0..self.clients.len()).rev() {
            self.deliver(i, frame.to_vec());
        }
    }

    /// A Pulpo too slow to keep up is dropped rather than holding up the terminals; it reconnects.
    fn deliver(&mut self, i: usize, frame: Vec<u8>) {
        if let Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) = self.clients[i].frames.try_send(frame) {
            let client = self.clients.remove(i);
            let _ = client.stream.shutdown(std::net::Shutdown::Both);
        }
    }
}

pub fn built(exe: &Path) -> u64 {
    fs::metadata(exe)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}
