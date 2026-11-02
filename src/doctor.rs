//! `pulpo doctor`: whether your keys reach Pulpo, and whether Claude
//! reports to it. `--fix` changes what it can: Ghostty's Option key.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::keymap::{self, Key, Keymap};

pub fn doctor(args: &[String]) -> anyhow::Result<()> {
    let fix = args.iter().any(|a| a == "--fix");
    let mut ok = true;

    let (keymap, error) = Keymap::load();
    let path = keymap::path();
    println!("Keys  {}", crate::agents::shorten(&path));
    match error {
        Some(e) => {
            ok = false;
            println!("  ✗ {e}");
        }
        None if path.exists() => println!("  ✓ read"),
        None => println!("  ✓ the defaults (no keys.toml)"),
    }
    let prefixes: Vec<String> = keymap.prefix.iter().map(Key::to_string).collect();
    println!("  prefix {}", prefixes.join(" or "));
    let ctrl_space = Key::parse("ctrl-space");
    if keymap.prefix.iter().any(|k| Some(*k) == ctrl_space)
        && let Some(layouts) = input_switch_on_ctrl_space()
    {
        ok = false;
        let others: Vec<&str> = prefixes.iter().map(String::as_str).filter(|p| *p != "⌃␣").collect();
        let fallback = if others.is_empty() { "nothing else".to_string() } else { others.join(" or ") };
        println!("  ✗ macOS switches between your {layouts} input sources on ⌃␣, so Pulpo never gets it; {fallback} still works.");
        println!("    To free ⌃␣: System Settings › Keyboard › Keyboard Shortcuts › Input Sources,");
        println!("    turn off \"Select the previous input source\".");
    }

    let configs = ghostty_configs();
    let written = configs.first().cloned().unwrap_or_else(|| home().join(".config/ghostty/config"));
    println!("Ghostty  {}", crate::agents::shorten(&written));
    match option_as_alt(&configs) {
        Some(value) if value != "false" => println!("  ✓ macos-option-as-alt = {value}: ⌥1-9 and ⌥↑↓ reach Pulpo"),
        _ if fix => {
            add_option_as_alt(&written)?;
            println!("  ✓ added macos-option-as-alt = left; reload Ghostty's config (⌘⇧,) or open a new window");
        }
        _ => {
            ok = false;
            println!("  ✗ macos-option-as-alt is not set: ⌥1-9 and ⌥↑↓ type characters instead of reaching Pulpo");
            println!("    pulpo doctor --fix sets it to left, so the right Option key still types accents");
        }
    }

    println!("Claude Code");
    if crate::agents::hooks_installed() {
        println!("  ✓ hooks installed: Pulpo sees what Claude runs and asks");
    } else {
        ok = false;
        println!("  ✗ hooks missing: pulpo hooks install");
    }
    if ok {
        println!("All good.");
    }
    Ok(())
}

fn home() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_default())
}

/// The input sources ⌃Space cycles through, when macOS has that shortcut
/// on and there is more than one.
fn input_switch_on_ctrl_space() -> Option<usize> {
    let read = |domain: &str, key: &str| {
        Command::new("defaults").args(["read", domain, key]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
    };
    let sources = read("com.apple.HIToolbox", "AppleEnabledInputSources")?;
    let layouts = sources.matches("\"KeyboardLayout Name\"").count() + sources.matches("\"Input Mode\"").count();
    if layouts < 2 {
        return None;
    }
    // Shortcut 60, "Select the previous input source": on, and ⌃Space
    // (space is key code 49, control is 262144), unless you changed it.
    let hotkeys = read("com.apple.symbolichotkeys", "AppleSymbolicHotKeys").unwrap_or_default();
    let Some(start) = hotkeys.find("    60 =") else { return Some(layouts) };
    let entry = &hotkeys[start..];
    let entry = &entry[..entry[1..].find("\n    6").map_or(entry.len(), |i| i + 1)];
    let on = entry.contains("enabled = 1");
    let ctrl_space = entry.contains("49,") && entry.contains("262144");
    (on && ctrl_space).then_some(layouts)
}

/// Ghostty's config files, in the order it reads them.
fn ghostty_configs() -> Vec<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    let support = home().join("Library/Application Support/com.mitchellh.ghostty");
    [xdg.join("ghostty/config"), xdg.join("ghostty/config.ghostty"), support.join("config"), support.join("config.ghostty")]
        .into_iter()
        .filter(|p| p.is_file())
        .collect()
}

/// The last `macos-option-as-alt` in those files.
fn option_as_alt(configs: &[PathBuf]) -> Option<String> {
    let mut value = None;
    for path in configs {
        for line in std::fs::read_to_string(path).unwrap_or_default().lines() {
            if let Some((key, v)) = line.split_once('=')
                && key.trim() == "macos-option-as-alt"
            {
                value = Some(v.trim().to_string());
            }
        }
    }
    value
}

fn add_option_as_alt(path: &Path) -> anyhow::Result<()> {
    let mut text = std::fs::read_to_string(path).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str("\n# Pulpo: the left Option key is Alt (⌥1-9, ⌥↑↓); the right one still types accents.\nmacos-option-as-alt = left\n");
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, text)?;
    Ok(())
}
