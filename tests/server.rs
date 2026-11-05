mod common;

use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use common::{Conn, running, wait_for};
use serde_json::json;

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
    conn.watch(id);
    conn.input(id, "echo A$((1+1))\n");
    conn.until_output("A2");

    conn.send(json!({ "type": "upgrade", "exe": exe }));
    assert!(std::iter::from_fn(|| conn.read()).count() < 1000, "the connection should end");
    let mut conn = Conn::open(&sock);
    assert_eq!(conn.until("hello")["pid"], pid, "the same process carries on");
    let attach = conn.until("attach");
    assert_eq!(attach["info"]["id"], id);
    assert_eq!(attach["info"]["pid"], shell);
    conn.watch(id);
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
    conn.watch(other);
    conn.input(other, "echo C$((2+2))\n");
    conn.until_output("C4");
    conn.input(other, "exit\n");
    conn.until("exit");

    let server_pid = pid.as_u64().unwrap().to_string();
    conn.send(json!({ "type": "spawn", "token": 9, "program": "/usr/bin/yes", "args": [], "cwd": "/", "rows": 24, "cols": 80 }));
    let yes = conn.until("attach")["info"]["id"].as_u64().unwrap();
    conn.watch(yes);
    std::thread::sleep(Duration::from_secs(3));
    let rss = Command::new("ps").args(["-o", "rss=", "-p", &server_pid]).output().unwrap();
    let kb: u64 = String::from_utf8_lossy(&rss.stdout).trim().parse().unwrap();
    assert!(kb < 200 * 1024, "the server grew to {} MB", kb / 1024);
    // Not read meanwhile, this connection was dropped as too slow.
    drop(conn);
    let mut conn = Conn::open(&sock);
    conn.send(json!({ "type": "close", "id": yes }));
    conn.until("exit");

    let cleaned = state.join("cleaned");
    conn.send(json!({ "type": "spawn", "token": 10, "program": "/bin/sh", "args": [], "cwd": "/", "rows": 24, "cols": 80 }));
    let polite = conn.until("attach")["info"]["id"].as_u64().unwrap();
    let job = format!("sh -c 'trap \"sleep 1; touch {}; exit\" INT; while :; do sleep 0.1; done'\n", cleaned.display());
    conn.input(polite, &job);
    wait_for("the job to start", || running(&format!("touch {}", cleaned.display())));
    conn.send(json!({ "type": "close", "id": polite }));
    conn.until("exit");
    assert!(cleaned.exists(), "hung up before it cleaned up");

    // Claude quits on a second ctrl-c, sent for it; here a shell named claude stands in.
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
