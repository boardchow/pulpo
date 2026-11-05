//! Many busy agents at once, as numbers to compare builds by: `make stress`.
//! `PULPO_STRESS_EXE=/path/to/pulpo` measures another build with the same load.

mod common;

use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::os::fd::{FromRawFd, OwnedFd};
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use common::{connect, frame, read_frame};
use serde_json::{Value, json};

const AGENTS: usize = 30;
const WORKSPACES: usize = 20;
const WINDOW: Duration = Duration::from_secs(10);

/// What Claude Code prints: a long history, then a region redrawn 20 times a second, a frame
/// per write, with a line added above it now and then, and a file written every second.
const AGENT: &str = r#"
my @c = map { "\e[3${_}m" } 1..6;
for my $i (1..5000) { print $c[$i % 6], "history line $i ", "x" x 100, "\e[m\n"; }
STDOUT->flush();
my $n = 0;
while (1) {
    my $out = $n ? ("\e[2K\e[1A" x 20) . "\e[2K\r" : "";
    $out .= "\e[32m*\e[m step $n " . ("y" x 80) . "\n" if $n % 20 == 0;
    for my $l (1..20) { $out .= $c[($l + $n) % 6] . "  working $n " . ("." x ($l * 4)) . "\e[m\n"; }
    syswrite(STDOUT, $out);
    if ($n % 20 == 0) { open(my $f, ">", "notes.txt"); print $f $n; close $f; }
    $n++;
    select(undef, undef, undef, 0.05);
}
"#;

enum Got {
    Message(Value),
    Output(u32, Vec<u8>),
}

/// A client that reads on its own thread, so the server never waits for it.
struct Client {
    stream: UnixStream,
    got: Receiver<(Instant, Got)>,
    /// Output passed on only for these terminals; the rest is counted.
    listen: Arc<Mutex<Vec<u32>>>,
    bytes: Arc<AtomicU64>,
    protocol: u32,
    server: u32,
}

impl Client {
    fn open(sock: &Path) -> Client {
        // Any build answers a hello with its own protocol.
        let mut probe = connect(sock);
        probe.write_all(&frame(0, &serde_json::to_vec(&json!({ "type": "hello", "protocol": 1 })).unwrap())).unwrap();
        let hello: Value = loop {
            let (kind, payload) = read_frame(&mut probe).expect("a hello");
            if kind == 0 {
                break serde_json::from_slice(&payload).unwrap();
            }
        };
        drop(probe);
        let protocol = hello["protocol"].as_u64().unwrap() as u32;
        let stream = connect(sock);
        let (tx, got) = mpsc::channel();
        let listen = Arc::new(Mutex::new(Vec::new()));
        let bytes = Arc::new(AtomicU64::new(0));
        let mut reader = stream.try_clone().unwrap();
        let (l, b) = (listen.clone(), bytes.clone());
        std::thread::spawn(move || {
            while let Some((kind, payload)) = read_frame(&mut reader) {
                b.fetch_add(payload.len() as u64 + 5, Ordering::Relaxed);
                let got = match kind {
                    0 => Got::Message(serde_json::from_slice(&payload).unwrap()),
                    _ => {
                        let id = u32::from_be_bytes(payload[..4].try_into().unwrap());
                        if !l.lock().unwrap().contains(&id) {
                            continue;
                        }
                        Got::Output(id, payload[4..].to_vec())
                    }
                };
                if tx.send((Instant::now(), got)).is_err() {
                    return;
                }
            }
        });
        let mut client = Client { stream, got, listen, bytes, protocol, server: hello["pid"].as_u64().unwrap() as u32 };
        client.send(json!({ "type": "hello", "protocol": protocol }));
        client
    }

    fn send(&mut self, message: Value) {
        self.stream.write_all(&frame(0, &serde_json::to_vec(&message).unwrap())).unwrap();
    }

    fn input(&mut self, id: u32, text: &str) {
        self.stream.write_all(&frame(2, &[&id.to_be_bytes()[..], text.as_bytes()].concat())).unwrap();
    }

    fn next(&self, timeout: Duration) -> Option<(Instant, Got)> {
        self.got.recv_timeout(timeout).ok()
    }

    fn until(&self, kind: &str) -> (Instant, Value) {
        loop {
            match self.next(Duration::from_secs(60)).unwrap_or_else(|| panic!("no {kind} from the server")) {
                (at, Got::Message(m)) if m["type"] == kind => return (at, m),
                _ => {}
            }
        }
    }

