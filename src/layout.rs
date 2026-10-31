//! Where you were, kept across restarts of Pulpo and of the machine: the
//! session on screen, the session last used in each repository, the
//! terminal last on screen in each session, and every open terminal with
//! the Claude session running in it. Pulpo reopens where you were; after a
//! reboot (the server comes back without terminals), it opens the
//! terminals again and resumes their Claude sessions.
//!
//! Kept next to the server's socket, so a nested Pulpo has its own.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Layout {
    pub current: Option<PathBuf>,
    /// Repository → its session last used; session → the tag (`PULPO_TAB`)
    /// of its terminal last on screen.
    pub sessions: BTreeMap<PathBuf, PathBuf>,
    pub terminals: BTreeMap<PathBuf, String>,
    pub open: Vec<Open>,
}

/// A terminal that was open: where, and the Claude session alive in it.
#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Open {
    pub cwd: PathBuf,
    pub tag: String,
    pub claude: Option<String>,
}

fn path() -> PathBuf {
    crate::server::dir().join("layout.json")
}

pub fn load() -> Layout {
    fs::read(path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn save(layout: &Layout) {
    let path = path();
    let Some(dir) = path.parent() else { return };
    let tmp = path.with_extension("json.tmp");
    let written = fs::create_dir_all(dir).and_then(|()| fs::write(&tmp, serde_json::to_vec_pretty(layout).unwrap_or_default()));
    if written.is_ok() {
        let _ = fs::rename(&tmp, &path);
    }
}

/// The command that brings a terminal back: Claude resumed where it was,
/// then a shell when it exits; or just a shell.
pub fn reopen(open: &Open, shell: &str) -> Vec<String> {
    match &open.claude {
        // Ids come from Claude's hooks; keep anything else out of the shell.
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
            vec!["-l".into(), "-c".into(), format!("claude --resume {id}; exec {shell} -l")]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumes_claude_then_leaves_a_shell() {
        let open = |claude: Option<&str>| Open { cwd: "/code".into(), tag: "1-2".into(), claude: claude.map(String::from) };
        assert_eq!(reopen(&open(Some("0f1e-a2")), "/bin/zsh")[2], "claude --resume 0f1e-a2; exec /bin/zsh -l");
        assert!(reopen(&open(None), "/bin/zsh").is_empty());
        // Nothing but an id gets into the command.
        assert!(reopen(&open(Some("x; rm -rf ~")), "/bin/zsh").is_empty());
    }
}
