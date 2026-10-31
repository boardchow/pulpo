//! Isolated sessions: a copy of a repository's main checkout to work on one
//! task, with its own Docker containers, ports and database, so several
//! tasks (and their agents) run side by side without touching each other.
//!
//! The copy is an APFS clone of the whole checkout (`clonefile`): instant,
//! sharing disk blocks until they change, and exact, from `.env` and hand
//! made settings to installed dependencies. Pulpo runs no git command for
//! it. Docker gets its own Compose project and its own value for each port
//! variable of the compose file, in the environment of the session's
//! terminals. Setting it up (main's images, a copy of main's database) and
//! removing it run as scripts in a terminal, where you see them work.

use std::collections::BTreeSet;
use std::ffi::CString;
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Ports are handed out in blocks of this many, from `FIRST_PORT` up.
const FIRST_PORT: u16 = 20000;
const BLOCK: u16 = 10;
const COMPOSE_FILES: [&str; 4] = ["compose.yaml", "compose.yml", "docker-compose.yaml", "docker-compose.yml"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
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
}

/// How sessions of one repository are set up, in
/// `~/.config/pulpo/repos/<repo>.toml`; guessed from its compose file the
/// first time.
#[derive(Serialize, Deserialize, Default, Debug, PartialEq)]
#[serde(default)]
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
    let from_env = fs::read_to_string(dir.join(".env")).ok().and_then(|env| {
        env.lines().find_map(|l| l.trim().strip_prefix("COMPOSE_PROJECT_NAME=").map(|v| v.trim_matches(|c| c == '"' || c == '\'').to_string()))
    });
    from_env.unwrap_or_else(|| project_name(&dir.file_name().unwrap_or_default().to_string_lossy()))
}

fn project_name(name: &str) -> String {
    name.to_lowercase().chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect()
}

fn config_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("pulpo/repos")
}

pub fn config_path(repo: &Path) -> PathBuf {
    config_dir().join(format!("{}.toml", repo.file_name().unwrap_or_default().to_string_lossy()))
}

/// The repository's session settings, written from its compose file the
/// first time. Also what that file can't isolate.
pub fn config(repo: &Path) -> Result<(Config, Vec<String>)> {
    let compose = compose_file(repo).map(|f| fs::read_to_string(f).unwrap_or_default());
    let guessed = compose.as_deref().map(guess).unwrap_or_default();
    let path = config_path(repo);
    if let Ok(text) = fs::read_to_string(&path) {
        let config = toml::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
        return Ok((config, guessed.warnings));
    }
    let (db_service, db) = guessed.db.clone().unwrap_or_default();
    let fresh = if repo.join("bin/rails").is_file() && guessed.images.iter().any(|s| s == "app") {
        "docker compose exec -T app bin/rails db:prepare"
    } else {
        ""
    };
    let config = Config { ports: guessed.ports.clone(), images: guessed.images.clone(), db, db_service, fresh: fresh.into(), ..Config::default() };
    fs::create_dir_all(config_dir())?;
    fs::write(&path, starter(&config))?;
    Ok((config, guessed.warnings))
}

/// The settings file, with what each line does.
fn starter(c: &Config) -> String {
    let list = |items: &[String]| items.iter().map(|i| format!("{i:?}")).collect::<Vec<_>>().join(", ");
    format!(
        "# How Pulpo sets up a session of this repository: a copy of its checkout
# with its own Docker project. Guessed from its compose file; edit freely.

# Port variables of the compose file; each session gets its own ports.
ports = [{}]
# Services main builds: their images are reused instead of built again.
images = [{}]
# How main's database is copied into the session: mysql, postgres or \"\".
db = {:?}
db_service = {:?}
# Run instead when main's database isn't running.
fresh = {:?}
# Run at the end of the setup, and before a session is removed.
setup = {:?}
teardown = {:?}
",
        list(&c.ports),
        list(&c.images),
        c.db,
        c.db_service,
        c.fresh,
        c.setup,
        c.teardown
    )
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

/// Copy main into a new session and record it. Its setup runs next, in a
/// terminal (`setup_script`).
pub fn create(repo: &Path, name: &str, config: &Config) -> Result<Session> {
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        bail!("a session name is letters, digits, - and _");
    }
    let repo_name = repo.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let parent = repo.parent().context("the repository has no parent folder")?;
    let root = parent.join(format!("{repo_name}.sessions")).join(name);
    if root.exists() {
        bail!("{} already exists", root.display());
    }
    fs::create_dir_all(root.parent().expect("has a parent"))?;
    clone(repo, &root)?;
    // Main's worktrees are main's, not the copy's.
    let _ = fs::remove_dir_all(root.join(".git/worktrees"));

    let main_project = compose_project(repo);
    let result = update(|sessions| {
        let taken: BTreeSet<u16> = sessions.iter().flat_map(|s| s.ports.iter().map(|(_, p)| *p)).collect();
        let ports = allocate(&taken, config.ports.len(), port_free).context("no free ports from 20000 up")?;
        let session = Session {
            name: name.to_string(),
            repo: repo.to_path_buf(),
            root: root.clone(),
            project: project_name(&format!("{main_project}-{name}")),
            main_project: main_project.clone(),
            ports: config.ports.iter().cloned().zip(ports).collect(),
            images: config.images.clone(),
        };
        sessions.push(session.clone());
        Ok(session)
    });
    if result.is_err() {
        let _ = fs::remove_dir_all(&root);
    }
    result
}

