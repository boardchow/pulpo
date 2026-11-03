//! `pulpo doctor`: whether the prefix reaches Pulpo, and whether
//! Claude reports to it.

use std::path::PathBuf;
use std::process::Command;

use crate::keymap::{self, Key, Keymap};

/// `pulpo keytest`: what Pulpo gets for each key pressed, and whether it's
/// the prefix; q stops.
pub fn keytest() -> anyhow::Result<()> {
    use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags};
    use ratatui::crossterm::{execute, terminal};
    let (keymap, _) = Keymap::load();
    let keys: Vec<String> = keymap.prefix.iter().map(Key::to_string).collect();
    println!("Press keys to see what Pulpo gets (as in Pulpo). The prefix: {}. q stops.\r", keys.join(" or "));
    terminal::enable_raw_mode()?;
    execute!(std::io::stdout(), PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES))?;
    let result = (|| -> anyhow::Result<()> {
        loop {
            if let Event::Key(event) = event::read()?
                && event.kind == KeyEventKind::Press
            {
                if event.code == KeyCode::Char('q') && event.modifiers.is_empty() {
                    return Ok(());
                }
                let key = Key::of(event);
                let note = if keymap.is_prefix(key) { "  ← the prefix" } else { "" };
                println!("  {key}{note}\r");
            }
        }
    })();
    execute!(std::io::stdout(), PopKeyboardEnhancementFlags)?;
    terminal::disable_raw_mode()?;
    result
}

pub fn doctor(_args: &[String]) -> anyhow::Result<()> {
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
    let keys: Vec<String> = keymap.prefix.iter().map(Key::to_string).collect();
    println!("  the prefix (then w s t f c, n x…): {}", keys.join(" or "));
    let ctrl_space = Key::parse("ctrl-space");
    if keymap.prefix.iter().any(|k| Some(*k) == ctrl_space)
        && let Some(layouts) = input_switch_on_ctrl_space()
    {
        // Only a problem when ⌃␣ is all there is.
        ok &= keymap.prefix.len() > 1;
        let others: Vec<&str> = keys.iter().map(String::as_str).filter(|k| *k != "⌃␣").collect();
        let fallback = if others.is_empty() { "nothing else".to_string() } else { others.join(" or ") };
        println!("  · macOS switches between your {layouts} input sources on ⌃␣, so Pulpo never gets it; {fallback} work.");
        println!("    To free ⌃␣: System Settings › Keyboard › Keyboard Shortcuts › Input Sources,");
        println!("    turn off \"Select the previous input source\" (or give it ⌃⌥Space).");
    }

    println!("  `pulpo keytest` shows what reaches Pulpo when you press a key.");

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
pub fn ghostty_configs() -> Vec<PathBuf> {
    let xdg = std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home().join(".config"));
    let support = home().join("Library/Application Support/com.mitchellh.ghostty");
    [xdg.join("ghostty/config"), xdg.join("ghostty/config.ghostty"), support.join("config"), support.join("config.ghostty")]
        .into_iter()
        .filter(|p| p.is_file())
        .collect()
}

