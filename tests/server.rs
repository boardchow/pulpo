//! The server keeps terminals across an upgrade, stops what runs in them
//! when they close, and leaves once idle. Speaks the wire protocol by hand.

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

struct Conn(UnixStream);

impl Conn {
    fn open(sock: &PathBuf) -> Conn {
        let started = Instant::now();
        loop {
            match UnixStream::connect(sock) {
                Ok(stream) => {
                    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
                    let mut conn = Conn(stream);
                    conn.send(json!({ "type": "hello", "protocol": 1 }));
                    return conn;
                }
                Err(_) if started.elapsed() < Duration::from_secs(5) => std::thread::sleep(Duration::from_millis(50)),
                Err(e) => panic!("no server at {}: {e}", sock.display()),
            }
        }
    }

    fn frame(&mut self, kind: u8, payload: &[u8]) {
        let mut out = (payload.len() as u32).to_be_bytes().to_vec();
        out.push(kind);
        out.extend(payload);
        self.0.write_all(&out).unwrap();
    }

    fn send(&mut self, message: Value) {
        self.frame(0, &serde_json::to_vec(&message).unwrap());
    }

    fn input(&mut self, id: u64, text: &str) {
        self.frame(2, &[&(id as u32).to_be_bytes()[..], text.as_bytes()].concat());
    }

    /// The next frame: a JSON message, or a terminal's output as text.
    fn read(&mut self) -> Option<Result<Value, String>> {
        let mut head = [0u8; 5];
        self.0.read_exact(&mut head).ok()?;
        let mut payload = vec![0u8; u32::from_be_bytes(head[..4].try_into().unwrap()) as usize];
        self.0.read_exact(&mut payload).ok()?;
        Some(match head[4] {
            0 => Ok(serde_json::from_slice(&payload).unwrap()),
            _ => Err(String::from_utf8_lossy(&payload[4..]).into_owned()),
        })
    }

    /// Read until a message of `kind` comes; returns it.
    fn until(&mut self, kind: &str) -> Value {
        loop {
            match self.read().expect("the server hung up") {
                Ok(message) if message["type"] == kind => return message,
                _ => {}
            }
        }
    }

    /// Read until a terminal shows `text`.
    fn until_output(&mut self, text: &str) {
        let mut seen = String::new();
        while !seen.contains(text) {
            if let Err(output) = self.read().expect("the server hung up") {
                seen.push_str(&output);
            }
        }
    }
}

fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn running(pattern: &str) -> bool {
    Command::new("pgrep").args(["-f", pattern]).output().unwrap().status.success()
}

