use std::fmt;
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

/// The character says whether shift was down.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Key {
    pub fn of(event: KeyEvent) -> Key {
        let mut mods = event.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT | KeyModifiers::SUPER);
        let code = match event.code {
            KeyCode::Char(c) => {
                let c = if mods.contains(KeyModifiers::SHIFT) { c.to_ascii_uppercase() } else { c };
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::Char(c)
            }
            KeyCode::Tab if mods.contains(KeyModifiers::SHIFT) => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            KeyCode::BackTab => {
                mods.remove(KeyModifiers::SHIFT);
                KeyCode::BackTab
            }
            code => code,
        };
        Key { code, mods }
    }

    pub fn parse(text: &str) -> Option<Key> {
        let mut mods = KeyModifiers::NONE;
        let mut rest = text;
        loop {
            let lower = rest.to_ascii_lowercase();
            let (modifier, len) = match () {
                _ if lower.starts_with("ctrl-") => (KeyModifiers::CONTROL, 5),
                _ if lower.starts_with("alt-") || lower.starts_with("opt-") => (KeyModifiers::ALT, 4),
                _ if lower.starts_with("shift-") => (KeyModifiers::SHIFT, 6),
                _ if lower.starts_with("cmd-") => (KeyModifiers::SUPER, 4),
                _ if lower.starts_with("super-") => (KeyModifiers::SUPER, 6),
                _ => break,
            };
            mods |= modifier;
            rest = &rest[len..];
        }
        let code = match rest.to_ascii_lowercase().as_str() {
            "space" => KeyCode::Char(' '),
            "tab" => KeyCode::Tab,
            "enter" | "return" => KeyCode::Enter,
            "esc" | "escape" => KeyCode::Esc,
            "backspace" => KeyCode::Backspace,
            "left" => KeyCode::Left,
            "right" => KeyCode::Right,
            "up" => KeyCode::Up,
            "down" => KeyCode::Down,
            _ => {
                let mut chars = rest.chars();
                let c = chars.next()?;
                if chars.next().is_some() {
                    return None;
                }
                KeyCode::Char(c)
            }
        };
        Some(Key::of(KeyEvent::new(code, mods)))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (modifier, glyph) in [(KeyModifiers::CONTROL, "⌃"), (KeyModifiers::ALT, "⌥"), (KeyModifiers::SHIFT, "⇧"), (KeyModifiers::SUPER, "⌘")] {
            if self.mods.contains(modifier) {
                f.write_str(glyph)?;
            }
        }
        match self.code {
            KeyCode::Char(' ') => f.write_str("␣"),
            KeyCode::Char(c) => write!(f, "{c}"),
            KeyCode::Tab => f.write_str("⇥"),
            KeyCode::BackTab => f.write_str("⇧⇥"),
            KeyCode::Enter => f.write_str("⏎"),
            KeyCode::Esc => f.write_str("esc"),
            code => write!(f, "{}", format!("{code:?}").to_lowercase()),
        }
    }
}

pub struct Keymap {
    pub prefix: Vec<Key>,
}

impl Default for Keymap {
    fn default() -> Self {
        // ⌘p is free in Ghostty, macOS and every program; ⌃b for terminals that keep ⌘ to themselves.
        Keymap { prefix: ["cmd-p", "ctrl-b"].iter().filter_map(|k| Key::parse(k)).collect() }
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Keys {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct File {
    prefix: Option<Keys>,
    /// What the prefix was called before.
    pulpo: Option<Keys>,
    /// Tables of earlier versions, accepted so an old file still loads.
    /// loads from an old file.
    #[serde(rename = "after_prefix")]
    _after_prefix: Option<toml::Value>,
    #[serde(rename = "chords")]
    _chords: Option<toml::Value>,
    #[serde(rename = "legacy_ctrl_keys")]
    _legacy_ctrl_keys: Option<toml::Value>,
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("pulpo/keys.toml")
}

impl Keymap {
    pub fn load() -> (Keymap, Option<String>) {
        let path = path();
        match std::fs::read_to_string(&path) {
            Ok(text) => match Keymap::from_toml(&text) {
                Ok(keymap) => (keymap, None),
                Err(e) => (Keymap::default(), Some(format!("{}: {e}; using the default keys", path.display()))),
            },
            Err(_) => (Keymap::default(), None),
        }
    }

    fn from_toml(text: &str) -> Result<Keymap, String> {
        let file: File = toml::from_str(text).map_err(|e| e.message().to_string())?;
        let mut keymap = Keymap::default();
        if let Some(keys) = file.prefix.or(file.pulpo) {
            let list = match &keys {
                Keys::One(k) => vec![k.as_str()],
                Keys::Many(ks) => ks.iter().map(String::as_str).collect(),
            };
            keymap.prefix = list.into_iter().map(|k| Key::parse(k).ok_or_else(|| format!("unknown key {k:?}"))).collect::<Result<_, _>>()?;
        }
        Ok(keymap)
    }

    pub fn is_prefix(&self, key: Key) -> bool {
        // Without the kitty keyboard protocol, ⌃] arrives as ⌃5.
        let legacy = (key == Key { code: KeyCode::Char('5'), mods: KeyModifiers::CONTROL }).then(|| Key::parse("ctrl-]")).flatten();
        self.prefix.contains(&key) || legacy.is_some_and(|k| self.prefix.contains(&k))
    }

    pub fn label(&self) -> String {
        self.prefix.first().map(Key::to_string).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(text: &str) -> Key {
        Key::parse(text).unwrap()
    }

    #[test]
    fn parses_and_shows_keys() {
        assert_eq!(key("ctrl-space"), Key::of(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::CONTROL)));
        assert_eq!(key("ctrl-space").to_string(), "⌃␣");
        assert_eq!(key("cmd-p").to_string(), "⌘p");
        assert_eq!(key("cmd-p"), Key::of(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::SUPER)));
        assert_eq!(key("ctrl-]").to_string(), "⌃]");
        assert_eq!(key("N"), Key::of(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::SHIFT)));
        assert_eq!(key("shift-tab").code, KeyCode::BackTab);
        assert!(Key::parse("ctrl-nope").is_none());
    }

    #[test]
    fn keys_toml_sets_the_prefix() {
        let keymap = Keymap::from_toml("prefix = \"ctrl-a\"").unwrap();
        assert_eq!(keymap.prefix, vec![key("ctrl-a")]);
        assert_eq!(Keymap::from_toml("pulpo = [\"cmd-j\"]").unwrap().prefix, vec![key("cmd-j")]);
        assert_eq!(Keymap::default().label(), "⌘p");
        assert!(Keymap::default().is_prefix(key("ctrl-b")));
        assert!(Keymap::from_toml("leader = \"ctrl-b\"").is_err());
        let old = "prefix = \"ctrl-a\"\nlegacy_ctrl_keys = true\n[after_prefix]\nn = \"new\"\n[chords]\nx = \"alt-1\"\n";
        assert_eq!(Keymap::from_toml(old).unwrap().prefix, vec![key("ctrl-a")]);
    }
}