    fn spawn(&mut self, token: u64, program: &str, args: &[&str], cwd: &Path) -> u32 {
        let message = json!({ "type": "spawn", "token": token, "program": program, "args": args, "cwd": cwd, "rows": 40, "cols": 150 });
        self.send(message);
        loop {
            let (_, attach) = self.until("attach");
            if attach["token"] == token {
                return attach["info"]["id"].as_u64().unwrap() as u32;
            }
        }
    }

    /// What it takes to show a terminal: its whole screen and history.
    fn watch(&mut self, id: u32) -> Option<(Duration, usize)> {
        if self.protocol < 2 {
            return None;
        }
        self.listen.lock().unwrap().push(id);
        let sent = Instant::now();
        self.send(json!({ "type": "watch", "id": id }));
        loop {
            if let (at, Got::Output(got, bytes)) = self.next(Duration::from_secs(30)).expect("a snapshot")
                && got == id
            {
                return Some((at - sent, bytes.len()));
            }
        }
    }

    /// From a key to its echo.
    fn key(&mut self, id: u32, key: char) -> Option<Duration> {
        let sent = Instant::now();
        self.input(id, &key.to_string());
        while let Some((at, got)) = self.next(Duration::from_secs(2).saturating_sub(sent.elapsed())) {
            if let Got::Output(got, bytes) = got
                && got == id
                && bytes.contains(&(key as u8))
            {
                return Some(at - sent);
            }
        }
        None
    }
}

/// Pulpo itself, on a terminal of its own nobody looks at.
struct Tui {
    child: Child,
    bytes: Arc<AtomicU64>,
    keyboard: fs::File,
    /// When it drew a typed `Q`.
    drew: Receiver<Instant>,
    /// When it drew an agent's screen.
    agent: Receiver<Instant>,
}

impl Tui {
    fn start(exe: &str, state: &Path, repo: &Path) -> Tui {
        let (mut master, mut slave) = (-1, -1);
        let mut size = libc::winsize { ws_row: 50, ws_col: 200, ws_xpixel: 0, ws_ypixel: 0 };
        assert_eq!(unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) }, 0);
        let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
        let path = format!("{}:{}", state.join("bin").display(), std::env::var("PATH").unwrap());
        let mut cmd = Command::new(exe);
        cmd.arg(repo)
            .current_dir(repo)
            .env("XDG_STATE_HOME", state)
            .env("XDG_CONFIG_HOME", state.join("config"))
            .env("PATH", path)
            .env("TERM", "xterm-256color")
            .env("SHELL", "/bin/sh")
            .env_remove("PULPO_TAB")
            .stdin(Stdio::from(slave.try_clone().unwrap()))
            .stdout(Stdio::from(slave.try_clone().unwrap()))
            .stderr(Stdio::from(slave));
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                libc::ioctl(0, libc::TIOCSCTTY as _, 0);
                Ok(())
            });
        }
        let child = cmd.spawn().unwrap();
        let bytes = Arc::new(AtomicU64::new(0));
        let counted = bytes.clone();
        let mut screen = fs::File::from(master);
        let keyboard = screen.try_clone().unwrap();
        let (tx, drew) = mpsc::channel();
        let (agent_tx, agent) = mpsc::channel();
        std::thread::spawn(move || {
            let mut buf = [0u8; 64 * 1024];
            while let Ok(n @ 1..) = screen.read(&mut buf) {
                counted.fetch_add(n as u64, Ordering::Relaxed);
                if buf[..n].contains(&b'Q') {
                    let _ = tx.send(Instant::now());
                }
                if buf[..n].windows(7).any(|w| w == b"working") {
                    let _ = agent_tx.send(Instant::now());
                }
                if buf[..n].windows(4).any(|w| w == b"\x1b[6n") {
                    let _ = screen.write_all(b"\x1b[1;1R");
                }
            }
        });
        Tui { child, bytes, keyboard, drew, agent }
    }

    /// From a key typed into Pulpo to Pulpo drawing its echo.
    fn key(&mut self) -> Option<Duration> {
        while self.drew.try_recv().is_ok() {}
        let sent = Instant::now();
        self.keyboard.write_all(b"Q").unwrap();
        self.drew.recv_timeout(Duration::from_secs(2)).ok().map(|at| at - sent)
    }

    /// From the prefix and a workspace's number to its agent drawn.
    fn switch(&mut self, workspace: u8) -> Option<Duration> {
        // From the first one, where nothing draws.
        self.keyboard.write_all(&[0x02, b'1']).unwrap();
        std::thread::sleep(Duration::from_millis(300));
        while self.agent.try_recv().is_ok() {}
        let sent = Instant::now();
        self.keyboard.write_all(&[0x02, b'0' + workspace]).unwrap();
        self.agent.recv_timeout(Duration::from_secs(2)).ok().map(|at| at - sent)
    }
}

