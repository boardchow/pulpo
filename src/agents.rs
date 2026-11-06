//! Agent status from Claude Code and Codex hooks: `pulpo emit` appends one JSON line per event to `events.jsonl`.

use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;
use serde_json::{Value, json};

const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

#[cfg(test)]
thread_local! {
    /// Per test thread: setting `XDG_STATE_HOME` while other test threads read it isn't safe.
    pub static TEST_STATE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

pub fn state_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = TEST_STATE.with_borrow(Clone::clone) {
        return dir;
    }
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

pub struct Kind {
    /// The command that starts it, and what `pulpo hooks install <name>` takes.
    pub name: &'static str,
    pub title: &'static str,
    /// The variable that moves its config folder, and the folder under home otherwise.
    dir: (&'static str, &'static str),
    file: &'static str,
    hooks: &'static [(&'static str, Option<&'static str>)],
    pub resume: &'static str,
    installed: &'static str,
}

pub static KINDS: [Kind; 2] = [
    Kind {
        name: "claude",
        title: "Claude Code",
        dir: ("CLAUDE_CONFIG_DIR", ".claude"),
        file: "settings.json",
        hooks: &[
            ("SessionStart", None),
            ("UserPromptSubmit", None),
            ("PreToolUse", Some("*")),
            ("PostToolUse", Some("*")),
            ("Notification", None),
            ("Stop", None),
            ("SessionEnd", None),
        ],
        resume: "claude --resume",
        installed: "New Claude sessions started in Pulpo now report what they run and ask.",
    },
    Kind {
        name: "codex",
        title: "Codex",
        dir: ("CODEX_HOME", ".codex"),
        file: "hooks.json",
        // No matcher: it's a regex in Codex, and none matches every tool.
        hooks: &[
            ("SessionStart", None),
            ("UserPromptSubmit", None),
            ("PreToolUse", None),
            ("PostToolUse", None),
            ("PermissionRequest", None),
            ("Interrupt", None),
            ("Stop", None),
            ("SessionEnd", None),
        ],
        resume: "codex resume",
        installed: "Codex runs new hooks once you trust them: in Codex, type /hooks and trust Pulpo's.",
    },
];

/// Events from before Pulpo knew other agents name none: they're Claude's.
pub fn kind(name: &str) -> &'static Kind {
    KINDS.iter().find(|k| k.name == name).unwrap_or(&KINDS[0])
}

impl Kind {
    fn settings(&self) -> PathBuf {
        let (var, dir) = self.dir;
        let dir = std::env::var_os(var).map(PathBuf::from).unwrap_or_else(|| home().join(dir));
        dir.join(self.file)
    }

