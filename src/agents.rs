//! Agent status from Claude Code hooks.
//!
//! Hooks run `pulpo emit`, which appends one JSON line per event to
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
    base.join("pulpo")
}

fn log_path() -> PathBuf {
    state_dir().join("events.jsonl")
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// `pulpo emit`: read a hook payload from stdin and append it to the log.
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
        "tab": std::env::var("PULPO_TAB").unwrap_or_default(),
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

/// The Claude Code hook events Pulpo listens to, with their tool matcher.
const HOOKS: [(&str, Option<&str>); 7] = [
    ("SessionStart", None),
    ("UserPromptSubmit", None),
    ("PreToolUse", Some("*")),
    ("PostToolUse", Some("*")),
    ("Notification", None),
    ("Stop", None),
    ("SessionEnd", None),
];

fn emit_command() -> anyhow::Result<String> {
    Ok(format!("{} emit", std::env::current_exe()?.display()))
}

/// A hook command Pulpo installed, from whichever build: `.../pulpo emit`.
fn is_ours(handler: &Value) -> bool {
    handler.get("command").and_then(Value::as_str).is_some_and(|c| c.trim_end().ends_with("pulpo emit"))
}

fn hook_entry(command: &str, matcher: Option<&str>) -> Value {
    let mut entry = json!({ "hooks": [{ "type": "command", "command": command }] });
    if let Some(m) = matcher {
        entry["matcher"] = json!(m);
    }
    entry
}

/// `pulpo hooks [install|status|remove] [--dry-run]`.
pub fn hooks(args: &[String]) -> anyhow::Result<()> {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let path = claude_settings();
    let shown = shorten(&path);
    match args.first().map(String::as_str) {
        None => {
            let command = emit_command()?;
            let hooks: serde_json::Map<String, Value> =
                HOOKS.iter().map(|&(event, matcher)| (event.to_string(), json!([hook_entry(&command, matcher)]))).collect();
            println!("{}", serde_json::to_string_pretty(&json!({ "hooks": hooks }))?);
        }
        Some("status") => {
            let (_, settings) = read_settings(&path)?;
            println!("Claude Code  {shown}");
            for (event, state) in hook_status(&settings, &emit_command()?) {
                println!("  {event:<17} {state}");
            }
        }
        Some(action @ ("install" | "remove")) => {
            let (text, mut settings) = read_settings(&path)?;
            let command = (action == "install").then(emit_command).transpose()?;
            let changes = set_hooks(&mut settings, command.as_deref());
            println!("Claude Code  {shown}{}", if dry_run { "  (dry run, nothing written)" } else { "" });
            for (event, change) in &changes {
                println!("  {event:<17} {change}");
            }
            if !dry_run && changes.iter().any(|(_, c)| !matches!(*c, "unchanged" | "not installed")) {
                write_settings(&path, &text, &settings)?;
            }
            if action == "install" && !dry_run {
                println!("New Claude sessions started in Pulpo now report what they run and ask.");
            }
        }
        Some(other) => anyhow::bail!("unknown hooks command {other:?}: use install, status or remove"),
    }
    Ok(())
}

/// Pulpo's hooks are in Claude's settings, from this build or another.
pub fn hooks_installed() -> bool {
    let Ok(command) = emit_command() else { return false };
    read_settings(&claude_settings()).is_ok_and(|(_, s)| hook_status(&s, &command).iter().all(|(_, state)| *state != "missing"))
}

fn hook_status(settings: &Value, command: &str) -> Vec<(&'static str, &'static str)> {
    let mut installed = settings.clone();
    let changes = set_hooks(&mut installed, Some(command));
    changes
        .into_iter()
        .map(|(event, change)| {
            let state = match change {
                "unchanged" => "current",
                "updated" => "outdated",
                _ => "missing",
            };
            (event, state)
        })
        .collect()
}

/// Put Pulpo's hooks in `settings` (or take them out, with no command), and
/// leave every other hook as it was. Says what changed for each event.
fn set_hooks(settings: &mut Value, command: Option<&str>) -> Vec<(&'static str, &'static str)> {
    let mut changes = Vec::new();
    if !settings.is_object() {
        *settings = json!({});
    }
    if !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }
    let hooks = settings["hooks"].as_object_mut().expect("just made an object");
    for (event, matcher) in HOOKS {
        let before = hooks.get(event).cloned();
        let mut groups: Vec<Value> = before.as_ref().and_then(Value::as_array).cloned().unwrap_or_default();
        let mut had_ours = false;
        groups.retain_mut(|group| {
            let Some(handlers) = group.get_mut("hooks").and_then(Value::as_array_mut) else { return true };
            let count = handlers.len();
            handlers.retain(|h| !is_ours(h));
            had_ours |= handlers.len() < count;
            // A group that held only Pulpo's hook goes with it.
            !(handlers.is_empty() && count > 0)
        });
        if let Some(command) = command {
            groups.push(hook_entry(command, matcher));
        }
        let after = (!groups.is_empty()).then(|| Value::Array(groups));
        let change = match (after == before, had_ours, command.is_some()) {
            (true, _, true) => "unchanged",
            (_, false, true) => "added",
            (_, true, true) => "updated",
            (_, true, false) => "removed",
            (_, false, false) => "not installed",
        };
        match after {
            Some(groups) => hooks.insert(event.to_string(), groups),
            None => hooks.remove(event),
        };
        changes.push((event, change));
    }
    if hooks.is_empty() {
        settings.as_object_mut().expect("an object").remove("hooks");
    }
    changes
}