fn sh(dir: &Path, script: &str) {
    let out = Command::new("sh").args(["-c", script]).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "{script}: {}", String::from_utf8_lossy(&out.stderr));
}

fn cpu_secs(pid: u32) -> f64 {
    let out = Command::new("ps").args(["-o", "time=", "-p", &pid.to_string()]).output().unwrap();
    String::from_utf8_lossy(&out.stdout).trim().split(':').fold(0.0, |acc, part| acc * 60.0 + part.parse::<f64>().unwrap_or(0.0))
}

/// What Activity Monitor shows: unlike RSS, it counts what macOS compressed.
fn footprint_mb(pid: u32) -> u64 {
    let mut info: libc::rusage_info_v4 = unsafe { std::mem::zeroed() };
    let got = unsafe { libc::proc_pid_rusage(pid as i32, libc::RUSAGE_INFO_V4, (&raw mut info).cast()) };
    if got == 0 { info.ri_phys_footprint / 1_000_000 } else { 0 }
}

fn lines(path: &Path) -> usize {
    fs::read_to_string(path).map_or(0, |text| text.lines().count())
}

struct Window {
    secs: f64,
    cpu: HashMap<&'static str, f64>,
    memory: HashMap<&'static str, u64>,
    drawn: u64,
    git: usize,
    keys: Vec<Option<Duration>>,
    typed: Vec<Option<Duration>>,
}

fn measure(client: &mut Client, tui: &mut Tui, probe: u32, git_log: &Path) -> Window {
    let pids = [("server", client.server), ("pulpo", tui.child.id())];
    let before: Vec<f64> = pids.iter().map(|(_, pid)| cpu_secs(*pid)).collect();
    let (drawn, git, started) = (tui.bytes.load(Ordering::Relaxed), lines(git_log), Instant::now());
    let (mut keys, mut typed) = (Vec::new(), Vec::new());
    let mut key = b'a';
    while started.elapsed() < WINDOW {
        keys.push(client.key(probe, key as char));
        typed.push(tui.key());
        key = if key == b'z' { b'a' } else { key + 1 };
        std::thread::sleep(Duration::from_millis(100));
    }
    let secs = started.elapsed().as_secs_f64();
    Window {
        secs,
        cpu: pids.iter().zip(&before).map(|((name, pid), b)| (*name, (cpu_secs(*pid) - b) / secs * 100.0)).collect(),
        memory: pids.iter().map(|(name, pid)| (*name, footprint_mb(*pid))).collect(),
        drawn: tui.bytes.load(Ordering::Relaxed) - drawn,
        git: lines(git_log) - git,
        keys,
        typed,
    }
}

fn latency(what: &str, samples: &[Option<Duration>]) {
    let mut keys: Vec<Duration> = samples.iter().flatten().copied().collect();
    keys.sort();
    let lost = samples.len() - keys.len();
    let at = |q: f64| keys.get(((keys.len() as f64 * q) as usize).min(keys.len().saturating_sub(1))).map_or(f64::NAN, |d| d.as_secs_f64() * 1000.0);
    println!("  {what:<7} p50 {:.1} ms  p95 {:.1} ms  max {:.1} ms  lost {lost}/{}", at(0.5), at(0.95), at(1.0), samples.len());
}

fn report(name: &str, w: &Window) {
    println!("{name}");
    for who in ["server", "pulpo"] {
        println!("  {who:<7} {:>5.1}% cpu  {:>5} MB", w.cpu[who], w.memory[who]);
    }
    println!("  drawn   {:>5.1} KB/s to the terminal", w.drawn as f64 / 1024.0 / w.secs);
    println!("  git     {:>5.1} runs/s", w.git as f64 / w.secs);
    latency("server", &w.keys);
    latency("typed", &w.typed);
}

