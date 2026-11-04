//! Isolated sessions: a checkout of a repository to work on one task, with
//! its own branch, ports, Docker containers and database, so several tasks
//! (and their agents) run side by side without touching each other.
//!
//! The checkout is a git worktree on a new branch, from the default branch
//! as on the remote (the way Conductor, Claude Code and Cursor do it).
//! Main's ignored files a session needs (`.env`, keys) are cloned into it
//! (`clonefile`: instant, no space until changed). Its terminals get a
//! block of ten ports, its own Compose project and a value for each port
//! variable of the compose file. Setting it up (main's images, a copy of
//! main's database) and removing it run as scripts in the background, with
//! a log. Sessions made before were APFS copies of the whole checkout
//! (`Kind::Copy`); they're still removed the way they were made.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use ignore::gitignore::GitignoreBuilder;
use serde::{Deserialize, Serialize};

/// Ports are handed out in blocks of this many, from `FIRST_PORT` up.
const FIRST_PORT: u16 = 20000;
const BLOCK: u16 = 10;
const COMPOSE_FILES: [&str; 4] = ["compose.yaml", "compose.yml", "docker-compose.yaml", "docker-compose.yml"];
const COMPOSE_OVERRIDES: [&str; 4] = ["compose.override.yaml", "compose.override.yml", "docker-compose.override.yaml", "docker-compose.override.yml"];
/// `clonefile`'s flag to clone a link as a link (sys/clonefile.h).
const CLONE_NOFOLLOW: u32 = 1;
/// What a session gets of main's ignored files when nothing says otherwise.
const DEFAULT_INCLUDE: [&str; 5] = [".env", ".env.*", "!.env.example", "!.env.sample", "!.env.template"];

/// How a session was made.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// An APFS copy of the whole checkout (sessions made before worktrees).
    #[default]
    Copy,
    Worktree,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Session {
    pub name: String,
    /// The main checkout it copies, and where the copy is.
    pub repo: PathBuf,
    pub root: PathBuf,
    /// Its Compose project, and main's.
    pub project: String,
    pub main_project: String,
    /// Its value for each port variable of the compose file.
    pub ports: Vec<(String, u16)>,
    /// Services whose image main built, tagged for this project too.
    pub images: Vec<String>,
    /// A copy's: the commit it was copied at, its changes being what came
    /// after. A worktree's changes are since it left the default branch.
    #[serde(default)]
    pub base: Option<String>,
    #[serde(default)]
    pub kind: Kind,
    /// The first of its ten ports (`PULPO_PORT`); the compose file's port
    /// variables take the ones after the block.
    #[serde(default)]
    pub port: Option<u16>,
    /// Its branch, whether Pulpo made it, and the commit it started at:
    /// a branch Pulpo made goes with the session if it's still there.
    #[serde(default)]
    pub branch: Option<String>,
    #[serde(default)]
    pub made_branch: bool,
    #[serde(default)]
    pub start: Option<String>,
    /// `COMPOSE_FILE` as made, when a service mounts its folder: its compose
    /// files, then one that mounts main's `.git`. Its terminals get it read
    /// again when they start (`env`).
    #[serde(default)]
    pub compose_file: Option<String>,
}

/// How sessions of one repository are set up, in
/// layers (`config`): guessed, Conductor's settings, then
/// `~/.config/pulpo/repos/<repo>.toml`.
#[derive(Default, Debug, PartialEq, Clone)]
pub struct Config {
    pub ports: Vec<String>,
    pub images: Vec<String>,
    /// mysql, postgres, or empty for none: how main's database is copied.
    pub db: String,
    pub db_service: String,
    /// Run when main's database isn't running to copy from.
    pub fresh: String,
    pub setup: String,
    pub teardown: String,
    /// Main's ignored files a session gets (gitignore patterns), besides its
    /// `.env` files and those in `.worktreeinclude`; `!pattern` takes one away.
    pub include: Vec<String>,
    /// What a session's branch starts with (`<prefix>/<name>`), when not
    /// git's name.
    pub branch_prefix: String,
    /// Whether `setup` and `teardown` came from a file committed to the
    /// repository: run only once trusted (`untrusted`).
    pub committed: (bool, bool),
}

/// One layer of settings: what it says. Pulpo's file says what's
/// uncommented, an empty value included (it turns a guess off).
#[derive(Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
struct Layer {
    ports: Option<Vec<String>>,
    images: Option<Vec<String>>,
    db: Option<String>,
    db_service: Option<String>,
    fresh: Option<String>,
    setup: Option<String>,
    teardown: Option<String>,
    include: Option<Vec<String>>,
    branch_prefix: Option<String>,
}

/// What Pulpo reads of Conductor's settings (`.conductor/settings.toml`,
/// `settings.local.toml`): the rest is Conductor's.
#[derive(Deserialize, Default)]
#[serde(default)]
struct Conductor {
    /// Ignored files a workspace gets, one string (comma or line separated).
    file_include_globs: String,
    scripts: ConductorScripts,
    git: ConductorGit,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ConductorScripts {
    setup: String,
    archive: String,
}

#[derive(Deserialize, Default)]
#[serde(default)]
struct ConductorGit {
    branch_prefix: String,
}

/// What a compose file says, as far as sessions are concerned.
#[derive(Default, Debug, PartialEq)]
pub struct Guess {
    pub ports: Vec<String>,
    pub images: Vec<String>,
    pub db: Option<(String, String)>,
    /// What sessions can't isolate: fixed host ports, container names,
    /// volumes with a fixed name.
    pub warnings: Vec<String>,
}

/// Read a compose file closely enough: services, their `build`, `image`,
/// `container_name` and published ports, and top-level volume names. It is
/// a line scan, not a YAML parser; it expects the usual layout.
pub fn guess(compose: &str) -> Guess {
    let mut guess = Guess::default();
    let indent = |line: &str| line.len() - line.trim_start().len();
    let (mut section, mut service_indent, mut service) = (String::new(), None, String::new());
    let (mut has_build, mut has_image) = (false, false);
    let mut ports_indent: Option<usize> = None;
    let finish = |service: &str, has_build: bool, has_image: bool, guess: &mut Guess| {
        if has_build && !has_image && !service.is_empty() {
            guess.images.push(service.to_string());
        }
    };
    for line in compose.lines() {
        let text = line.trim();
        if text.is_empty() || text.starts_with('#') {
            continue;
        }
        let at = indent(line);
        if at == 0 {
            finish(&service, has_build, has_image, &mut guess);
            (service, has_build, has_image, ports_indent, service_indent) = (String::new(), false, false, None, None);
            section = text.trim_end_matches(':').to_string();
            continue;
        }
        if section == "volumes" && text.starts_with("name:") {
            guess.warnings.push(format!("a volume has a fixed name ({}): sessions would share it", value(text)));
        }
        if section != "services" {
            continue;
        }
        if service_indent.is_none_or(|s| at <= s) && text.ends_with(':') && !text.starts_with('-') {
            finish(&service, has_build, has_image, &mut guess);
            service_indent = Some(at);
            service = text.trim_end_matches(':').to_string();
            (has_build, has_image, ports_indent) = (false, false, None);
            continue;
        }
        if let Some(p) = ports_indent {
            if at > p && text.starts_with('-') {
                port(&service, text.trim_start_matches('-').trim(), &mut guess);
                continue;
            }
            ports_indent = None;
        }
        let key = text.split(':').next().unwrap_or("");
        match key {
            "build" => has_build = true,
            "image" => {
                has_image = true;
                let image = value(text);
                let engine = ["mysql", "mariadb", "postgres", "postgis"].into_iter().find(|e| image.contains(e));
                if let Some(engine) = engine
                    && guess.db.is_none()
                {
                    let engine = if engine == "mariadb" { "mysql" } else if engine == "postgis" { "postgres" } else { engine };
                    guess.db = Some((service.clone(), engine.to_string()));
                }
            }
            "container_name" => guess.warnings.push(format!("{service} has a fixed container name ({}): two sessions can't both run it", value(text))),
            "ports" => ports_indent = Some(at),
            _ => {}
        }
    }
    finish(&service, has_build, has_image, &mut guess);
    guess.ports.dedup();
    guess
}

/// One published port, like `${DB_PORT:-3306}:3306` or `'3000:3000'`.
fn port(service: &str, entry: &str, guess: &mut Guess) {
    let entry = entry.trim_matches(|c| c == '\'' || c == '"');
    // The host side is all but the last part (the container's port).
    let host = entry.rsplit_once(':').map_or("", |(host, _)| host);
    if let Some(start) = host.find("${") {
        let var: String = host[start + 2..].chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '_').collect();
        if !var.is_empty() && !guess.ports.contains(&var) {
            guess.ports.push(var);
        }
    } else if !host.is_empty() {
        guess.warnings.push(format!("{service} publishes fixed port {host}: two sessions can't both run it"));
    }
}

fn value(text: &str) -> String {
    text.split_once(':').map_or("", |(_, v)| v).trim().trim_matches(|c| c == '\'' || c == '"').to_string()
}

pub fn compose_file(dir: &Path) -> Option<PathBuf> {
    COMPOSE_FILES.iter().map(|f| dir.join(f)).find(|p| p.is_file())
}

/// The Compose project a checkout runs as: `COMPOSE_PROJECT_NAME` from its
/// `.env`, or its folder's name the way Compose writes it.
pub fn compose_project(dir: &Path) -> String {
    dotenv(dir, "COMPOSE_PROJECT_NAME").unwrap_or_else(|| project_name(&dir.file_name().unwrap_or_default().to_string_lossy()))
}

/// A variable of the `.env` in `dir`, as Compose reads it.
fn dotenv(dir: &Path, name: &str) -> Option<String> {
    let env = fs::read_to_string(dir.join(".env")).ok()?;
    env.lines()
        .find_map(|l| l.trim().strip_prefix(name)?.strip_prefix('=').map(|v| v.trim().trim_matches(|c| c == '"' || c == '\'').to_string()))
        .filter(|v| !v.is_empty())
}

