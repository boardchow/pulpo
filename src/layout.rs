//! Kept next to the server's socket, so a nested Pulpo has its own.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Layout {
    pub current: Option<PathBuf>,
    pub workspaces: BTreeMap<PathBuf, PathBuf>,
    pub terminals: BTreeMap<PathBuf, String>,
    pub open: Vec<Open>,
}

#[derive(Serialize, Deserialize, Default, Clone, PartialEq, Debug)]
#[serde(default)]
pub struct Open {
    pub cwd: PathBuf,
    pub tag: String,
    #[serde(alias = "claude")]
    pub session: Option<String>,
    /// Its agent's name; none for Claude, from before Pulpo knew others.
    pub agent: String,
    /// Waiting for you when Pulpo closed.
    pub unread: bool,
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

pub fn reopen(open: &Open, shell: &str) -> Vec<String> {
    match &open.session {
        // Ids come from the agents' hooks; keep anything else out of the shell.
        Some(id) if !id.is_empty() && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') => {
            let resume = crate::agents::kind(&open.agent).resume;
            vec!["-l".into(), "-c".into(), format!("{resume} {id}; exec {shell} -l")]
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resumes_the_agent_then_leaves_a_shell() {
        let open = |session: Option<&str>| Open { cwd: "/code".into(), tag: "1-2".into(), session: session.map(String::from), ..Default::default() };
        assert_eq!(reopen(&open(Some("0f1e-a2")), "/bin/zsh")[2], "claude --resume 0f1e-a2; exec /bin/zsh -l");
        let codex = Open { agent: "codex".into(), ..open(Some("0f1e-a2")) };
        assert_eq!(reopen(&codex, "/bin/zsh")[2], "codex resume 0f1e-a2; exec /bin/zsh -l");
        assert!(reopen(&open(None), "/bin/zsh").is_empty());
        assert!(reopen(&open(Some("x; rm -rf ~")), "/bin/zsh").is_empty());
    }
}