/// An APFS clone: every file at once, sharing disk blocks with the original
/// until either side changes.
fn clone(from: &Path, to: &Path) -> Result<()> {
    let (src, dst) = (CString::new(from.as_os_str().as_bytes())?, CString::new(to.as_os_str().as_bytes())?);
    if unsafe { libc::clonefile(src.as_ptr(), dst.as_ptr(), 0) } != 0 {
        let error = io::Error::last_os_error();
        bail!("could not copy {} ({error}); it has to be on an APFS disk", from.display());
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

/// What its terminals get: its own Compose project and ports.
pub fn env(session: &Session) -> Vec<(String, String)> {
    let mut env = vec![("COMPOSE_PROJECT_NAME".to_string(), session.project.clone()), ("PULPO_SESSION".to_string(), session.name.clone())];
    env.extend(session.ports.iter().map(|(var, port)| (var.clone(), port.to_string())));
    env
}

/// What deleting the copy would lose: uncommitted changes, and commits that
/// are on no remote and not in main either (main's own unpushed commits
/// were copied with it, and stay in main). Read-only git.
pub fn unsaved(session: &Session) -> Vec<String> {
    // Its output if it worked; `input` on its stdin.
    let git = |dir: &Path, args: &[&str], input: String| -> Option<String> {
        let mut child = std::process::Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        std::thread::spawn(move || stdin.write_all(input.as_bytes()));
        let out = child.wait_with_output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let mut lost = Vec::new();
    match git(&session.root, &["status", "--porcelain"], String::new()).map(|out| out.lines().count()) {
        Some(0) => {}
        Some(n) => lost.push(format!("{n} uncommitted change(s)")),
        None => lost.push("changes git couldn't check".into()),
    }
    // HEAD too, when there is one: commits on a detached HEAD are on no branch.
    let head = git(&session.root, &["rev-parse", "-q", "--verify", "HEAD"], String::new()).is_some();
    let tips: &[&str] = if head { &["HEAD", "--branches"] } else { &["--branches"] };
    let commits = git(&session.root, &[&["rev-list"], tips, &["--not", "--remotes"]].concat(), String::new());
    let in_main = commits.and_then(|commits| git(&session.repo, &["cat-file", "--batch-check"], commits));
    match in_main.map(|out| out.lines().filter(|line| line.ends_with(" missing")).count()) {
        Some(0) => {}
        Some(n) => lost.push(format!("{n} commit(s) not pushed")),
        None => lost.push("commits git couldn't check".into()),
    }
    let stash = |dir: &Path| git(dir, &["rev-parse", "-q", "--verify", "refs/stash"], String::new());
    if stash(&session.root).is_some_and(|s| Some(&s) != stash(&session.repo).as_ref()) {
        lost.push("stashed changes".into());
    }
    lost
}

/// A folder Pulpo made for a session: `<repo>.sessions/<name>`.
fn is_copy(root: &Path) -> bool {
    root.parent().and_then(Path::file_name).is_some_and(|dir| dir.to_string_lossy().ends_with(".sessions"))
        && root.file_name().is_some_and(|name| !name.is_empty() && name != "..")
}

/// `'text'` for a shell.
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', r"'\''"))
}

/// The setup, run in the session's first terminal with its environment:
/// main's images, its database, the containers, then a shell.
pub fn setup_script(session: &Session, config: &Config, warnings: &[String]) -> String {
    let root = quote(&session.root.to_string_lossy());
    let main = &session.main_project;
    let mut s = String::from("step() { printf '\\n\\033[1m%s\\033[0m\\n' \"$1\"; }\nsetup() (\nset -e\nset -o pipefail\n");
    s += &format!("cd {root}\n");
    s += &format!("step {}\n", quote(&format!("Session {}: a copy of {}", session.name, session.repo.display())));
    for (var, port) in &session.ports {
        s += &format!("echo {}\n", quote(&format!("  {var}={port}")));
    }
    s += &format!("echo {}\n", quote(&format!("  settings: {}", config_path(&session.repo).display())));
    for warning in warnings {
        s += &format!("echo {}\n", quote(&format!("  ! {warning}")));
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
    s += ")\nsetup\nif [ $? = 0 ]; then\nstep 'Ready.'\n";
    for (var, port) in &session.ports {
        s += &format!("echo {}\n", quote(&format!("  {var}: localhost:{port}")));
    }
    s += "else\nprintf '\\n\\033[31mThe setup stopped (see above). Fix it and run those steps here, or remove the session: x x in Sessions.\\033[0m\\n'\nfi\n";
    s += "exec \"${SHELL:-/bin/zsh}\" -l\n";
    s
}

/// The removal: its teardown, its containers and volumes (its database),
/// the image tags made for it, then the copy. Nothing is deleted if Docker
/// can't stop it. `exe` is Pulpo, to take it off the registry at the end.
pub fn teardown_script(session: &Session, config: &Config, exe: &Path) -> String {
    // Only ever a copy Pulpo made: `<repo>.sessions/<name>`.
    if !is_copy(&session.root) {
        return format!("echo {}\n", quote(&format!("Not removing {}: not a session copy", session.root.display())));
    }
    let root = quote(&session.root.to_string_lossy());
    let mut s = format!("remove() (\nset -e\ncd {root}\n");
    if !config.teardown.is_empty() {
        s += &format!("{}\n", config.teardown);
    }
    if compose_file(&session.root).is_some() {
        s += "docker compose down -v --remove-orphans\n";
        for image in &session.images {
            s += &format!("docker image rm {} >/dev/null 2>&1 || true\n", quote(&format!("{}-{image}", session.project)));
        }
    }
    s += &format!("cd / && rm -rf {root}\n{} forget-session {root}\n)\n", quote(&exe.to_string_lossy()));
    s += &format!(
        "remove\nif [ $? = 0 ]; then echo {}; else printf '\\n\\033[31mStopped before deleting anything that was left (see above).\\033[0m\\n'; exec \"${{SHELL:-/bin/zsh}}\" -l; fi\n",
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
        };
        let config = Config { db: "mysql".into(), db_service: "db".into(), ..Config::default() };
        let teardown = teardown_script(&session, &config, Path::new("/bin/pulpo"));
        assert!(teardown.contains("cd / && rm -rf '/code/backbone.sessions/a'"));
        assert!(teardown.contains("'/bin/pulpo' forget-session '/code/backbone.sessions/a'"));
        assert_eq!(env(&session)[0], ("COMPOSE_PROJECT_NAME".into(), "backbone-a".into()));
        assert_eq!(quote("it's"), r"'it'\''s'");
        // Never anything but a session copy.
        let elsewhere = Session { root: "/code/backbone".into(), ..session };
        assert!(!teardown_script(&elsewhere, &config, Path::new("/bin/pulpo")).contains("rm -rf"));
    }

    #[test]
    fn removal_stops_when_a_step_fails() {
        let t = std::env::temp_dir().join(format!("pulpo-teardown-{}", std::process::id()));
        let root = t.join("r.sessions").join("a");
        let session = Session {
            name: "a".into(),
            repo: t.join("r"),
            root: root.clone(),
            project: "r-a".into(),
            main_project: "r".into(),
            ports: Vec::new(),
            images: Vec::new(),
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
        let _ = fs::remove_dir_all(&t);
    }

    #[test]
    fn counts_only_what_the_copy_alone_has() {
        let t = std::env::temp_dir().join(format!("pulpo-unsaved-{}", std::process::id()));
        let _ = fs::remove_dir_all(&t);
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
        };
        assert_eq!(unsaved(&session), ["1 uncommitted change(s)", "1 commit(s) not pushed"]);
        // A commit on a detached HEAD, and a stash, are lost too.
        git("copy", &["checkout", "-q", "--detach"]);
        git("copy", &["commit", "-q", "--allow-empty", "-m", "detached"]);
        git("copy", &["stash", "-q", "-u"]);
        assert_eq!(unsaved(&session), ["2 commit(s) not pushed", "stashed changes"]);
        fs::remove_dir_all(&t).unwrap();
    }
}