#[test]
fn keeps_terminals_across_an_upgrade_and_stops_them_on_close() {
    // Unix socket paths are short (104 bytes), so not the usual temp dir.
    let state = PathBuf::from(format!("/tmp/pulpo-test-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&state);
    let sock = state.join("pulpo/server/sock");
    let exe = env!("CARGO_BIN_EXE_pulpo");
    let mut server: Child = Command::new(exe)
        .arg("serve")
        .env("XDG_STATE_HOME", &state)
        .env("PULPO_SERVER_IDLE_SECS", "1")
        .env_remove("PULPO_TAB")
        .spawn()
        .unwrap();

    let mut conn = Conn::open(&sock);
    let pid = conn.until("hello")["pid"].clone();
    conn.until("ready");
    conn.send(json!({ "type": "spawn", "token": 7, "program": "/bin/sh", "args": [], "cwd": "/", "rows": 24, "cols": 80 }));
    let attach = conn.until("attach");
    assert_eq!(attach["token"], 7);
    let id = attach["info"]["id"].as_u64().unwrap();
    let shell = attach["info"]["pid"].clone();
    conn.input(id, "echo A$((1+1))\n");
    conn.until_output("A2");

    // Upgrade (to the same program): the connection drops, the terminal stays.
    conn.send(json!({ "type": "upgrade", "exe": exe }));
    assert!(std::iter::from_fn(|| conn.read()).count() < 1000, "the connection should end");
    let mut conn = Conn::open(&sock);
    assert_eq!(conn.until("hello")["pid"], pid, "the same process carries on");
    let attach = conn.until("attach");
    assert_eq!(attach["info"]["id"], id);
    assert_eq!(attach["info"]["pid"], shell);
    conn.until_output("A2");
    conn.input(id, "echo B$((1+2))\n");
    conn.until_output("B3");

    // Closing stops what runs in it, and then the idle server leaves.
    conn.input(id, "sleep 1234.5\n");
    wait_for("sleep to start", || running("sleep 1234.5"));

    // A paste it doesn't read holds up nothing else.
    conn.input(id, &"echo x\n".repeat(8 * 1024));
    conn.send(json!({ "type": "spawn", "token": 8, "program": "/bin/sh", "args": [], "cwd": "/", "rows": 24, "cols": 80 }));
    let other = conn.until("attach")["info"]["id"].as_u64().unwrap();
    conn.input(other, "echo C$((2+2))\n");
    conn.until_output("C4");
    conn.input(other, "exit\n");
    conn.until("exit");

    // A program printing as fast as it can waits for the server, which
    // doesn't pile up its output.
    let server_pid = pid.as_u64().unwrap().to_string();
    conn.send(json!({ "type": "spawn", "token": 9, "program": "/usr/bin/yes", "args": [], "cwd": "/", "rows": 24, "cols": 80 }));
    let yes = conn.until("attach")["info"]["id"].as_u64().unwrap();
    std::thread::sleep(Duration::from_secs(3));
    let rss = Command::new("ps").args(["-o", "rss=", "-p", &server_pid]).output().unwrap();
    let kb: u64 = String::from_utf8_lossy(&rss.stdout).trim().parse().unwrap();
    assert!(kb < 200 * 1024, "the server grew to {} MB", kb / 1024);
    // Not read meanwhile, this connection was dropped as too slow.
    drop(conn);
    let mut conn = Conn::open(&sock);
    conn.send(json!({ "type": "close", "id": yes }));
    conn.until("exit");

    // Closing interrupts the program first and gives it time to clean up.
    let cleaned = state.join("cleaned");
    conn.send(json!({ "type": "spawn", "token": 10, "program": "/bin/sh", "args": [], "cwd": "/", "rows": 24, "cols": 80 }));
    let polite = conn.until("attach")["info"]["id"].as_u64().unwrap();
    let job = format!("sh -c 'trap \"sleep 1; touch {}; exit\" INT; while :; do sleep 0.1; done'\n", cleaned.display());
    conn.input(polite, &job);
    wait_for("the job to start", || running(&format!("touch {}", cleaned.display())));
    conn.send(json!({ "type": "close", "id": polite }));
    conn.until("exit");
    assert!(cleaned.exists(), "hung up before it cleaned up");

    // Claude quits on a second ctrl-c, sent for it, rather than after the grace.
    // A shell by the name of claude, which quits on its second ctrl-c.
    let claude = state.join("claude");
    std::os::unix::fs::symlink("/bin/sh", &claude).unwrap();
    conn.send(json!({ "type": "spawn", "token": 11, "program": "/bin/sh", "args": [], "cwd": "/", "rows": 24, "cols": 80 }));
    let agent = conn.until("attach")["info"]["id"].as_u64().unwrap();
    let quits = "trap \"n=\\$((n+1)); [ \\$n = 2 ] && exit\" INT; while :; do sleep 0.1; done";
    conn.input(agent, &format!("{} -c '{quits}'\n", claude.display()));
    wait_for("claude to start", || running(&claude.display().to_string()));
    let closed = Instant::now();
    conn.send(json!({ "type": "close", "id": agent }));
    conn.until("exit");
    assert!(closed.elapsed() < Duration::from_secs(5), "claude waited out the grace");

    conn.send(json!({ "type": "close", "id": id }));
    conn.until("exit");
    wait_for("sleep to stop", || !running("sleep 1234.5"));
    drop(conn);
    wait_for("the server to leave", || server.try_wait().unwrap().is_some());
    assert!(!sock.exists());
    let _ = std::fs::remove_dir_all(&state);
}