/// `$CLAUDE_CONFIG_DIR/settings.json`, or `~/.claude/settings.json`.
fn claude_settings() -> PathBuf {
    let dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".claude"));
    dir.join("settings.json")
}

fn shorten(path: &std::path::Path) -> String {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    match path.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

/// The file's text and its JSON; a missing file is empty settings.
fn read_settings(path: &std::path::Path) -> anyhow::Result<(String, Value)> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value = serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", path.display()))?;
            Ok((text, value))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((String::new(), json!({}))),
        Err(e) => Err(anyhow::anyhow!("cannot read {}: {e}", path.display())),
    }
}

/// Write the settings back with the file's own indentation, through a
/// temporary file so an interrupted write never leaves half a file. A
/// symlinked settings file (dotfiles) is written where it points.
fn write_settings(path: &std::path::Path, old_text: &str, settings: &Value) -> anyhow::Result<()> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(dir) = target.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = format_json(settings, &indent_of(old_text))?;
    let tmp = target.with_extension("json.pulpo-tmp");
    fs::write(&tmp, text)?;
    fs::rename(&tmp, &target)?;
    Ok(())
}

/// The indentation of the first indented line, two spaces if none.
fn indent_of(text: &str) -> String {
    text.lines()
        .map(|line| &line[..line.len() - line.trim_start().len()])
        .find(|indent| !indent.is_empty())
        .unwrap_or("  ")
        .to_string()
}

fn format_json(value: &Value, indent: &str) -> anyhow::Result<String> {
    let mut out = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(indent.as_bytes());
    let mut serializer = serde_json::Serializer::with_formatter(&mut out, formatter);
    serde::Serialize::serialize(value, &mut serializer)?;
    out.push(b'\n');
    Ok(String::from_utf8(out)?)
}

#[derive(Clone, Copy, PartialEq)]
pub enum State {
    Idle,
    Running,
    Waiting,
    Done,
}

pub struct Agent {
    /// The Pulpo terminal running this session (`PULPO_TAB`), if any.
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

    /// The latest session reported from a Pulpo terminal.
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

    #[test]
    fn installs_and_removes_only_its_hooks() {
        let original = r#"{
    "model": "opus",
    "hooks": {
        "UserPromptSubmit": [
            {
                "hooks": []
            }
        ],
        "Stop": [
            {
                "hooks": [
                    {
                        "type": "command",
                        "command": "say done"
                    }
                ]
            }
        ]
    },
    "theme": "dark"
}
"#;
        let mut settings: Value = serde_json::from_str(original).unwrap();
        let changes = set_hooks(&mut settings, Some("/old/pulpo emit"));
        assert!(changes.iter().all(|(_, c)| *c == "added"));
        // Again from a new build: updated in place, then unchanged.
        let changes = set_hooks(&mut settings, Some("/new/pulpo emit"));
        assert!(changes.iter().all(|(_, c)| *c == "updated"));
        assert!(set_hooks(&mut settings, Some("/new/pulpo emit")).iter().all(|(_, c)| *c == "unchanged"));
        assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert_eq!(settings["hooks"]["Stop"][1]["hooks"][0]["command"], "/new/pulpo emit");
        assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], "*");

        // Removing puts the file back as it was, key order and indentation included.
        assert!(set_hooks(&mut settings, None).iter().all(|(_, c)| *c == "removed"));
        assert_eq!(format_json(&settings, &indent_of(original)).unwrap(), original);
        assert!(set_hooks(&mut settings, None).iter().all(|(_, c)| *c == "not installed"));
    }
}