#[test]
#[ignore = "a minute of load; run with make stress"]
fn many_busy_agents() {
    let exe = std::env::var("PULPO_STRESS_EXE").unwrap_or_else(|_| env!("CARGO_BIN_EXE_pulpo").into());
    // Short, for the socket path; canonical, as Pulpo keeps paths (/tmp is a link).
    let state = PathBuf::from(format!("/tmp/pulpo-stress-{}", std::process::id()));
    let _ = fs::remove_dir_all(&state);
    fs::create_dir_all(state.join("bin")).unwrap();
    let state = state.canonicalize().unwrap();
    let repo = state.join("repo");
    fs::create_dir_all(repo.join("src")).unwrap();
    for i in 0..200 {
        fs::write(repo.join(format!("src/file{i}.rs")), "fn main() {}\n".repeat(50)).unwrap();
    }
    sh(&repo, "git init -q -b main && git add . && git -c user.email=t@t -c user.name=t commit -qm start");
    let mut workspaces = Vec::new();
    for i in 0..WORKSPACES {
        let root = state.join(format!("ws/w{i}"));
        sh(&repo, &format!("git worktree add -q {} -b w{i}", root.display()));
        workspaces.push(json!({
            "name": format!("w{i}"), "repo": repo, "root": root, "project": format!("repo-w{i}"),
            "main_project": "repo", "ports": [], "images": [], "kind": "worktree",
        }));
    }
    fs::create_dir_all(state.join("pulpo")).unwrap();
    fs::write(state.join("pulpo/workspaces.json"), serde_json::to_vec(&workspaces).unwrap()).unwrap();
    let git_log = state.join("git.log");
    let git = String::from_utf8(Command::new("sh").args(["-c", "command -v git"]).output().unwrap().stdout).unwrap();
    let wrapper = state.join("bin/git");
    fs::write(&wrapper, format!("#!/bin/sh\necho \"$*\" >> {}\nexec {} \"$@\"\n", git_log.display(), git.trim())).unwrap();
    sh(&state, &format!("chmod +x {}", wrapper.display()));
    let agent = state.join("agent.pl");
    fs::write(&agent, AGENT).unwrap();

    let mut tui = Tui::start(&exe, &state, &repo);
    let sock = state.join("pulpo/server/sock");
    let mut client = Client::open(&sock);
    client.until("ready");
    println!("\n{exe}, protocol {}: {AGENTS} agents in {WORKSPACES} workspaces", client.protocol);

    let probe = client.spawn(1, "/bin/cat", &[], &repo);
    client.listen.lock().unwrap().push(probe);
    client.watch(probe);
    let agent_path = agent.display().to_string();
    let agents: Vec<u32> = (0..AGENTS)
        .map(|i| client.spawn(10 + i as u64, "/usr/bin/perl", &[&agent_path], &state.join(format!("ws/w{}", i % WORKSPACES))))
        .collect();

    // The history they print first, then the redraws.
    let (server_cpu, tui_cpu) = (cpu_secs(client.server), cpu_secs(tui.child.id()));
    std::thread::sleep(Duration::from_secs(15));
    println!(
        "start   server {:.1} s cpu, pulpo {:.1} s cpu, for {} MB of history",
        cpu_secs(client.server) - server_cpu,
        cpu_secs(tui.child.id()) - tui_cpu,
        AGENTS * 5000 * 125 / 1_000_000
    );

    let busy = measure(&mut client, &mut tui, probe, &git_log);
    report("busy agents, watching a quiet terminal", &busy);

    if let Some((took, bytes)) = client.watch(agents[0]) {
        println!("switch  {:.1} ms to show a busy agent ({} KB)", took.as_secs_f64() * 1000.0, bytes / 1024);
    }
    client.watch(probe);
    let other = Client::open(&sock);
    let sent = Instant::now();
    other.until("ready");
    let mb = other.bytes.load(Ordering::Relaxed) as f64 / 1_000_000.0;
    println!("connect {:.1} ms to be ready ({mb:.1} MB)", sent.elapsed().as_secs_f64() * 1000.0);
    drop(other);

    let line = "\x1b[33mflood\x1b[m ".repeat(12);
    client.spawn(100, "/usr/bin/yes", &[&line], &repo);
    std::thread::sleep(Duration::from_secs(2));
    let flood = measure(&mut client, &mut tui, probe, &git_log);
    report("plus one terminal printing as fast as it can", &flood);

    // Its own terminal first: the agents only show from the second workspace on.
    let switches: Vec<Option<Duration>> = (0..10).map(|i| tui.switch(2 + i % 8)).collect();
    println!("in pulpo");
    latency("switch", &switches);

    unsafe {
        libc::kill(tui.child.id() as i32, libc::SIGKILL);
        libc::kill(client.server as i32, libc::SIGKILL);
    }
    let _ = Command::new("pkill").args(["-f", &agent_path]).status();
    let _ = fs::remove_dir_all(&state);
}