    /// Installed, or used before: on the PATH, or its config folder exists.
    pub fn found(&self) -> bool {
        let on_path = std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(self.name).is_file()));
        on_path || self.settings().parent().is_some_and(Path::is_dir)
    }

    pub fn installed(&self) -> bool {
        let Ok(command) = emit_command(self) else { return false };
        read_settings(&self.settings()).is_ok_and(|(_, s)| hook_status(self, &s, &command).iter().all(|(_, state)| *state != "missing"))
    }
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// Never fails loudly, since a failing hook would get in the agent's way.
pub fn emit(agent: Option<&str>) {
    let mut input = String::new();
    let _ = io::stdin().read_to_string(&mut input);
    let Ok(payload) = serde_json::from_str::<Value>(&input) else { return };
    let field = |key: &str| payload.get(key).and_then(Value::as_str).unwrap_or("");
    let text = |pointer: &str| payload.pointer(pointer).and_then(Value::as_str);
    let message = match field("hook_event_name") {
        "PermissionRequest" => text("/tool_input/description").unwrap_or(""),
        _ => field("message"),
    };
    let file = text("/tool_input/file_path")
        .or_else(|| text("/tool_input/notebook_path"))
        .map(String::from)
        .or_else(|| {
            let patch = text("/tool_input/command").filter(|_| field("tool_name") == "apply_patch")?;
            Some(Path::new(field("cwd")).join(patched_file(patch)?).display().to_string())
        })
        .unwrap_or_default();
    let record = json!({
        "ts": now(),
        "agent": agent.unwrap_or("claude"),
        "session": field("session_id"),
        "cwd": field("cwd"),
        "event": field("hook_event_name"),
        "tool": field("tool_name"),
        "message": message,
        "file": file,
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

/// The last file an `apply_patch` (Codex's edit) adds or updates.
fn patched_file(patch: &str) -> Option<&str> {
    patch.lines().rev().find_map(|l| l.strip_prefix("*** Update File: ").or_else(|| l.strip_prefix("*** Add File: "))).map(str::trim)
}

fn emit_command(kind: &Kind) -> anyhow::Result<String> {
    let exe = std::env::current_exe()?;
    // Claude's came first: it stays `pulpo emit`, so hooks installed then stay current.
    Ok(match kind.name {
        "claude" => format!("{} emit", exe.display()),
        name => format!("{} emit {name}", exe.display()),
    })
}

/// Any build's `.../pulpo emit [agent]`, not only this one's.
fn is_ours(handler: &Value) -> bool {
    handler.get("command").and_then(Value::as_str).is_some_and(|c| {
        let c = c.trim_end();
        c.ends_with("pulpo emit") || KINDS.iter().any(|k| c.ends_with(&format!("pulpo emit {}", k.name)))
    })
}

fn hook_entry(command: &str, matcher: Option<&str>) -> Value {
    let mut entry = json!({ "hooks": [{ "type": "command", "command": command }] });
    if let Some(m) = matcher {
        entry["matcher"] = json!(m);
    }
    entry
}

/// `pulpo hooks [install|status|remove] [agent] [--dry-run]`; without an agent, every one found.
pub fn hooks(args: &[String]) -> anyhow::Result<()> {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let mut words = args.iter().map(String::as_str).filter(|a| *a != "--dry-run").peekable();
    let action = words.next_if(|w| matches!(*w, "install" | "status" | "remove"));
    let names: Vec<&str> = KINDS.iter().map(|k| k.name).collect();
    let named: Vec<&str> = words.collect();
    let kinds: Vec<&Kind> = match named.as_slice() {
        [] => KINDS
            .iter()
            .filter(|k| match action {
                None => true,
                Some("remove") => k.settings().exists(),
                _ => k.found(),
            })
            .collect(),
        [name] => match KINDS.iter().find(|k| k.name == *name) {
            Some(kind) => vec![kind],
            None => anyhow::bail!("unknown hooks command {name:?}: use install, status or remove, then {}", names.join(" or ")),
        },
        _ => anyhow::bail!("name one agent at most: {}", names.join(" or ")),
    };
    if named.is_empty() && matches!(action, Some("install" | "status")) {
        for kind in KINDS.iter().filter(|k| !k.found()) {
            println!("{}  not found, skipped", kind.title);
        }
    }
    for kind in kinds {
        let path = kind.settings();
        let shown = shorten(&path);
        let command = emit_command(kind)?;
        match action {
            None => {
                let hooks: serde_json::Map<String, Value> =
                    kind.hooks.iter().map(|&(event, matcher)| (event.to_string(), json!([hook_entry(&command, matcher)]))).collect();
                println!("{}  {shown}", kind.title);
                println!("{}", serde_json::to_string_pretty(&json!({ "hooks": hooks }))?);
            }
            Some("status") => {
                let (_, settings) = read_settings(&path)?;
                println!("{}  {shown}", kind.title);
                for (event, state) in hook_status(kind, &settings, &command) {
                    println!("  {event:<18} {state}");
                }
            }
            Some(action) => {
                let (text, mut settings) = read_settings(&path)?;
                let install = action == "install";
                let changes = set_hooks(kind, &mut settings, install.then_some(command.as_str()));
                println!("{}  {shown}{}", kind.title, if dry_run { "  (dry run, nothing written)" } else { "" });
                for (event, change) in &changes {
                    println!("  {event:<18} {change}");
                }
                if !dry_run && changes.iter().any(|(_, c)| !matches!(*c, "unchanged" | "not installed")) {
                    write_settings(&path, &text, &settings)?;
                }
                if install && !dry_run {
                    println!("{}", kind.installed);
                }
            }
        }
    }
    Ok(())
}

/// For every agent found; true when none is.
pub fn hooks_installed() -> bool {
    KINDS.iter().filter(|k| k.found()).all(Kind::installed)
}

fn hook_status(kind: &Kind, settings: &Value, command: &str) -> Vec<(&'static str, &'static str)> {
    let mut installed = settings.clone();
    let changes = set_hooks(kind, &mut installed, Some(command));
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

fn set_hooks(kind: &Kind, settings: &mut Value, command: Option<&str>) -> Vec<(&'static str, &'static str)> {
    let mut changes = Vec::new();
    if !settings.is_object() {
        *settings = json!({});
    }
    if !settings["hooks"].is_object() {
        settings["hooks"] = json!({});
    }
    let hooks = settings["hooks"].as_object_mut().expect("just made an object");
    for &(event, matcher) in kind.hooks {
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

pub fn shorten(path: &Path) -> String {
    match path.strip_prefix(home()) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn read_settings(path: &Path) -> anyhow::Result<(String, Value)> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let value = serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{} is not valid JSON: {e}", path.display()))?;
            Ok((text, value))
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok((String::new(), json!({}))),
        Err(e) => Err(anyhow::anyhow!("cannot read {}: {e}", path.display())),
    }
}

/// Atomic through a temp file; a symlinked settings file (dotfiles) is written where it points.
fn write_settings(path: &Path, old_text: &str, settings: &Value) -> anyhow::Result<()> {
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
    Ready,
    Failed,
}

impl State {
    pub fn urgency(self) -> u8 {
        match self {
            State::Ready | State::Failed => 3,
            State::Waiting | State::Done => 2,
            State::Running => 1,
            State::Idle => 0,
        }
    }
}

pub struct Agent {
    pub kind: &'static Kind,
    pub tab: String,
    pub state: State,
    pub detail: String,
    pub ts: u64,
    /// The file it last edited, numbered in the order of all edits.
    pub edited: Option<(u64, PathBuf)>,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct Record {
    ts: u64,
    agent: String,
    session: String,
    tab: String,
    event: String,
    tool: String,
    message: String,
    file: String,
}

#[derive(Default)]
pub struct Agents {
    offset: u64,
    sessions: HashMap<String, Agent>,
    edits: u64,
}

impl Agents {
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
            kind: kind(&r.agent),
            tab: r.tab.clone(),
            state: State::Idle,
            detail: String::new(),
            ts: r.ts,
            edited: None,
        });
        agent.ts = r.ts;
        if r.event == "PostToolUse" && !r.file.is_empty() && matches!(r.tool.as_str(), "Edit" | "Write" | "MultiEdit" | "NotebookEdit" | "apply_patch") {
            self.edits += 1;
            agent.edited = Some((self.edits, PathBuf::from(&r.file)));
        }
        let (state, detail) = match r.event.as_str() {
            "SessionStart" => (State::Idle, String::new()),
            "UserPromptSubmit" => (State::Running, "thinking".into()),
            "PreToolUse" | "PostToolUse" => (State::Running, r.tool),
            "PreCompact" => (State::Running, "compacting".into()),
            "Notification" => (State::Waiting, r.message),
            "PermissionRequest" if r.message.is_empty() => (State::Waiting, r.tool),
            "PermissionRequest" => (State::Waiting, r.message),
            "Interrupt" => (State::Idle, String::new()),
            "Stop" => (State::Done, String::new()),
            _ => return,
        };
        agent.state = state;
        agent.detail = detail;
    }

    pub fn session_for_tab(&self, tab: &str) -> Option<(&str, &'static Kind)> {
        self.sessions.iter().filter(|(_, a)| a.tab == tab).max_by_key(|(_, a)| a.ts).map(|(id, a)| (id.as_str(), a.kind))
    }

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
        let mut edit = record("a", "PostToolUse", "Edit");
        edit.file = "/code/src/main.rs".into();
        agents.apply(edit);
        assert_eq!(agents.for_tab("t1").unwrap().edited, Some((1, PathBuf::from("/code/src/main.rs"))));
        agents.apply(record("a", "Stop", ""));
        assert!(agents.for_tab("t1").unwrap().state == State::Done);
        agents.apply(record("a", "SessionEnd", ""));
        assert!(agents.for_tab("t1").is_none());
    }

    #[test]
    fn tracks_codex() {
        let mut agents = Agents::default();
        let mut start = record("c", "SessionStart", "");
        start.agent = "codex".into();
        agents.apply(start);
        let mut patch = record("c", "PostToolUse", "apply_patch");
        patch.file = "/code/src/lib.rs".into();
        agents.apply(patch);
        agents.apply(record("c", "PermissionRequest", "Bash"));
        let agent = agents.for_tab("t1").unwrap();
        assert!(agent.state == State::Waiting);
        assert_eq!(agent.detail, "Bash");
        assert_eq!(agent.edited, Some((1, PathBuf::from("/code/src/lib.rs"))));
        assert_eq!(agents.session_for_tab("t1").map(|(id, k)| (id, k.name)), Some(("c", "codex")));
        agents.apply(record("c", "Interrupt", ""));
        assert!(agents.for_tab("t1").unwrap().state == State::Idle);
    }

    #[test]
    fn reads_the_file_a_patch_edits() {
        let patch = "*** Begin Patch\n*** Update File: src/a.rs\n@@\n-x\n+y\n*** Add File: src/b.rs\n+z\n*** End Patch";
        assert_eq!(patched_file(patch), Some("src/b.rs"));
        assert_eq!(patched_file("*** Begin Patch\n*** Delete File: a\n*** End Patch"), None);
    }

    #[test]
    fn knows_each_agents_hooks() {
        let codex = json!({ "hooks": [{ "type": "command", "command": "/x/pulpo emit codex" }] });
        assert!(is_ours(&codex["hooks"][0]));
        assert!(!is_ours(&json!({ "command": "say pulpo emitted" })));
        let mut settings = json!({});
        set_hooks(&KINDS[1], &mut settings, Some("/x/pulpo emit codex"));
        assert!(settings["hooks"]["PreToolUse"][0].get("matcher").is_none());
        assert!(settings["hooks"]["PermissionRequest"].is_array());
        assert!(set_hooks(&KINDS[1], &mut settings, None).iter().all(|(_, c)| *c == "removed"));
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
        let changes = set_hooks(&KINDS[0], &mut settings, Some("/old/pulpo emit"));
        assert!(changes.iter().all(|(_, c)| *c == "added"));
        let changes = set_hooks(&KINDS[0], &mut settings, Some("/new/pulpo emit"));
        assert!(changes.iter().all(|(_, c)| *c == "updated"));
        assert!(set_hooks(&KINDS[0], &mut settings, Some("/new/pulpo emit")).iter().all(|(_, c)| *c == "unchanged"));
        assert_eq!(settings["hooks"]["Stop"][0]["hooks"][0]["command"], "say done");
        assert_eq!(settings["hooks"]["Stop"][1]["hooks"][0]["command"], "/new/pulpo emit");
        assert_eq!(settings["hooks"]["PreToolUse"][0]["matcher"], "*");

        assert!(set_hooks(&KINDS[0], &mut settings, None).iter().all(|(_, c)| *c == "removed"));
        assert_eq!(format_json(&settings, &indent_of(original)).unwrap(), original);
        assert!(set_hooks(&KINDS[0], &mut settings, None).iter().all(|(_, c)| *c == "not installed"));
    }
}
