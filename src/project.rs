use std::fs;
use std::path::{Path, PathBuf};

fn registry_path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("pulpo").join("projects")
}

pub fn registered() -> Vec<PathBuf> {
    fs::read_to_string(registry_path())
        .unwrap_or_default()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(PathBuf::from)
        .collect()
}

pub fn register(repo: &Path) {
    let mut repos = registered();
    repos.retain(|r| r != repo);
    repos.insert(0, repo.to_path_buf());
    save(&repos);
}

pub fn unregister(repo: &Path) {
    let mut repos = registered();
    repos.retain(|r| r != repo);
    save(&repos);
}

fn save(repos: &[PathBuf]) {
    let path = registry_path();
    if let Some(dir) = path.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let lines: Vec<String> = repos.iter().map(|r| r.display().to_string()).collect();
    let _ = fs::write(path, lines.join("\n") + "\n");
}
