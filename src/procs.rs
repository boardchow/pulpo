//! What runs in a terminal: the processes descended from the one it
//! started, and the network ports they listen on.

use std::process::Command;

pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
    pub name: String,
}

/// Every process on the machine.
pub fn table() -> Vec<Proc> {
    let out = Command::new("ps").args(["-A", "-o", "pid=,ppid=,pgid=,comm="]).output();
    out.map(|o| parse_ps(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

fn parse_ps(text: &str) -> Vec<Proc> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let mut number = || fields.next()?.parse().ok();
            let (pid, ppid, pgid) = (number()?, number()?, number()?);
            // The command is the rest, spaces included (an app's path).
            let command = fields.collect::<Vec<_>>().join(" ");
            let name = command.rsplit('/').next().unwrap_or("").trim_start_matches('-').to_string();
            Some(Proc { pid, ppid, pgid, name })
        })
        .collect()
}

/// `root` and everything it started, and they started, and so on.
pub fn tree(table: &[Proc], root: i32) -> Vec<&Proc> {
    let mut found: Vec<&Proc> = table.iter().filter(|p| p.pid == root).collect();
    let mut i = 0;
    while i < found.len() {
        let parent = found[i].pid;
        found.extend(table.iter().filter(|p| p.ppid == parent && p.pid != parent));
        i += 1;
    }
    found
}

/// The TCP ports these processes listen on, by process.
pub fn listening(pids: &[i32]) -> Vec<(i32, u16)> {
    if pids.is_empty() {
        return Vec::new();
    }
    let list = pids.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
    let out = Command::new("lsof").args(["-nP", "-a", "-iTCP", "-sTCP:LISTEN", "-p", &list, "-F", "pn"]).output();
    out.map(|o| parse_lsof(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

/// `lsof -F pn`: a `p<pid>` line, then names like `n*:3000` or `n[::1]:5432`.
fn parse_lsof(text: &str) -> Vec<(i32, u16)> {
    let mut pid = 0;
    let mut ports = Vec::new();
    for line in text.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().unwrap_or(0);
        } else if let Some(port) = line.strip_prefix('n').and_then(|n| n.rsplit(':').next()?.parse().ok()) {
            ports.push((pid, port));
        }
    }
    ports.sort_unstable();
    ports.dedup();
    ports
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_a_terminal_process_tree() {
        let table = parse_ps(
            "    1     0     1 /sbin/launchd
  100     1   100 -zsh
  101   100   101 /opt/homebrew/bin/node
  102   101   101 /Applications/Docker.app/Contents/Resources/bin/docker compose
  200     1   200 /bin/zsh
",
        );
        assert_eq!(table.len(), 5);
        assert_eq!(table[1].name, "zsh");
        assert_eq!(table[3].name, "docker compose");
        let pids: Vec<i32> = tree(&table, 100).iter().map(|p| p.pid).collect();
        assert_eq!(pids, [100, 101, 102]);
        assert!(tree(&table, 999).is_empty());
    }

    #[test]
    fn reads_listening_ports() {
        let text = "p84392\nf23\nn*:3000\nf24\nn[::1]:3000\np57335\nf9\nn127.0.0.1:5432\n";
        assert_eq!(parse_lsof(text), [(57335, 5432), (84392, 3000)]);
    }
}
