//! Agent status from Claude Code hooks.
//!
//! Hooks run `ws emit`, which appends one JSON line per event to
//! `events.jsonl`. The TUI replays the file on start and tails it afterwards.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state"));
    base.join("ws")
}

fn log_path() -> PathBuf {
    state_dir().join("events.jsonl")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `ws emit`: read a hook payload from stdin and append it to the log.
/// Never fails loudly, since a failing hook would get in the agent's way.
pub fn emit() {
    let mut input = String::new();
    let _ = io::stdin().read_to_string(&mut input);
    let Ok(payload) = serde_json::from_str::<Value>(&input) else { return };
    let field = |key: &str| payload.get(key).and_then(Value::as_str).unwrap_or("");
    let record = json!({
        "ts": now(),
        "session": field("session_id"),
        "cwd": field("cwd"),
        "event": field("hook_event_name"),
        "tool": field("tool_name"),
        "message": field("message"),
        "tab": std::env::var("WS_TAB").unwrap_or_default(),
    });

    let path = log_path();
    let _ = fs::create_dir_all(state_dir());
    if fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES) {
        let _ = fs::rename(&path, path.with_extension("jsonl.old"));
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(format!("{record}\n").as_bytes());
    }
}

/// `ws hooks`: the Claude Code settings snippet that wires up `ws emit`.
pub fn print_hooks() -> anyhow::Result<()> {
    let command = format!("{} emit", std::env::current_exe()?.display());
    let hook = |matcher: Option<&str>| {
        let mut entry = json!({ "hooks": [{ "type": "command", "command": command }] });
        if let Some(m) = matcher {
            entry["matcher"] = json!(m);
        }
        json!([entry])
    };
    let snippet = json!({ "hooks": {
        "SessionStart": hook(None),
        "UserPromptSubmit": hook(None),
        "PreToolUse": hook(Some("*")),
        "PostToolUse": hook(Some("*")),
        "Notification": hook(None),
        "Stop": hook(None),
        "SessionEnd": hook(None),
    }});
    println!("{}", serde_json::to_string_pretty(&snippet)?);
    Ok(())
}

#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Idle,
    Running,
    Waiting,
    Done,
}

pub struct Agent {
    /// The ws terminal running this session (`WS_TAB`), if any.
    pub tab: String,
    pub state: State,
    pub detail: String,
    pub ts: u64,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Record {
    ts: u64,
    session: String,
    tab: String,
    event: String,
    tool: String,
    message: String,
}

#[derive(Default)]
pub struct Agents {
    offset: u64,
    sessions: HashMap<String, Agent>,
}

impl Agents {
    /// Read any lines appended since the last call.
    pub fn load(&mut self) {
        let Ok(mut file) = File::open(log_path()) else { return };
        let len = file.metadata().map(|m| m.len()).unwrap_or(0);
        if len < self.offset {
            // The log was rotated: start over.
            self.offset = 0;
            self.sessions.clear();
        }
        let mut buf = String::new();
        if file.seek(SeekFrom::Start(self.offset)).is_err() || file.read_to_string(&mut buf).is_err() {
            return;
        }
        // Leave a partially written last line for the next read.
        let complete = buf.rfind('\n').map_or(0, |i| i + 1);
        for line in buf[..complete].lines() {
            if let Ok(record) = serde_json::from_str::<Record>(line) {
                self.apply(record);
            }
        }
        self.offset += complete as u64;
    }

    fn apply(&mut self, r: Record) {
        if r.event == "SessionEnd" {
            self.sessions.remove(&r.session);
            return;
        }
        let agent = self.sessions.entry(r.session).or_insert_with(|| Agent {
            tab: r.tab.clone(),
            state: State::Idle,
            detail: String::new(),
            ts: r.ts,
        });
        agent.ts = r.ts;
        let (state, detail) = match r.event.as_str() {
            "SessionStart" => (State::Idle, String::new()),
            "UserPromptSubmit" => (State::Running, "thinking".into()),
            "PreToolUse" | "PostToolUse" => (State::Running, r.tool),
            "PreCompact" => (State::Running, "compacting".into()),
            "Notification" => (State::Waiting, r.message),
            "Stop" => (State::Done, String::new()),
            _ => return,
        };
        agent.state = state;
        agent.detail = detail;
    }

    /// The latest session reported from a ws terminal.
    pub fn for_tab(&self, tab: &str) -> Option<&Agent> {
        self.sessions.values().filter(|a| a.tab == tab).max_by_key(|a| a.ts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(session: &str, event: &str, tool: &str) -> Record {
        let (session, tab, event, tool) = (session.into(), "t1".into(), event.into(), tool.into());
        Record { ts: now(), session, tab, event, tool, ..Default::default() }
    }

    #[test]
    fn tracks_session_lifecycle() {
        let mut agents = Agents::default();
        agents.apply(record("a", "SessionStart", ""));
        agents.apply(record("a", "PreToolUse", "Bash"));
        let agent = agents.for_tab("t1").unwrap();
        assert!(agent.state == State::Running);
        assert_eq!(agent.detail, "Bash");
        agents.apply(record("a", "Stop", ""));
        assert!(agents.for_tab("t1").unwrap().state == State::Done);
        agents.apply(record("a", "SessionEnd", ""));
        assert!(agents.for_tab("t1").is_none());
    }
}
