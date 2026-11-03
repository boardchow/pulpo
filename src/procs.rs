//! What runs in a terminal: the processes descended from the one it
//! started.

use std::process::Command;

pub struct Proc {
    pub pid: i32,
    pub ppid: i32,
    pub pgid: i32,
}

/// Every process on the machine.
pub fn table() -> Vec<Proc> {
    let out = Command::new("ps").args(["-A", "-o", "pid=,ppid=,pgid="]).output();
    out.map(|o| parse_ps(&String::from_utf8_lossy(&o.stdout))).unwrap_or_default()
}

fn parse_ps(text: &str) -> Vec<Proc> {
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let mut number = || fields.next()?.parse().ok();
            Some(Proc { pid: number()?, ppid: number()?, pgid: number()? })
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
        let pids: Vec<i32> = tree(&table, 100).iter().map(|p| p.pid).collect();
        assert_eq!(pids, [100, 101, 102]);
        assert!(tree(&table, 999).is_empty());
    }

}
