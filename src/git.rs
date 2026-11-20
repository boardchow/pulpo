use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Default)]
pub struct Status {
    pub branch: Option<String>,
}

fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git").args(args).current_dir(dir).output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn toplevel(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"]).ok().map(|out| PathBuf::from(out.trim()))
}

/// With an empty first commit, for workspaces to branch off.
pub fn init(dir: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    let branch = git(dir, &["config", "--get", "init.defaultBranch"]).map_or("main".into(), |b| b.trim().to_string());
    if let Err(e) = git(dir, &["init", "-q", "-b", &branch]) {
        let _ = std::fs::remove_dir(dir);
        return Err(format!("git init failed: {e}"));
    }
    // Without it, git makes up an email from the Mac's name.
    git(dir, &["-c", "user.useConfigOnly=true", "commit", "-q", "--allow-empty", "-m", "Initial commit"])
        .map(drop)
        .map_err(|e| format!("no first commit, so no workspaces yet: {}", e.lines().last().unwrap_or_default()))
}

pub fn main_repo(dir: &Path) -> Option<PathBuf> {
    let common = git(dir, &["rev-parse", "--path-format=absolute", "--git-common-dir"]).ok()?;
    let common = PathBuf::from(common.trim());
    match common.file_name() {
        Some(name) if name == ".git" => common.parent().map(Path::to_path_buf),
        _ => toplevel(dir),
    }
}

pub fn is_tracked(root: &Path, path: &Path) -> bool {
    git(root, &["ls-files", "--", &path.to_string_lossy()]).is_ok_and(|out| !out.trim().is_empty())
}

pub fn worktrees(repo: &Path) -> Vec<PathBuf> {
    git(repo, &["worktree", "list", "--porcelain"])
        .map(|out| out.lines().filter_map(|l| l.strip_prefix("worktree ")).map(PathBuf::from).collect())
        .unwrap_or_default()
}

pub fn blame(path: &Path) -> Vec<String> {
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return Vec::new() };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    git(dir, &["--no-optional-locks", "blame", "--porcelain", "--", &name.to_string_lossy()])
        .map(|out| parse_blame(&out, now))
        .unwrap_or_default()
}

fn parse_blame(out: &str, now: u64) -> Vec<String> {
    let mut commits: std::collections::HashMap<&str, (&str, u64, &str)> = std::collections::HashMap::new();
    let mut hash = "";
    let mut lines = Vec::new();
    for line in out.lines() {
        if line.starts_with('\t') {
            lines.push(match commits.get(hash) {
                _ if !hash.is_empty() && hash.bytes().all(|b| b == b'0') => "not committed yet".to_string(),
                Some((author, time, subject)) => format!("{author}, {} · {subject}", ago(now.saturating_sub(*time))),
                None => String::new(),
            });
            continue;
        }
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        let info = commits.entry(hash).or_default();
        match key {
            "author" => info.0 = value,
            "author-time" => info.1 = value.parse().unwrap_or(0),
            "summary" => info.2 = value,
            _ if matches!(key.len(), 40 | 64) && key.bytes().all(|b| b.is_ascii_hexdigit()) => hash = key,
            _ => {}
        }
    }
    lines
}

fn ago(secs: u64) -> String {
    let (n, unit) = match secs {
        0..60 => return "just now".into(),
        60..3600 => (secs / 60, "minute"),
        3600..86_400 => (secs / 3600, "hour"),
        86_400..1_209_600 => (secs / 86_400, "day"),
        1_209_600..5_184_000 => (secs / 604_800, "week"),
        5_184_000..31_536_000 => (secs / 2_592_000, "month"),
        _ => (secs / 31_536_000, "year"),
    };
    format!("{n} {unit}{} ago", if n == 1 { "" } else { "s" })
}

pub fn status(root: &Path) -> Status {
    let branch = git(root, &["symbolic-ref", "--short", "-q", "HEAD"]).ok().map(|b| b.trim().to_string());
    Status { branch: branch.filter(|b| !b.is_empty()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_makes_a_repository_in_a_new_folder() {
        let dir = std::env::temp_dir().join(format!("pulpo-init-{}", std::process::id())).join("new");
        match init(&dir) {
            Ok(()) => assert!(git(&dir, &["rev-parse", "HEAD"]).is_ok()),
            Err(e) => assert!(e.starts_with("no first commit")),
        }
        let branch = git(&dir, &["config", "--get", "init.defaultBranch"]).map_or("main".into(), |b| b.trim().to_string());
        assert_eq!(git(&dir, &["symbolic-ref", "--short", "HEAD"]).unwrap().trim(), branch);
        assert_eq!(main_repo(&dir).unwrap().canonicalize().unwrap(), dir.canonicalize().unwrap());
        std::fs::remove_dir_all(dir.parent().unwrap()).unwrap();
    }

    #[test]
    fn parses_blame() {
        let a = "a".repeat(40);
        let zero = "0".repeat(40);
        let out = format!(
            "{a} 1 1 2\nauthor Sam\nauthor-time 1000\nsummary Add status\nfilename x.rs\n\tone\n{a} 2 2\n\ttwo\n\
             {zero} 3 3 1\nauthor Not Committed Yet\nauthor-time 4000\nsummary Version of x.rs from x.rs\nfilename x.rs\n\tthree\n"
        );
        let now = 1000 + 3 * 86_400;
        assert_eq!(parse_blame(&out, now), ["Sam, 3 days ago · Add status", "Sam, 3 days ago · Add status", "not committed yet"]);
        assert_eq!(ago(30), "just now");
        assert_eq!(ago(3600), "1 hour ago");
        assert_eq!(ago(40 * 86_400), "5 weeks ago");
    }
}