/// The files Compose reads in `dir`: `COMPOSE_FILE` from its `.env`, else
/// the compose file and its override.
fn compose_files(dir: &Path) -> Vec<PathBuf> {
    if let Some(list) = dotenv(dir, "COMPOSE_FILE") {
        let separator = dotenv(dir, "COMPOSE_PATH_SEPARATOR").unwrap_or_else(|| ":".into());
        return list.split(separator.as_str()).filter(|f| !f.is_empty()).map(|f| dir.join(f)).collect();
    }
    let overrides = COMPOSE_OVERRIDES.iter().map(|f| dir.join(f)).filter(|p| p.is_file());
    compose_file(dir).into_iter().chain(overrides).collect()
}

fn project_name(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

#[cfg(test)]
thread_local! {
    /// A test's own settings folder, like `agents::TEST_STATE`.
    static TEST_CONFIG: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

fn config_dir() -> PathBuf {
    #[cfg(test)]
    if let Some(dir) = TEST_CONFIG.with_borrow(Clone::clone) {
        return dir.join("pulpo/repos");
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("pulpo/repos")
}

pub fn config_path(repo: &Path) -> PathBuf {
    config_dir().join(format!("{}.toml", repo.file_name().unwrap_or_default().to_string_lossy()))
}

/// The repository's session settings, in layers, a later one winning where
/// it says something: guessed from its compose file and lockfiles,
/// Conductor's settings (`.conductor/settings.toml` as `checkout` has it,
/// then `settings.local.toml` in main; an empty value says nothing there),
/// then Pulpo's own file, written the first time all commented out.
/// Includes add up instead (a `!pattern` takes one away). Also what can't
/// be isolated.
pub fn config(repo: &Path, checkout: &Path) -> Result<(Config, Vec<String>)> {
    let compose = compose_file(repo).map(|f| fs::read_to_string(f).unwrap_or_default());
    let guessed = compose.as_deref().map(guess).unwrap_or_default();
    let (db_service, db) = guessed.db.clone().unwrap_or_default();
    let rails = checkout.join("bin/rails").is_file();
    // After the copy too: main's database is at main's migrations, the
    // session's code at the default branch's (`db:prepare` does what's due).
    // Without Docker, the install the lockfiles say; with it, that's the
    // containers' business.
    let setup = if rails && guessed.images.iter().any(|s| s == "app") {
        "docker compose exec -T app bin/rails db:prepare".to_string()
    } else if compose.is_none() {
        install(checkout)
    } else {
        String::new()
    };
    let include = if rails { vec!["config/master.key".to_string(), "config/credentials/*.key".to_string()] } else { Vec::new() };
    let mut config = Config { ports: guessed.ports.clone(), images: guessed.images.clone(), db, db_service, setup, include, ..Config::default() };
    let mut warnings = guessed.warnings;

    for (dir, name) in [(checkout, "settings.toml"), (repo, "settings.local.toml")] {
        let file = dir.join(".conductor").join(name);
        let Ok(text) = fs::read_to_string(&file) else { continue };
        let conductor: Conductor = match toml::from_str(&text) {
            Ok(c) => c,
            Err(e) => {
                warnings.push(format!("{} doesn't read: {e}", file.display()));
                continue;
            }
        };
        let said = |text: String| Some(text).filter(|t| !t.trim().is_empty());
        let globs: Vec<String> = conductor.file_include_globs.split([',', '\n']).map(str::trim).filter(|g| !g.is_empty()).map(String::from).collect();
        let layer = Layer {
            setup: said(conductor.scripts.setup),
            teardown: said(conductor.scripts.archive),
            include: Some(globs).filter(|g| !g.is_empty()),
            branch_prefix: said(conductor.git.branch_prefix.trim_end_matches('/').to_string()),
            ..Layer::default()
        };
        config.overlay(layer, !yours(dir, &file));
    }

    let path = config_path(repo);
    match fs::read_to_string(&path) {
        Ok(text) => {
            let mut mine: Layer = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
            // Written by the first worktree release, every line set: its
            // empty values meant "nothing" then, so they still do.
            if text.contains("compose file; edit freely.") {
                mine.forget_empty();
            }
            config.overlay(mine, false);
        }
        Err(_) => {
            fs::create_dir_all(config_dir())?;
            fs::write(&path, starter(&config))?;
        }
    }
    Ok((config, warnings))
}

/// Whether a settings file is yours, not the repository's: it's there by
/// exactly this name, a real folder and file (on a case-insensitive disk
/// `.Conductor/Settings.toml` opens as `.conductor/settings.toml`, and a
/// link leads elsewhere, while git, asked about this name, sees nothing),
/// and git says plainly it's untracked or ignored (it says nothing inside a
/// submodule, say). Anything unsure is the repository's, so its scripts are
/// asked about.
fn yours(dir: &Path, file: &Path) -> bool {
    let Ok(rel) = file.strip_prefix(dir) else { return false };
    let parts: Vec<_> = rel.components().collect();
    let mut at = dir.to_path_buf();
    for (i, part) in parts.iter().enumerate() {
        let last = i + 1 == parts.len();
        let exact = fs::read_dir(&at).into_iter().flatten().flatten().any(|e| {
            e.file_name() == part.as_os_str() && e.file_type().is_ok_and(|t| if last { t.is_file() } else { t.is_dir() })
        });
        if !exact {
            return false;
        }
        at.push(part);
    }
    let rel = rel.to_string_lossy();
    // Case-insensitive as the disk is: else a committed `.Conductor/...`
    // pulled over your ignored file shows that file as ignored.
    git(dir, &["-c", "core.ignorecase=true", "status", "--porcelain", "--ignored", "--untracked-files=all", "--", &rel], None)
        .is_ok_and(|out| out == format!("?? {rel}") || out == format!("!! {rel}"))
}

impl Layer {
    /// Its empty values unsaid.
    fn forget_empty(&mut self) {
        for text in [&mut self.db, &mut self.db_service, &mut self.fresh, &mut self.setup, &mut self.teardown, &mut self.branch_prefix] {
            if text.as_deref() == Some("") {
                *text = None;
            }
        }
        for list in [&mut self.ports, &mut self.images, &mut self.include] {
            if list.as_ref().is_some_and(Vec::is_empty) {
                *list = None;
            }
        }
    }
}

impl Config {
    /// What `layer` says, over this; `committed` when it's a file of the
    /// repository's.
    fn overlay(&mut self, layer: Layer, committed: bool) {
        let set = |mine: &mut String, theirs: Option<String>| {
            if let Some(theirs) = theirs {
                *mine = theirs;
            }
        };
        if let Some(ports) = layer.ports {
            self.ports = ports;
        }
        if let Some(images) = layer.images {
            self.images = images;
        }
        self.include.extend(layer.include.unwrap_or_default());
        if layer.setup.is_some() {
            self.committed.0 = committed;
        }
        if layer.teardown.is_some() {
            self.committed.1 = committed;
        }
        set(&mut self.db, layer.db);
        set(&mut self.db_service, layer.db_service);
        set(&mut self.fresh, layer.fresh);
        set(&mut self.setup, layer.setup);
        set(&mut self.teardown, layer.teardown);
        set(&mut self.branch_prefix, layer.branch_prefix);
    }

    /// The committed scripts, by what they are: `("setup", script)`.
    fn committed_scripts(&self) -> Vec<(&'static str, String)> {
        let scripts = [(self.committed.0, "setup", &self.setup), (self.committed.1, "teardown", &self.teardown)];
        scripts.into_iter().filter(|(c, _, s)| *c && !s.is_empty()).map(|(_, what, s)| (what, s.clone())).collect()
    }

    /// Without these scripts (`untrusted`'s): what's left out, for the log.
    pub fn without(&mut self, scripts: &[(&'static str, String)]) -> Vec<String> {
        let mut notes = Vec::new();
        for (what, _) in scripts {
            let script = if *what == "setup" { &mut self.setup } else { &mut self.teardown };
            script.clear();
            notes.push(format!("! the committed {what} script isn't trusted: skipped"));
        }
        notes
    }
}

/// The install a checkout's lockfiles call for, empty without any.
fn install(dir: &Path) -> String {
    let has = |f: &str| dir.join(f).is_file();
    let js = [
        ("pnpm-lock.yaml", "pnpm install --frozen-lockfile"),
        ("yarn.lock", "yarn install --frozen-lockfile"),
        ("bun.lock", "bun install --frozen-lockfile"),
        ("bun.lockb", "bun install --frozen-lockfile"),
        ("package-lock.json", "npm ci"),
    ]
    .into_iter()
    .find(|(lock, _)| has(lock))
    .map(|(_, command)| command);
    let steps = [has("Gemfile.lock").then_some("bundle install"), js, has("uv.lock").then_some("uv sync")];
    steps.into_iter().flatten().collect::<Vec<_>>().join(" && ")
}

/// The settings file: every setting as it is now, commented out, so it
/// says nothing until you change one (and later guesses still count).
fn starter(c: &Config) -> String {
    let list = |items: &[String]| items.iter().map(|i| format!("{i:?}")).collect::<Vec<_>>().join(", ");
    format!(
        "# How Pulpo sets up a session of this repository: a git worktree on its
# own branch, with its own ports and Docker project. Below, each setting as
# Pulpo found it when writing this file (guessed from the compose file and
# lockfiles, or read from .conductor/settings.toml). Uncomment one to set
# it; an empty value (\"\" or []) turns it off, but for include.

# Main's ignored files each session gets (gitignore patterns), added to its
# .env files, those .worktreeinclude names and these found ones: [{}].
# \"!pattern\" leaves one out; [] adds nothing.
# include = []

# Port variables of the compose file; each session gets its own ports.
# ports = [{}]
# Services main builds: their images are reused instead of built again.
# images = [{}]
# How main's database is copied into the session: mysql, postgres or \"\" for no copy.
# db = {:?}
# db_service = {:?}
# Run instead when main's database isn't running.
# fresh = {:?}
# Run at the end of the setup, and before a session is removed.
# setup = {:?}
# teardown = {:?}
# A session's branch is <branch_prefix>/<name>; \"\" for git's name.
# branch_prefix = {:?}
",
        list(&c.include),
        list(&c.ports),
        list(&c.images),
        c.db,
        c.db_service,
        c.fresh,
        c.setup,
        c.teardown,
        c.branch_prefix
    )
}

/// The committed scripts of `config` you haven't trusted for this
/// repository (`trust`). A session starts from the remote, so whoever can
/// push could otherwise run code here.
pub fn untrusted(repo: &Path, config: &Config) -> Vec<(&'static str, String)> {
    let trusted = trusted();
    config.committed_scripts().into_iter().filter(|(_, script)| !trusted.iter().any(|(r, s)| r == repo && s == script)).collect()
}

/// Run these scripts of this repository from now on, unasked.
pub fn trust(repo: &Path, scripts: &[(&'static str, String)]) -> Result<()> {
    let mut trusted = trusted();
    trusted.extend(scripts.iter().map(|(_, script)| (repo.to_path_buf(), script.clone())));
    fs::create_dir_all(trust_path().parent().expect("has a parent"))?;
    // The hashes of before: replaced by exact text.
    let _ = fs::remove_file(crate::agents::state_dir().join("trusted-scripts"));
    Ok(fs::write(trust_path(), serde_json::to_vec_pretty(&trusted)?)?)
}

/// What you trusted: each repository with a script's exact text (no hash
/// to forge).
fn trusted() -> Vec<(PathBuf, String)> {
    fs::read(trust_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

fn trust_path() -> PathBuf {
    crate::agents::state_dir().join("trusted-scripts.json")
}

/// FNV-1a: the same in every build, unlike std's hasher.
fn fnv(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(*b)).wrapping_mul(0x100000001b3))
}

// The registry of sessions, in the state directory.

fn registry_path() -> PathBuf {
    crate::agents::state_dir().join("sessions.json")
}

/// Every session, as recorded.
pub fn all() -> Vec<Session> {
    fs::read(registry_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

/// Change the registry with no other Pulpo changing it at the same time.
fn update<T>(change: impl FnOnce(&mut Vec<Session>) -> Result<T>) -> Result<T> {
    let dir = crate::agents::state_dir();
    fs::create_dir_all(&dir)?;
    let lock = File::create(dir.join("sessions.lock"))?;
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(io::Error::last_os_error().into());
    }
    let mut sessions = all();
    let result = change(&mut sessions)?;
    let tmp = registry_path().with_extension("json.tmp");
    fs::write(&tmp, serde_json::to_vec_pretty(&sessions)?)?;
    fs::rename(&tmp, registry_path())?;
    Ok(result)
}

/// The first block of `count` ports that no session holds and that are
/// free right now.
fn allocate(taken: &BTreeSet<u16>, count: usize, free: impl Fn(u16) -> bool) -> Option<Vec<u16>> {
    let blocks = (count as u16).div_ceil(BLOCK).max(1);
    (0..1000u16).map(|k| FIRST_PORT + k * BLOCK).find_map(|base| {
        let ports: Vec<u16> = (base..base + blocks * BLOCK).take(count).collect();
        (ports.iter().all(|p| !taken.contains(p) && free(*p))).then_some(ports)
    })
}

fn port_free(port: u16) -> bool {
    std::net::TcpListener::bind(("127.0.0.1", port)).is_ok()
}

/// Make a session and record it: a git worktree of `repo` in
/// `<repo>.sessions/<name>`, on its own branch from the default branch as on
/// the remote, locked so nothing prunes it behind Pulpo's back, with main's
/// ignored files it needs. Its setup runs next, in the background
/// (`setup_script`). Also what it noted on the way, for the setup's log.
/// Whatever it made is undone if a step fails.
pub fn create(repo: &Path, name: &str, config: &Config) -> Result<(Session, Vec<String>)> {
    // Lowercase: Compose project names are, and two sessions mustn't share one.
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_') {
        bail!("a session name is lowercase letters, digits, - and _");
    }
    if git(repo, &["rev-parse", "-q", "--verify", "HEAD"], None).is_err() {
        bail!("the repository has no commit yet: make one first");
    }
    let repo_name = repo.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let parent = repo.parent().context("the repository has no parent folder")?;
    let root = parent.join(format!("{repo_name}.sessions")).join(name);
    if root.exists() {
        bail!("{} already exists", root.display());
    }
    let (from, mut notes) = base(repo);
    let start = git(repo, &["rev-parse", "--verify", &format!("{from}^{{commit}}")], None)?;
    let branch = branch_name(repo, name, &config.branch_prefix);
    // A branch by that name is taken only as it would start: nothing of its own.
    let made_branch = match git(repo, &["rev-parse", "-q", "--verify", &format!("refs/heads/{branch}")], None) {
        Ok(tip) if tip == start => false,
        Ok(_) => bail!("branch {branch} exists already, with commits of its own: pick another name"),
        Err(_) => true,
    };
    fs::create_dir_all(root.parent().expect("has a parent"))?;
    let path = root.to_string_lossy().into_owned();
    let mut add = vec!["worktree", "add", "--lock", "--reason", "a Pulpo session"];
    if made_branch {
        add.extend(["-b", branch.as_str(), path.as_str(), start.as_str()]);
    } else {
        add.extend([path.as_str(), branch.as_str()]);
    }
    // Hooks run (post-checkout, LFS) and can fail on their own: the
    // worktree is there or it isn't.
    let added = git(repo, &add, None);
    if !root.join(".git").is_file() {
        // Only ours: git won't delete a branch checked out elsewhere.
        if made_branch {
            let _ = git(repo, &["branch", "-D", &branch], None);
        }
        bail!("git worktree add failed: {:#}", added.err().unwrap_or_else(|| anyhow::anyhow!("no worktree")));
    }
    if let Err(e) = added {
        notes.push(format!("! git worktree add: {e:#}"));
    }
    let undo = |project: &str| {
        let _ = fs::remove_file(override_path(repo, project));
        let _ = git(repo, &["worktree", "remove", "--force", "--force", &path], None);
        if made_branch {
            let _ = git(repo, &["branch", "-D", &branch], None);
        }
    };

    notes.extend(include(repo, &root, &config.include));
    let main_project = compose_project(repo);
    let project = project_name(&format!("{main_project}-{name}"));
    let compose_file = match compose_override(repo, &root, &project) {
        Ok(files) => files,
        Err(e) => {
            notes.push(format!("! git won't work in its containers: {e:#}"));
            None
        }
    };
    if compose_file.is_some() && fs::read_to_string(root.join("Makefile")).is_ok_and(|m| m.contains("compose -f")) {
        notes.push("! the Makefile names compose files with -f: those commands don't see main's .git".into());
    }
    let result = update(|sessions| {
        // Its own block of ten, then the compose file's port variables.
        let taken: BTreeSet<u16> =
            sessions.iter().flat_map(|s| s.ports.iter().map(|(_, p)| *p).chain(s.port.into_iter().flat_map(|p| p..p + BLOCK))).collect();
        let ports = allocate(&taken, BLOCK as usize + config.ports.len(), port_free).context("no free ports from 20000 up")?;
        let session = Session {
            name: name.to_string(),
            repo: repo.to_path_buf(),
            root: root.clone(),
            project: project.clone(),
            main_project: main_project.clone(),
            ports: config.ports.iter().cloned().zip(ports[BLOCK as usize..].iter().copied()).collect(),
            images: config.images.clone(),
            base: None,
            kind: Kind::Worktree,
            port: Some(ports[0]),
            branch: Some(branch.clone()),
            made_branch,
            start: Some(start.clone()),
            compose_file: compose_file.clone(),
        };
        sessions.push(session.clone());
        Ok(session)
    });
    match result {
        Ok(session) => Ok((session, notes)),
        Err(e) => {
            undo(&project);
            Err(e)
        }
    }
}

/// git in `dir`, for making and removing sessions: with no terminal to
/// ask on (Pulpo's screen is raw; ssh opens the terminal itself to ask,
/// so it runs in a session of its own). Its output, trimmed, or what it
/// said as the error. Stopped after `limit`, if given.
fn git(dir: &Path, args: &[&str], limit: Option<Duration>) -> Result<String> {
    use std::os::unix::process::CommandExt;
    let mut command = Command::new("git");
    command.arg("-C").arg(dir).args(args).env("GIT_TERMINAL_PROMPT", "0");
    command.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped());
    unsafe {
        command.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = command.spawn().context("running git")?;
    if let Some(limit) = limit {
        let end = Instant::now() + limit;
        while child.try_wait()?.is_none() {
            if Instant::now() > end {
                // Its whole group (ssh, index-pack), and TERM first: git
                // then removes its ref locks, in the repository every
                // session shares.
                let group = -(child.id() as i32);
                unsafe { libc::kill(group, libc::SIGTERM) };
                let grace = Instant::now() + Duration::from_secs(2);
                while child.try_wait()?.is_none() && Instant::now() < grace {
                    std::thread::sleep(Duration::from_millis(50));
                }
                unsafe { libc::kill(group, libc::SIGKILL) };
                let _ = child.wait();
                bail!("git {} took over {} s", args[0], limit.as_secs());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!("{}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Where a new session starts: the remote's default branch, fetched now
/// (five seconds at most, else as last fetched), or main's HEAD when there's
/// no remote. Also why, when it isn't the remote's.
fn base(repo: &Path) -> (String, Vec<String>) {
    let head = |why: &str| ("HEAD".to_string(), vec![format!("! {why}: starting from main's last commit")]);
    if git(repo, &["remote", "get-url", "origin"], None).is_err() {
        return ("HEAD".into(), Vec::new());
    }
    let exists = |branch: &str| git(repo, &["rev-parse", "-q", "--verify", &format!("refs/remotes/origin/{branch}")], None).is_ok();
    let default = git(repo, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], None)
        .ok()
        .and_then(|r| r.strip_prefix("origin/").map(str::to_string))
        .or_else(|| ["main", "master"].into_iter().find(|b| exists(b)).map(str::to_string));
    let Some(default) = default else { return head("origin's default branch isn't known (git remote set-head origin --auto)") };
    let mut notes = Vec::new();
    if let Err(e) = git(repo, &["fetch", "-q", "origin", &default], Some(Duration::from_secs(5))) {
        notes.push(format!("! couldn't fetch origin ({e:#}): starting from origin/{default} as last fetched"));
    }
    if !exists(&default) {
        return head(&format!("origin/{default} was never fetched"));
    }
    (format!("origin/{default}"), notes)
}

/// A new session's branch: `<user>/<name>`, as Conductor and Orca name
/// them. The user is the settings' `branch_prefix` if set (Conductor's own
/// setting), else the name you gave git, the repository's own before your
/// global one (git's own order); without one, your account where the
/// remote is: gh's login for that host (from gh's settings, no network),
/// else git's `github.user`. As a branch can spell it.
fn branch_name(repo: &Path, name: &str, prefix: &str) -> String {
    let prefix: String = prefix.chars().filter(|c| c.is_ascii_alphanumeric() || "-_./".contains(*c)).collect();
    let prefix = prefix.trim_matches('/');
    if !prefix.is_empty() {
        return format!("{prefix}/{name}");
    }
    let login = || {
        let url = git(repo, &["remote", "get-url", "origin"], None).ok()?;
        let out = Command::new("gh").args(["config", "get", "-h", &remote_host(&url)?, "user"]).stdin(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|l| !l.is_empty())
    };
    let user = git(repo, &["config", "user.name"], None)
        .ok()
        .filter(|n| !n.is_empty())
        .or_else(login)
        .or_else(|| git(repo, &["config", "github.user"], None).ok())
        .unwrap_or_default()
        .to_lowercase();
    let user = user.split(|c: char| !c.is_ascii_alphanumeric()).filter(|w| !w.is_empty()).collect::<Vec<_>>().join("-");
    if user.is_empty() { name.to_string() } else { format!("{user}/{name}") }
}

/// The host of a git remote: `git@github.com:o/r.git`,
/// `https://github.com/o/r`, `ssh://git@host:22/o/r`.
fn remote_host(url: &str) -> Option<String> {
    let rest = url.split_once("://").map_or(url, |(_, rest)| rest);
    let rest = rest.split_once('@').map_or(rest, |(_, host)| host);
    let host = rest.split([':', '/']).next()?;
    (!host.is_empty() && url != host).then(|| host.to_string())
}

/// Clone main's ignored files the session needs into it: those
/// `.worktreeinclude` (Claude Code's file) and `include` name, in gitignore
/// syntax, and its `.env` files. Only ignored ones (tracked files come
/// with the branch), never out of the repository; a folder whole. A
/// folder that is ignored whole is looked into only when a pattern names
/// it. What it did, for the log.
fn include(repo: &Path, root: &Path, extra: &[String]) -> Vec<String> {
    let file = [root, repo].map(|d| d.join(".worktreeinclude")).into_iter().find(|f| f.is_file());
    let mut patterns: Vec<String> = file.map(|f| fs::read_to_string(f).unwrap_or_default().lines().map(str::to_string).collect()).unwrap_or_default();
    patterns.extend(extra.iter().cloned());
    patterns.retain(|p| !p.trim().is_empty() && !p.trim_start().starts_with('#'));
    // The `.env` files always; what's named adds to them (or, with `!`, takes away).
    patterns.splice(0..0, DEFAULT_INCLUDE.map(String::from));
    let mut builder = GitignoreBuilder::new(repo);
    for p in &patterns {
        let _ = builder.add_line(None, p);
    }
    let Ok(wanted) = builder.build() else { return vec!["! the include patterns don't parse".into()] };
    let listed = git(repo, &["ls-files", "-z", "--others", "--ignored", "--exclude-standard", "--directory"], None).unwrap_or_default();
    let (mut copied, mut notes) = (Vec::new(), Vec::new());
    let mut take = |rel: &Path| {
        let (from, to) = (repo.join(rel), root.join(rel));
        if to.symlink_metadata().is_ok() {
            return;
        }
        let made = to.parent().map_or(Ok(()), fs::create_dir_all).map_err(anyhow::Error::from).and_then(|()| clone(&from, &to));
        match made {
            Ok(()) => copied.push(rel.display().to_string()),
            Err(e) => notes.push(format!("! couldn't copy {}: {e:#}", rel.display())),
        }
    };
    let mut pending: Vec<(PathBuf, bool)> =
        listed.split('\0').filter(|e| !e.is_empty()).map(|e| (PathBuf::from(e.trim_end_matches('/')), e.ends_with('/'))).collect();
    while let Some((rel, is_dir)) = pending.pop() {
        if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
            continue;
        }
        if wanted.matched_path_or_any_parents(&rel, is_dir).is_ignore() {
            take(&rel);
        } else if is_dir && patterns.iter().any(|p| p.trim_start_matches(['!', '/']).starts_with(&format!("{}/", rel.display()))) {
            // Its entries, not following links out.
            for entry in fs::read_dir(repo.join(&rel)).into_iter().flatten().flatten() {
                let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                pending.push((rel.join(entry.file_name()), is_dir));
            }
        }
    }
    if !copied.is_empty() {
        copied.sort();
        notes.insert(0, format!("from main: {}", copied.join(", ")));
    }
    notes
}

/// Where the session's `.git` file points (main's `.git`) doesn't exist in
/// a container that mounts the session's folder, so git fails there. When
/// a service does (as Compose resolves them: every profile, `extends`),
/// an override mounts main's `.git` at the same path, read-only (a
/// container can't prune or rewrite the shared repository). What the
/// session's `COMPOSE_FILE` is then: its compose files, the override last.
fn compose_override(repo: &Path, root: &Path, project: &str) -> Result<Option<String>> {
    let files = compose_files(root);
    if files.is_empty() {
        return Ok(None);
    }
    let mut command = Command::new("docker");
    command.arg("compose");
    for f in &files {
        command.arg("-f").arg(f);
    }
    let out = command
        .args(["--profile", "*", "config", "--format", "json"])
        .current_dir(root)
        .env("COMPOSE_PROJECT_NAME", project)
        .stdin(Stdio::null())
        .output()
        .context("running docker compose config")?;
    if !out.status.success() {
        bail!("docker compose config: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    let config: serde_json::Value = serde_json::from_slice(&out.stdout).context("reading docker compose config")?;
    let (root, git_dir) = (root.canonicalize()?, repo.join(".git").canonicalize()?);
    let mount = serde_json::json!({ "volumes": [{ "type": "bind", "source": git_dir, "target": git_dir, "read_only": true }] });
    let services: serde_json::Map<String, serde_json::Value> = config["services"]
        .as_object()
        .into_iter()
        .flatten()
        .filter(|(_, service)| {
            service["volumes"].as_array().into_iter().flatten().any(|v| {
                v["type"] == "bind" && v["source"].as_str().and_then(|s| Path::new(s).canonicalize().ok()).is_some_and(|s| root.starts_with(s))
            })
        })
        .map(|(name, _)| (name.clone(), mount.clone()))
        .collect();
    if services.is_empty() {
        return Ok(None);
    }
    let path = override_path(repo, project);
    fs::create_dir_all(path.parent().expect("has a parent"))?;
    // JSON is YAML.
    fs::write(&path, serde_json::to_vec_pretty(&serde_json::json!({ "services": services }))?)?;
    Ok(Some(files.iter().chain([&path]).map(|f| f.to_string_lossy()).collect::<Vec<_>>().join(":")))
}

/// The session's compose override: in Pulpo's state, never in its folder,
/// where it would show in its changes. Named for the repository too: two
/// repositories of the same name can have sessions of the same name.
fn override_path(repo: &Path, project: &str) -> PathBuf {
    let hash = fnv(repo.as_os_str().as_bytes());
    crate::agents::state_dir().join("compose").join(format!("{project}-{:08x}.yaml", hash as u32))
}

/// A clone of a file or folder (APFS: instant, sharing disk blocks with the
/// original until either side changes), a link as a link; a plain copy
/// where cloning can't be done (another disk).
fn clone(from: &Path, to: &Path) -> Result<()> {
    let (src, dst) = (CString::new(from.as_os_str().as_bytes())?, CString::new(to.as_os_str().as_bytes())?);
    if unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), CLONE_NOFOLLOW) } == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    let copied = Command::new("cp").arg("-RP").arg(from).arg(to).stdin(Stdio::null()).output()?;
    if !copied.status.success() {
        bail!("{error}; cp: {}", String::from_utf8_lossy(&copied.stderr).trim());
    }
    Ok(())
}

/// Take a session off the registry (its removal script calls this last).
pub fn forget(root: &Path) -> Result<()> {
    update(|sessions| {
        sessions.retain(|s| s.root != root);
        Ok(())
    })
}

/// What its terminals get: its own Compose project and ports, and where
/// it and main are.
pub fn env(session: &Session) -> Vec<(String, String)> {
    let mut env = vec![("COMPOSE_PROJECT_NAME".to_string(), session.project.clone()), ("PULPO_SESSION".to_string(), session.name.clone())];
    env.extend(session.ports.iter().map(|(var, port)| (var.clone(), port.to_string())));
    let path = |p: &Path| p.to_string_lossy().into_owned();
    if let Some(port) = session.port {
        env.push(("PULPO_PORT".into(), port.to_string()));
        // Without Docker, the variable web servers read.
        if compose_file(&session.root).is_none() {
            env.push(("PORT".into(), port.to_string()));
        }
    }
    if session.kind == Kind::Worktree {
        env.push(("PULPO_ROOT_PATH".into(), path(&session.repo)));
        env.push(("PULPO_WORKSPACE_PATH".into(), path(&session.root)));
    }
    if session.compose_file.is_some() {
        // Read now: a compose file added since counts, and the override
        // only while it's there (state cleared, Compose would fail).
        let over = override_path(&session.repo, &session.project);
        let files: Vec<PathBuf> = compose_files(&session.root).into_iter().chain(over.is_file().then_some(over)).collect();
        if !files.is_empty() {
            env.push(("COMPOSE_FILE".into(), files.iter().map(|f| path(f)).collect::<Vec<_>>().join(":")));
            env.push(("COMPOSE_PATH_SEPARATOR".into(), ":".into()));
        }
    }
    env
}

/// What deleting the session would lose. A worktree's: uncommitted
/// changes, and commits on a detached HEAD (its branches stay, and the
/// repository with them). A copy's: uncommitted changes, and commits on no
/// remote and not in main either (main's own unpushed commits were copied
/// with it, and stay in main), and its stash. Read-only git.
pub fn unsaved(session: &Session) -> Vec<String> {
    // Its output if it worked; `input` on its stdin.
    let run = |dir: &Path, args: &[&str], input: String| -> Option<String> {
        let mut child = Command::new("git").arg("-C").arg(dir).args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let mut stdin = child.stdin.take()?;
        std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let out = child.wait_with_output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let mut lost = Vec::new();
    if !session.root.is_dir() {
        return lost;
    }
    match run(&session.root, &["status", "--porcelain"], String::new()).map(|out| out.lines().count()) {
        Some(0) => {}
        Some(n) => lost.push(format!("{n} uncommitted change(s)")),
        None => lost.push("changes git couldn't check".into()),
    }
    if session.kind == Kind::Worktree {
        if run(&session.root, &["symbolic-ref", "-q", "HEAD"], String::new()).is_none() {
            match run(&session.root, &["rev-list", "HEAD", "--not", "--branches", "--remotes"], String::new()).map(|out| out.lines().count()) {
                Some(0) => {}
                Some(n) => lost.push(format!("{n} commit(s) on no branch")),
                None => lost.push("commits git couldn't check".into()),
            }
        }
        return lost;
    }
    // HEAD too, when there is one: commits on a detached HEAD are on no branch.
    let head = run(&session.root, &["rev-parse", "-q", "--verify", "HEAD"], String::new()).is_some();
    let tips: &[&str] = if head { &["HEAD", "--branches"] } else { &["--branches"] };
    let commits = run(&session.root, &[&["rev-list"], tips, &["--not", "--remotes"]].concat(), String::new());
    let in_main = commits.and_then(|commits| run(&session.repo, &["cat-file", "--batch-check"], commits));
    match in_main.map(|out| out.lines().filter(|line| line.ends_with(" missing")).count()) {
        Some(0) => {}
        Some(n) => lost.push(format!("{n} commit(s) not pushed")),
        None => lost.push("commits git couldn't check".into()),
    }
    let stash = |dir: &Path| run(dir, &["rev-parse", "-q", "--verify", "refs/stash"], String::new());
    if stash(&session.root).is_some_and(|s| Some(&s) != stash(&session.repo).as_ref()) {
        lost.push("stashed changes".into());
    }
    lost
}

/// The branch a worktree session leaves behind when deleted: the one it's
/// on, unless that's the branch Pulpo made and nothing was added to it.
pub fn kept_branch(session: &Session) -> Option<String> {
    if session.kind != Kind::Worktree {
        return None;
    }
    let current = git(&session.root, &["branch", "--show-current"], None).ok().filter(|b| !b.is_empty())?;
    let tip = git(&session.repo, &["rev-parse", "-q", "--verify", &format!("refs/heads/{current}")], None).ok();
    let untouched = session.made_branch && session.branch.as_ref() == Some(&current) && tip.is_some() && tip == session.start;
    (!untouched).then_some(current)
}

/// A folder Pulpo made for a session: `<repo>.sessions/<name>`.
fn in_sessions_folder(root: &Path) -> bool {
    root.parent().and_then(Path::file_name).is_some_and(|dir| dir.to_string_lossy().ends_with(".sessions"))
        && root.file_name().is_some_and(|name| !name.is_empty() && name != "..")
}

/// Whether `a` and `b` are the same folder, however spelled.
pub fn same_path(a: &Path, b: &Path) -> bool {
    a == b || matches!((a.canonicalize(), b.canonicalize()), (Ok(a), Ok(b)) if a == b)
}

/// A worktree of `repo` other than `repo` itself, in a `.sessions` folder:
/// all a removal will ever delete.
fn is_session_worktree(repo: &Path, root: &Path) -> bool {
    in_sessions_folder(root) && !same_path(repo, root) && crate::git::worktrees(repo).iter().any(|w| same_path(w, root))
}

/// `'text'` for a shell.
pub fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The setup, run in the background with the session's environment while
/// its first terminal is already yours: main's images, its database, the
/// containers. It exits with an error if a step fails.
pub fn setup_script(session: &Session, config: &Config, notes: &[String]) -> String {
    let root = quote(&session.root.to_string_lossy());
    let main = &session.main_project;
    let mut s = String::from("step() { printf '\\n\\033[1m%s\\033[0m\\n' \"$1\"; }\nsetup() (\nset -e\nset -o pipefail\n");
    s += &format!("cd {root}\n");
    s += &conductor_exports(session);
    let what = match &session.branch {
        Some(branch) => format!("Session {}: branch {branch}, a worktree of {}", session.name, session.repo.display()),
        None => format!("Session {}: a copy of {}", session.name, session.repo.display()),
    };
    s += &format!("step {}\n", quote(&what));
    if let Some(port) = session.port {
        s += &format!("echo {}\n", quote(&format!("  PULPO_PORT={port} (to {})", port + BLOCK - 1)));
    }
    for (var, port) in &session.ports {
        s += &format!("echo {}\n", quote(&format!("  {var}={port}")));
    }
    s += &format!("echo {}\n", quote(&format!("  settings: {}", config_path(&session.repo).display())));
    for note in notes {
        s += &format!("echo {}\n", quote(&format!("  {note}")));
    }
    if session.kind == Kind::Worktree {
        s += "if [ -f .gitmodules ]; then\nstep 'Submodules'\nGIT_TERMINAL_PROMPT=0 git submodule update --init --recursive || echo '  ! some submodules could not be fetched: the rest of the setup goes on'\nfi\n";
    }
    if compose_file(&session.root).is_some() {
        if !session.images.is_empty() {
            s += "step \"Reusing main's images\"\n";
            for image in &session.images {
                let (from, to) = (quote(&format!("{main}-{image}")), quote(&format!("{}-{image}", session.project)));
                s += &format!("if docker image inspect {from} >/dev/null 2>&1; then docker tag {from} {to}; echo \"  {image}\"; fi\n");
            }
        }
        let db = quote(&config.db_service);
        let copy = match config.db.as_str() {
            "mysql" => Some((
                "MYSQL_PWD=\"$MYSQL_ROOT_PASSWORD\" exec mysqldump -uroot --all-databases --single-transaction --routines --events --triggers --quick",
                "MYSQL_PWD=\"$MYSQL_ROOT_PASSWORD\" exec mysql -uroot",
            )),
            "postgres" => Some(("exec pg_dumpall -U \"${POSTGRES_USER:-postgres}\"", "exec psql -q -U \"${POSTGRES_USER:-postgres}\" -d postgres")),
            _ => None,
        };
        let mut fresh = false;
        if let Some((dump, restore)) = copy.filter(|_| !config.db_service.is_empty()) {
            s += "step 'Starting its database'\n";
            s += &format!("docker compose up -d --wait {db}\n");
            s += &format!("if [ -n \"$(docker compose -p {} ps -q --status running {db} 2>/dev/null)\" ]; then\n", quote(main));
            s += "step \"Copying main's database\"\n";
            s += &format!("docker compose -p {} exec -T {db} sh -c {} | docker compose exec -T {db} sh -c {}\n", quote(main), quote(dump), quote(restore));
            s += "else\nstep \"main's database isn't running: starting from a fresh one\"\nfresh=1\nfi\n";
            fresh = !config.fresh.is_empty();
        }
        s += "step 'Starting its containers'\ndocker compose up -d\n";
        if fresh {
            s += &format!("if [ -n \"$fresh\" ]; then step 'Preparing the fresh database'; {}; fi\n", config.fresh);
        }
    }
    if !config.setup.is_empty() {
        s += &format!("step 'Setup'\n{}\n", config.setup);
    }
    // Called on its own: in an `if`, the shell would ignore its `set -e`.
    s += ")\nsetup\nstatus=$?\nif [ $status = 0 ]; then\nstep 'Ready.'\n";
    for (var, port) in &session.ports {
        s += &format!("echo {}\n", quote(&format!("  {var}: localhost:{port}")));
    }
    s += "fi\nexit $status\n";
    s
}

/// Conductor's variables, so its setup and archive scripts run as they
/// would there; only in Pulpo's scripts, not its terminals (where tools
/// would think they run in Conductor).
fn conductor_exports(session: &Session) -> String {
    let Some(port) = session.port else { return String::new() };
    let vars = [
        ("CONDUCTOR_PORT", port.to_string()),
        ("CONDUCTOR_ROOT_PATH", session.repo.to_string_lossy().into_owned()),
        ("CONDUCTOR_WORKSPACE_NAME", session.name.clone()),
        ("CONDUCTOR_WORKSPACE_PATH", session.root.to_string_lossy().into_owned()),
    ];
    vars.iter().map(|(var, value)| format!("export {var}={}\n", quote(value))).collect()
}

/// The removal: its teardown, its containers and volumes (its database),
/// the image tags made for it, then its folder: a worktree's with `git
/// worktree remove`, and the branch Pulpo made for it if nothing was added
/// to it. Nothing is deleted if Docker can't stop it, and it exits with an
/// error. `exe` is Pulpo, to take it off the registry at the end.
pub fn teardown_script(session: &Session, config: &Config, exe: &Path) -> String {
    // Only ever what Pulpo made: a folder in `<repo>.sessions`, and for a
    // worktree, one of the repository's.
    let safe = match session.kind {
        Kind::Copy => in_sessions_folder(&session.root),
        // Its folder deleted by hand: nothing on disk left to delete but the
        // worktree's record in main's `.git`.
        Kind::Worktree => is_session_worktree(&session.repo, &session.root) || (in_sessions_folder(&session.root) && !session.root.exists()),
    };
    if !safe {
        return format!("echo {}\nexit 1\n", quote(&format!("Not removing {}: not a session's folder", session.root.display())));
    }
    let root = quote(&session.root.to_string_lossy());
    let mut s = String::from("remove() (\nset -e\n");
    // Its teardown, from its folder while it's there.
    if !config.teardown.is_empty() {
        s += &format!("if [ -d {root} ]; then\ncd {root}\n{}{}\nfi\n", conductor_exports(session), config.teardown);
    }
    s += "cd /\n";
    // Its containers and volumes (its database), by project: with its
    // folder or without (deleted by hand).
    if compose_file(&session.repo).is_some() || compose_file(&session.root).is_some() || !session.images.is_empty() {
        s += &format!("env -u COMPOSE_FILE docker compose -p {} down -v --remove-orphans\n", quote(&session.project));
        for image in &session.images {
            s += &format!("docker image rm {} >/dev/null 2>&1 || true\n", quote(&format!("{}-{image}", session.project)));
        }
    }
    match session.kind {
        Kind::Copy => s += &format!("rm -rf {root}\n"),
        Kind::Worktree => {
            let repo = quote(&session.repo.to_string_lossy());
            // Forced twice: it's locked, and its changes go with it (asked before).
            s += &format!(
                "if [ -d {root} ]; then git -C {repo} worktree remove --force --force {root}; \
                 else git -C {repo} worktree unlock {root} 2>/dev/null || true; git -C {repo} worktree prune; fi\n"
            );
            if let (true, Some(branch), Some(start)) = (session.made_branch, &session.branch, &session.start) {
                let tip = format!("$(git -C {repo} rev-parse -q --verify {})", quote(&format!("refs/heads/{branch}")));
                s += &format!("if [ \"{tip}\" = {start} ]; then git -C {repo} branch -q -D {} || true; fi\n", quote(branch));
            }
        }
    }
    if session.compose_file.is_some() {
        s += &format!("rm -f {}\n", quote(&override_path(&session.repo, &session.project).to_string_lossy()));
    }
    s += &format!("{} forget-session {root}\n)\n", quote(&exe.to_string_lossy()));
    s += &format!(
        "remove\nif [ $? = 0 ]; then echo {}; else echo 'Stopped before deleting anything that was left (see above).'; exit 1; fi\n",
        quote(&format!("Session {} removed.", session.name))
    );
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKBONE: &str = r#"services:
  db:
    image: mysql:8.0
    ports:
      - ${DB_PORT:-3306}:3306
    volumes:
      - ./db/initdb.d:/docker-entrypoint-initdb.d:Z
  redis:
    image: redis:7.4
    ports:
      - ${REDIS_PORT:-6379}:6379
  app:
    build:
      context: .
      target: development
    ports:
      - ${APP_PORT:-8080}:8080
  sidekiq:
    build:
      context: .
  sidekiq-web:
    build:
      context: .
    ports:
      - ${SIDEKIQ_WEB_PORT:-8081}:8081
"#;

    #[test]
    fn guesses_backbone() {
        let g = guess(BACKBONE);
        assert_eq!(g.ports, ["DB_PORT", "REDIS_PORT", "APP_PORT", "SIDEKIQ_WEB_PORT"]);
        assert_eq!(g.images, ["app", "sidekiq", "sidekiq-web"]);
        assert_eq!(g.db, Some(("db".into(), "mysql".into())));
        assert!(g.warnings.is_empty(), "{:?}", g.warnings);
    }

    #[test]
    fn warns_about_what_sessions_share() {
        let g = guess(
            "version: '3'\nservices:\n  builder:\n    container_name: portals-builder\n    image: ghcr.io/x\n    ports:\n      - '3000:3000'\nvolumes:\n  data:\n    name: shared-data\n",
        );
        assert!(g.ports.is_empty());
        assert_eq!(g.warnings.len(), 3, "{:?}", g.warnings);
        assert!(g.warnings[0].contains("container name"));
        assert!(g.warnings[1].contains("fixed port 3000"));
        assert!(g.warnings[2].contains("shared-data"));
    }

    #[test]
    fn allocates_free_blocks() {
        let taken: BTreeSet<u16> = [20000, 20001].into();
        assert_eq!(allocate(&taken, 4, |_| true).unwrap(), [20010, 20011, 20012, 20013]);
        // A port in use on the machine moves it to the next block.
        assert_eq!(allocate(&BTreeSet::new(), 2, |p| p != 20001).unwrap(), [20010, 20011]);
        assert_eq!(allocate(&BTreeSet::new(), 0, |_| true).unwrap(), Vec::<u16>::new());
    }

    #[test]
    fn names_compose_projects_like_compose() {
        assert_eq!(project_name("My.App"), "myapp");
        assert_eq!(project_name("backbone-fix_1"), "backbone-fix_1");
    }

    #[test]
    fn scripts_use_the_session() {
        let session = Session {
            name: "a".into(),
            repo: "/code/backbone".into(),
            root: "/code/backbone.sessions/a".into(),
            project: "backbone-a".into(),
            main_project: "backbone".into(),
            ports: vec![("APP_PORT".into(), 20002)],
            images: vec!["app".into()],
            ..Session::default()
        };
        let config = Config { db: "mysql".into(), db_service: "db".into(), ..Config::default() };
        let teardown = teardown_script(&session, &config, Path::new("/bin/pulpo"));
        assert!(teardown.contains("docker compose -p 'backbone-a' down -v --remove-orphans\n"));
        assert!(teardown.contains("\nrm -rf '/code/backbone.sessions/a'"));
        assert!(teardown.contains("'/bin/pulpo' forget-session '/code/backbone.sessions/a'"));
        assert_eq!(env(&session)[0], ("COMPOSE_PROJECT_NAME".into(), "backbone-a".into()));
        assert_eq!(quote("it's"), r"'it'\''s'");
        // Never anything but a session copy.
        let elsewhere = Session { root: "/code/backbone".into(), ..session };
        assert!(!teardown_script(&elsewhere, &config, Path::new("/bin/pulpo")).contains("rm -rf"));
    }

    #[test]
    fn removal_stops_when_a_step_fails() {
        let t = Scratch::new("teardown");
        let root = t.join("r.sessions").join("a");
        let session = Session {
            name: "a".into(),
            repo: t.join("r"),
            root: root.clone(),
            project: "r-a".into(),
            main_project: "r".into(),
            ports: Vec::new(),
            images: Vec::new(),
            ..Session::default()
        };
        let run = |teardown: &str| {
            fs::create_dir_all(&root).unwrap();
            let config = Config { teardown: teardown.into(), ..Config::default() };
            let script = teardown_script(&session, &config, Path::new("/usr/bin/true"));
            // SHELL: the shell it leaves open when it stops.
            std::process::Command::new("sh").args(["-c", &script]).env("SHELL", "/usr/bin/true").output().unwrap();
        };
        run("false");
        assert!(root.is_dir());
        run("true");
        assert!(!root.exists());
    }

    /// Git in a test folder, as a test user; its output.
    fn run_git(dir: &Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .current_dir(dir)
            .args(["-c", "user.email=t@t", "-c", "user.name=Test User", "-c", "init.defaultBranch=main"])
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// A test's own folder, with Pulpo's state and settings in it (out of
    /// the real ones), gone when the test ends, passed or failed.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Scratch {
            let t = std::env::temp_dir().join(format!("pulpo-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&t);
            fs::create_dir_all(&t).unwrap();
            crate::agents::TEST_STATE.set(Some(t.join("state")));
            TEST_CONFIG.set(Some(t.join("config")));
            Scratch(t)
        }
    }

    impl std::ops::Deref for Scratch {
        type Target = Path;
        fn deref(&self) -> &Path {
            &self.0
        }
    }

    impl AsRef<Path> for Scratch {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            crate::agents::TEST_STATE.set(None);
            TEST_CONFIG.set(None);
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn layers_settings_and_asks_for_committed_scripts() {
        let t = Scratch::new("layers");
        let repo = t.join("layered");
        fs::create_dir_all(repo.join(".conductor")).unwrap();
        run_git(&repo, &["init", "-q"]);
        // No compose file: the lockfiles say the install.
        fs::write(repo.join("Gemfile.lock"), "").unwrap();
        fs::write(repo.join("package-lock.json"), "").unwrap();
        let (c, _) = config(&repo, &repo).unwrap();
        assert_eq!(c.setup, "bundle install && npm ci");
        assert_eq!(c.committed, (false, false));
        // The file it wrote says nothing until changed.
        let written = fs::read_to_string(config_path(&repo)).unwrap();
        assert!(written.contains("# setup = \"bundle install && npm ci\""));
        assert_eq!(toml::from_str::<Layer>(&written).unwrap(), Layer::default());

        // Conductor's: committed, then local (ignored by git) over it.
        fs::write(
            repo.join(".conductor/settings.toml"),
            "file_include_globs = \"/config/master.key, .secrets\"\n[scripts]\nsetup = \"bin/setup\"\narchive = \"bin/down\"\n[scripts.run.dev]\ncommand = \"bin/dev\"\n[git]\nbranch_prefix = \"team/\"\n",
        )
        .unwrap();
        run_git(&repo, &["add", ".conductor/settings.toml"]);
        fs::write(repo.join(".conductor/settings.local.toml"), "[scripts]\nsetup = \"bin/mine\"\n").unwrap();
        let (c, _) = config(&repo, &repo).unwrap();
        assert_eq!((c.setup.as_str(), c.teardown.as_str(), c.branch_prefix.as_str()), ("bin/mine", "bin/down", "team"));
        assert_eq!(c.include, ["/config/master.key", ".secrets"]);
        assert_eq!(c.committed, (false, true));
        assert_eq!(branch_name(&repo, "fix", &c.branch_prefix), "team/fix");

        // Committed scripts run once trusted, per repository, by exact text.
        assert_eq!(untrusted(&repo, &c), [("teardown", "bin/down".to_string())]);
        trust(&repo, &untrusted(&repo, &c)).unwrap();
        assert!(untrusted(&repo, &c).is_empty());
        let mut other = c.clone();
        let asked = untrusted(&t.join("elsewhere"), &other);
        assert_eq!(other.without(&asked).len(), 1);
        assert_eq!((other.setup.as_str(), other.teardown.as_str()), ("bin/mine", ""));
        // A local file committed by mistake is the repository's too.
        run_git(&repo, &["add", "-f", ".conductor/settings.local.toml"]);
        assert_eq!(config(&repo, &repo).unwrap().0.committed, (true, true));

        // A file of the first worktree release, every line set: its empty
        // values still say nothing.
        // Both releases' headers, as they wrote them.
        for header in [
            "# with its own Docker project. Guessed from its compose file; edit freely.",
            "# own branch, with its own ports and Docker project. Guessed from its\n# compose file; edit freely.",
        ] {
            fs::write(config_path(&repo), format!("{header}\nports = []\nsetup = \"\"\nteardown = \"\"\n")).unwrap();
            let (c, _) = config(&repo, &repo).unwrap();
            assert_eq!((c.setup.as_str(), c.teardown.as_str()), ("bin/mine", "bin/down"));
        }

        // Pulpo's own file over all of it; an empty value turns one off.
        fs::write(config_path(&repo), "teardown = \"make down\"\nsetup = \"\"\n").unwrap();
        let (c, _) = config(&repo, &repo).unwrap();
        assert_eq!((c.setup.as_str(), c.teardown.as_str(), c.committed), ("", "make down", (false, false)));
    }

    #[test]
    fn a_settings_file_is_yours_only_when_git_says_so() {
        let t = Scratch::new("yours");
        fs::create_dir_all(t.join("repo/evil")).unwrap();
        let repo = t.join("repo");
        run_git(&repo, &["init", "-q"]);
        fs::write(repo.join("evil/settings.toml"), "").unwrap();
        // Committed behind a committed link: git calls it untracked.
        std::os::unix::fs::symlink("evil", repo.join(".conductor")).unwrap();
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "a"]);
        assert!(run_git(&repo, &["ls-files", "--", ".conductor/settings.toml"]).is_empty());
        assert!(!yours(&repo, &repo.join(".conductor/settings.toml")));
        // Plainly untracked: yours. Git unable to say (no repository): not.
        fs::remove_file(repo.join(".conductor")).unwrap();
        fs::create_dir_all(repo.join(".conductor")).unwrap();
        fs::write(repo.join(".conductor/settings.local.toml"), "").unwrap();
        assert!(yours(&repo, &repo.join(".conductor/settings.local.toml")));
        fs::create_dir_all(t.join("plain/.conductor")).unwrap();
        fs::write(t.join("plain/.conductor/settings.toml"), "").unwrap();
        assert!(!yours(&t.join("plain"), &t.join("plain/.conductor/settings.toml")));
        // Committed under another case: the same file on a case-insensitive
        // disk, never yours.
        fs::remove_dir_all(repo.join(".conductor")).unwrap();
        fs::create_dir_all(repo.join(".Conductor")).unwrap();
        fs::write(repo.join(".Conductor/Settings.toml"), "").unwrap();
        run_git(&repo, &["add", "-A"]);
        run_git(&repo, &["commit", "-q", "-m", "b"]);
        assert!(repo.join(".conductor/settings.toml").is_file(), "a case-insensitive disk opens it");
        assert!(!yours(&repo, &repo.join(".conductor/settings.toml")));
        // Your ignored file, then a committed `.Conductor/...` pulled over it
        // with git's ignorecase off: the file looks ignored, yet isn't yours.
        let up = t.join("up");
        fs::create_dir_all(up.join(".Conductor")).unwrap();
        run_git(&up, &["init", "-q"]);
        run_git(&up, &["commit", "-q", "--allow-empty", "-m", "base"]);
        run_git(&t, &["clone", "-q", "up", "down"]);
        fs::write(up.join(".Conductor/settings.local.toml"), "[scripts]\nsetup = \"evil\"\n").unwrap();
        let down = t.join("down");
        run_git(&down, &["config", "core.ignorecase", "false"]);
        fs::write(down.join(".git/info/exclude"), "settings.local.toml\n").unwrap();
        run_git(&up, &["add", "-A"]);
        run_git(&up, &["commit", "-q", "-m", "evil"]);
        fs::create_dir_all(down.join(".conductor")).unwrap();
        fs::write(down.join(".conductor/settings.local.toml"), "mine").unwrap();
        run_git(&down, &["pull", "-q", "origin", "main"]);
        assert!(fs::read_to_string(down.join(".conductor/settings.local.toml")).unwrap().contains("evil"));
        assert!(!yours(&down, &down.join(".conductor/settings.local.toml")));
        // A submodule's file: git says nothing about it, so not yours.
        fs::create_dir_all(t.join("sub/.conductor")).unwrap();
        run_git(&t.join("sub"), &["init", "-q"]);
        fs::write(t.join("sub/settings.toml"), "").unwrap();
        run_git(&t.join("sub"), &["add", "-A"]);
        run_git(&t.join("sub"), &["commit", "-q", "-m", "s"]);
        fs::remove_dir_all(repo.join(".Conductor")).unwrap();
        run_git(&repo, &["rm", "-rq", "--cached", ".Conductor"]);
        run_git(&repo, &["-c", "protocol.file.allow=always", "submodule", "add", "-q", &t.join("sub").to_string_lossy(), ".conductor"]);
        assert!(repo.join(".conductor/settings.toml").is_file());
        assert!(!yours(&repo, &repo.join(".conductor/settings.toml")));
    }

    #[test]
    fn stops_a_fetch_that_hangs_with_what_it_started() {
        let t = Scratch::new("hang");
        fs::create_dir_all(&t).unwrap();
        run_git(&t, &["init", "-q"]);
        // A remote that never answers, and says who it is.
        let hang = t.join("hang.sh");
        fs::write(&hang, format!("#!/bin/sh\necho $$ > {}\nexec sleep 30\n", t.join("pid").display())).unwrap();
        std::process::Command::new("chmod").arg("+x").arg(&hang).status().unwrap();
        let url = format!("ext::{}", hang.display());
        let started = Instant::now();
        let fetched = git(&t, &["-c", "protocol.ext.allow=always", "fetch", &url], Some(Duration::from_secs(1)));
        assert!(fetched.is_err_and(|e| e.to_string().contains("took over")));
        assert!(started.elapsed() < Duration::from_secs(5));
        let pid: i32 = fs::read_to_string(t.join("pid")).unwrap().trim().parse().unwrap();
        std::thread::sleep(Duration::from_millis(200));
        assert!(unsafe { libc::kill(pid, 0) } != 0, "the remote's process outlived the fetch");
    }

    #[test]
    fn names_branches_and_overrides() {
        assert_eq!(remote_host("git@github.com:gigs/backbone.git").as_deref(), Some("github.com"));
        assert_eq!(remote_host("https://github.com/boardchow/pulpo.git").as_deref(), Some("github.com"));
        assert_eq!(remote_host("ssh://git@git.example.com:22/o/r").as_deref(), Some("git.example.com"));
        assert_eq!(remote_host("/tmp/origin.git"), None);
        assert_eq!(remote_host("origin.git"), None);
        // A prefix set for the repository wins, as a branch can spell it.
        assert_eq!(branch_name(Path::new("/nowhere"), "fix", "Valentin/"), "Valentin/fix");
        // Same session name, same project, two repositories: two files.
        assert_ne!(override_path(Path::new("/a/app"), "app-x"), override_path(Path::new("/b/app"), "app-x"));
    }

    #[test]
    fn makes_a_worktree_from_the_remote_and_removes_it() {
        let t = Scratch::new("worktree");
        fs::create_dir_all(t.join("upstream")).unwrap();
        let up = t.join("upstream");
        run_git(&up, &["init", "-q"]);
        fs::write(up.join(".gitignore"), ".env*\nnode_modules/\nsecret.key\n").unwrap();
        fs::write(up.join("a.txt"), "committed").unwrap();
        run_git(&up, &["add", "."]);
        run_git(&up, &["commit", "-q", "-m", "a"]);
        run_git(&t, &["clone", "-q", "--bare", "upstream", "origin.git"]);
        run_git(&t, &["clone", "-q", "origin.git", "app"]);
        let main = t.join("app");
        run_git(&main, &["config", "user.name", "Test User"]);
        // Pushed after main last pulled: the session has it, main doesn't.
        fs::write(up.join("b.txt"), "pushed").unwrap();
        run_git(&up, &["add", "."]);
        run_git(&up, &["commit", "-q", "-m", "b"]);
        run_git(&up, &["push", "-q", "../origin.git", "main"]);
        // Main's own work and ignored files.
        fs::write(main.join("a.txt"), "edited").unwrap();
        for f in [".env", ".env.example", "secret.key", "node_modules/x.js"] {
            fs::create_dir_all(main.join(f).parent().unwrap()).unwrap();
            fs::write(main.join(f), f).unwrap();
        }

        let config = Config { ports: vec!["APP_PORT".into()], include: vec![".env".into(), "secret.key".into()], ..Config::default() };
        let (s, notes) = create(&main, "fix", &config).unwrap();
        let root = t.join("app.sessions/fix");
        assert_eq!(s.root, root);
        assert_eq!((s.kind, s.branch.as_deref(), s.made_branch), (Kind::Worktree, Some("test-user/fix"), true));
        assert_eq!(run_git(&root, &["branch", "--show-current"]), "test-user/fix");
        assert_eq!(fs::read_to_string(root.join("a.txt")).unwrap(), "committed");
        assert_eq!(fs::read_to_string(root.join("b.txt")).unwrap(), "pushed");
        assert!(root.join(".env").is_file() && root.join("secret.key").is_file(), "{notes:?}");
        assert!(!root.join(".env.example").exists() && !root.join("node_modules").exists());
        assert_eq!(fs::read_to_string(main.join("a.txt")).unwrap(), "edited");
        assert!(run_git(&main, &["worktree", "list", "--porcelain"]).contains("locked"));
        // Its block of ten, the compose variable after it.
        let port = s.port.unwrap();
        assert!(s.ports[0].1 >= port + BLOCK);
        let other = create(&main, "other", &Config::default()).unwrap().0;
        let p = other.port.unwrap();
        assert!((p..p + BLOCK).all(|q| !(port..port + BLOCK).contains(&q) && q != s.ports[0].1));
        assert!(env(&s).contains(&("PULPO_PORT".into(), port.to_string())) && env(&s).contains(&("PORT".into(), port.to_string())));
        // The same name again, while it's there.
        assert!(create(&main, "fix", &config).is_err());

        // Untouched, it goes with its branch; with a commit, the branch stays.
        assert!(unsaved(&other).is_empty() && kept_branch(&other).is_none());
        fs::write(root.join("c.txt"), "work").unwrap();
        assert_eq!(unsaved(&s), ["1 uncommitted change(s)"]);
        run_git(&root, &["add", "c.txt"]);
        run_git(&root, &["commit", "-q", "-m", "c"]);
        assert_eq!(kept_branch(&s).as_deref(), Some("test-user/fix"));
        for session in [&s, &other] {
            let script = teardown_script(session, &Config::default(), Path::new("/usr/bin/true"));
            let out = std::process::Command::new("sh").args(["-c", &script]).current_dir(&main).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
            assert!(!session.root.exists());
        }
        let branches = run_git(&main, &["branch", "--list"]);
        assert!(branches.contains("test-user/fix") && !branches.contains("test-user/other"), "{branches}");
        assert!(!run_git(&main, &["worktree", "list"]).contains("sessions"));
        // A folder deleted by hand: the removal still clears the locked
        // worktree and the branch, so the name can be used again.
        let gone = create(&main, "gone", &Config::default()).unwrap().0;
        fs::remove_dir_all(&gone.root).unwrap();
        let script = teardown_script(&gone, &Config::default(), Path::new("/usr/bin/true"));
        let out = std::process::Command::new("sh").args(["-c", &script]).current_dir(&main).output().unwrap();
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stdout));
        assert!(!run_git(&main, &["worktree", "list"]).contains("gone"));
        assert!(!run_git(&main, &["branch", "--list"]).contains("test-user/gone"));
        assert!(create(&main, "gone", &Config::default()).is_ok());
        // Never main itself.
        let main_as_session = Session { root: main.clone(), ..s.clone() };
        assert!(!teardown_script(&main_as_session, &Config::default(), Path::new("/usr/bin/true")).contains("worktree remove"));
    }

    #[test]
    fn mounts_main_git_where_a_service_mounts_the_session() {
        if !std::process::Command::new("docker").args(["compose", "version"]).output().is_ok_and(|o| o.status.success()) {
            eprintln!("skipped: no docker compose");
            return;
        }
        let t = Scratch::new("override");
        fs::create_dir_all(t.join("app/.git")).unwrap();
        fs::create_dir_all(t.join("app.sessions/x")).unwrap();
        let compose = "services:\n  web:\n    image: alpine\n    volumes:\n      - .:/app\n  tool:\n    extends: web\n    profiles: [tool]\n  db:\n    image: alpine\n    volumes:\n      - ./db:/db\n";
        fs::write(t.join("app.sessions/x/compose.yaml"), compose).unwrap();
        let files = compose_override(&t.join("app"), &t.join("app.sessions/x"), "app-x").unwrap().unwrap();
        assert!(files.starts_with(&t.join("app.sessions/x/compose.yaml").to_string_lossy().into_owned()));
        let written: serde_json::Value = serde_json::from_slice(&fs::read(override_path(&t.join("app"), "app-x")).unwrap()).unwrap();
        let services = written["services"].as_object().unwrap();
        assert_eq!(services.keys().collect::<Vec<_>>(), ["tool", "web"]);
        let mount = &services["web"]["volumes"][0];
        assert_eq!(mount["read_only"], true);
        assert_eq!(mount["source"], t.join("app/.git").canonicalize().unwrap().to_string_lossy().as_ref());
        // Nothing mounts the session: nothing to add.
        fs::write(t.join("app.sessions/x/compose.yaml"), "services:\n  db:\n    image: alpine\n").unwrap();
        assert_eq!(compose_override(&t.join("app"), &t.join("app.sessions/x"), "app-x").unwrap(), None);
    }

    #[test]
    fn starts_from_what_it_can_reach() {
        let t = Scratch::new("base");
        fs::create_dir_all(t.join("solo")).unwrap();
        run_git(&t.join("solo"), &["init", "-q"]);
        run_git(&t.join("solo"), &["commit", "-q", "--allow-empty", "-m", "a"]);
        // No remote: main's last commit, nothing to say.
        assert_eq!(base(&t.join("solo")), ("HEAD".to_string(), Vec::<String>::new()));
        // A remote that can't be reached: as last fetched, and said.
        run_git(&t, &["clone", "-q", "solo", "app"]);
        run_git(&t.join("app"), &["remote", "set-url", "origin", &t.join("nowhere").to_string_lossy()]);
        let (from, notes) = base(&t.join("app"));
        assert_eq!(from, "origin/main");
        assert!(notes.len() == 1 && notes[0].contains("couldn't fetch origin"), "{notes:?}");
    }

    #[test]
    fn includes_env_files_and_what_is_named() {
        let t = Scratch::new("include");
        let (main, root) = (t.join("main"), t.join("root"));
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(main.join("secrets")).unwrap();
        run_git(&main, &["init", "-q"]);
        fs::write(main.join(".gitignore"), ".env*\nsecrets/\ncache/\n").unwrap();
        fs::write(main.join(".worktreeinclude"), "secrets/app.key\n!.env.local\n").unwrap();
        for f in [".env", ".env.local", ".env.example", "secrets/app.key", "secrets/other.key", "cache/big"] {
            fs::create_dir_all(main.join(f).parent().unwrap()).unwrap();
            fs::write(main.join(f), f).unwrap();
        }
        let notes = include(&main, &root, &[]);
        assert_eq!(notes, ["from main: .env, secrets/app.key"]);
        assert!(root.join(".env").is_file() && root.join("secrets/app.key").is_file());
        for f in [".env.local", ".env.example", "secrets/other.key", "cache"] {
            assert!(!root.join(f).exists(), "{f}");
        }
    }

    #[test]
    fn counts_only_what_the_copy_alone_has() {
        let t = Scratch::new("unsaved");
        fs::create_dir_all(t.join("main")).unwrap();
        let git = |dir: &str, args: &[&str]| {
            let ok = std::process::Command::new("git")
                .current_dir(t.join(dir))
                .args(["-c", "user.email=t@t", "-c", "user.name=t"])
                .args(args)
                .output()
                .unwrap()
                .status
                .success();
            assert!(ok, "git {args:?}");
        };
        git("main", &["init", "-q"]);
        // Main's own unpushed commit is copied with it: not lost with the copy.
        git("main", &["commit", "-q", "--allow-empty", "-m", "main"]);
        clone(&t.join("main"), &t.join("copy")).unwrap();
        git("copy", &["commit", "-q", "--allow-empty", "-m", "mine"]);
        fs::write(t.join("copy/new.txt"), "x").unwrap();
        let session = Session {
            name: "c".into(),
            repo: t.join("main"),
            root: t.join("copy"),
            project: String::new(),
            main_project: String::new(),
            ports: vec![],
            images: vec![],
            ..Session::default()
        };
        assert_eq!(unsaved(&session), ["1 uncommitted change(s)", "1 commit(s) not pushed"]);
        // A commit on a detached HEAD, and a stash, are lost too.
        git("copy", &["checkout", "-q", "--detach"]);
        git("copy", &["commit", "-q", "--allow-empty", "-m", "detached"]);
        git("copy", &["stash", "-q", "-u"]);
        assert_eq!(unsaved(&session), ["2 commit(s) not pushed", "stashed changes"]);
        // A copy's branches go with it: none is said to stay.
        assert_eq!(kept_branch(&session), None);
    }
}
