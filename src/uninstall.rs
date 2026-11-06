//! `pulpo uninstall`: the uninstall steps in the docs, in their order.

use std::io::{BufRead, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::agents::{self, shorten};
use crate::proto::{self, Frame, ToClient, ToServer};
use crate::worktree;

pub fn uninstall() -> Result<()> {
    if std::env::var_os("PULPO_TAB").is_some() {
        bail!("run this outside Pulpo: it stops every terminal, this one too");
    }
    if open_elsewhere() {
        bail!("quit Pulpo first: while it's open, it starts its server again");
    }
    let workspaces = worktree::all();
    let folders = folders();

    println!("This removes Pulpo:");
    println!("  · stops every terminal");
    for wt in &workspaces {
        let lost = worktree::unsaved(wt);
        let lost = if lost.is_empty() { String::new() } else { format!(", losing {}", lost.join(" and ")) };
        let kept = worktree::kept_branch(wt).map(|b| format!("; branch {b} stays")).unwrap_or_default();
        println!("  · deletes workspace {}: its folder, containers and database{lost}{kept}", wt.name);
    }
    println!("  · removes the hooks from Claude Code and Codex");
    for folder in folders.iter().filter(|f| f.exists()) {
        println!("  · deletes {}", shorten(folder));
    }
    println!("  · uninstalls the pulpo command");
    print!("Go ahead? [y/N] ");
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().lock().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        println!("Nothing changed.");
        return Ok(());
    }

    let stopped: usize = ["server", "server-nested"].iter().map(|dir| stop_terminals(&agents::state_dir().join(dir).join("sock"))).sum();
    println!("Stopped {stopped} terminal(s).");
    let exe = std::env::current_exe()?;
    for wt in &workspaces {
        let script = worktree::removal_script(wt, &exe);
        let dir = if wt.repo.is_dir() { wt.repo.as_path() } else { Path::new("/") };
        let removed = Command::new("sh").args(["-c", &script]).current_dir(dir).envs(worktree::env(wt)).stdin(Stdio::null()).status();
        if !removed.is_ok_and(|s| s.success()) {
            bail!("stopped before removing the rest: fix what's above, then run pulpo uninstall again");
        }
    }
    agents::hooks(&["remove".to_string()])?;
    for folder in folders.iter().filter(|f| f.exists()) {
        std::fs::remove_dir_all(folder)?;
        println!("Deleted {}.", shorten(folder));
    }
    if let Err(e) = std::fs::remove_file(&exe) {
        bail!("the pulpo command is still there: delete {} ({e})", exe.display());
    }
    println!("Pulpo is uninstalled.");
    Ok(())
}

/// The workspaces, the settings and the state.
fn folders() -> Vec<PathBuf> {
    let parent = |path: PathBuf| path.parent().map(Path::to_path_buf).unwrap_or_default();
    vec![parent(worktree::workspaces_dir()), parent(crate::keymap::path()), agents::state_dir()]
}

/// A Pulpo window, not a command like this one.
fn open_elsewhere() -> bool {
    let me = std::process::id().to_string();
    let Ok(out) = Command::new("ps").args(["-A", "-o", "pid=,args="]).output() else { return false };
    String::from_utf8_lossy(&out.stdout).lines().any(|line| {
        let mut words = line.split_whitespace();
        let (Some(pid), Some(exe)) = (words.next(), words.next()) else { return false };
        let command = matches!(
            words.next(),
            Some("serve" | "emit" | "forget-workspace" | "hooks" | "doctor" | "keytest" | "update" | "uninstall" | "-h" | "--help")
        );
        pid != me && Path::new(exe).file_name().is_some_and(|name| name == "pulpo") && !command
    })
}

/// Closes each terminal as quitting with Q does, waits for them to end, then stops the server.
fn stop_terminals(socket: &Path) -> usize {
    let Ok(mut stream) = UnixStream::connect(socket) else { return 0 };
    // The slowest stop: 10s for the program to quit, then hang-up, terminate and kill.
    let deadline = Instant::now() + Duration::from_secs(30);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.write_all(&proto::encode(&ToServer::Hello { protocol: proto::PROTOCOL }));
    let (mut server, mut open, mut closed) = (None, Vec::new(), None);
    while Instant::now() < deadline {
        let Ok(frame) = proto::read_frame(&mut stream) else { break };
        let Frame::Json(json) = frame else { continue };
        match serde_json::from_slice::<ToClient>(&json) {
            Ok(ToClient::Hello { protocol, pid, .. }) => {
                server = Some(pid);
                // Another build's server: it won't list its terminals, so it's only stopped.
                if protocol != proto::PROTOCOL {
                    break;
                }
            }
            Ok(ToClient::Attach { info, .. }) => open.push(info.id),
            Ok(ToClient::Ready) => {
                for &id in &open {
                    let _ = stream.write_all(&proto::encode(&ToServer::Close { id }));
                }
                closed = Some(open.len());
            }
            Ok(ToClient::Exit { id }) => open.retain(|&o| o != id),
            _ => {}
        }
        if closed.is_some() && open.is_empty() {
            break;
        }
    }
    if let Some(pid) = server {
        unsafe { libc::kill(pid as i32, libc::SIGTERM) };
    }
    closed.unwrap_or(0)
}
