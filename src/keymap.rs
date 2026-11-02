//! Pulpo's keys beyond a panel's own, in one table: the prefix (⌃Space or
//! ⌃b, then a key), the chords that work without it, and how both show in
//! the footer, the prefix panel and the help. The grammar is Omarchy's tmux
//! one. `~/.config/pulpo/keys.toml` changes any of it.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use serde::Deserialize;

/// A key as Pulpo compares them: the character says whether shift was down.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct Key {
    pub code: KeyCode,
    pub mods: KeyModifiers,
}

impl Key {
    pub fn of(event: KeyEvent) -> Key {
        let mut mods = event.modifiers & (KeyModifiers::CONTROL | KeyModifiers::ALT | KeyModifiers::SHIFT);
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

    /// `ctrl-space`, `alt-1`, `ctrl-alt-left`, `N`, `?`, `tab`…
    pub fn parse(text: &str) -> Option<Key> {
        let mut mods = KeyModifiers::NONE;
        let mut rest = text;
        loop {
            let lower = rest.to_ascii_lowercase();
            let (modifier, len) = match () {
                _ if lower.starts_with("ctrl-") => (KeyModifiers::CONTROL, 5),
                _ if lower.starts_with("alt-") || lower.starts_with("opt-") => (KeyModifiers::ALT, 4),
                _ if lower.starts_with("shift-") => (KeyModifiers::SHIFT, 6),
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

    fn arrow(self) -> bool {
        matches!(self.code, KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down)
    }

    fn digit(self) -> Option<u32> {
        match self.code {
            KeyCode::Char(c) => c.to_digit(10),
            _ => None,
        }
    }

    fn mods_label(self) -> String {
        let mut out = String::new();
        for (modifier, glyph) in [(KeyModifiers::CONTROL, "⌃"), (KeyModifiers::ALT, "⌥"), (KeyModifiers::SHIFT, "⇧")] {
            if self.mods.contains(modifier) {
                out.push_str(glyph);
            }
        }
        out
    }

    fn code_label(self) -> String {
        match self.code {
            KeyCode::Char(' ') => "␣".into(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Tab => "⇥".into(),
            KeyCode::BackTab => "⇧⇥".into(),
            KeyCode::Enter => "⏎".into(),
            KeyCode::Esc => "esc".into(),
            KeyCode::Backspace => "⌫".into(),
            KeyCode::Left => "←".into(),
            KeyCode::Right => "→".into(),
            KeyCode::Up => "↑".into(),
            KeyCode::Down => "↓".into(),
            code => format!("{code:?}").to_lowercase(),
        }
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}{}", self.mods_label(), self.code_label())
    }
}

/// Several keys as one label: `←↑→↓`, `⌥1-9`, `⌃⌥←→`, else one after another.
pub fn label(keys: &[Key]) -> String {
    let Some(&first) = keys.first() else { return String::new() };
    let same_mods = keys.iter().all(|k| k.mods == first.mods);
    if same_mods && keys.len() > 1 && keys.iter().all(|k| k.arrow()) {
        return first.mods_label() + &keys.iter().map(|k| k.code_label()).collect::<String>();
    }
    let digits: Option<Vec<u32>> = keys.iter().map(|k| k.digit()).collect();
    if let Some(digits) = digits.filter(|d| same_mods && d.len() > 2 && d.windows(2).all(|w| w[1] == w[0] + 1)) {
        return format!("{}{}-{}", first.mods_label(), digits[0], digits[digits.len() - 1]);
    }
    keys.iter().map(Key::to_string).collect::<Vec<_>>().join(" ")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    FocusLeft,
    FocusUp,
    FocusRight,
    FocusDown,
    SwapLeft,
    SwapUp,
    SwapRight,
    SwapDown,
    ResizeLeft,
    ResizeUp,
    ResizeRight,
    ResizeDown,
    SplitBeside,
    SplitBelow,
    Layout,
    Zoom,
    ClosePane,
    NewTab,
    /// Tab 1 to 9.
    Tab(u8),
    NextTab,
    PreviousTab,
    RenameTab,
    Scroll,
    CloseTab,
    NewSession,
    Sessions,
    NextSession,
    PreviousSession,
    DeleteSession,
    AddWorkspace,
    Swarm,
    Detach,
    Goto,
    NextWaiting,
    LastPlace,
    Menu,
    Theme,
    Board,
    /// Open the link or file under the mouse (from a right click).
    OpenLink,
    Files,
    SearchFiles,
    Sidebar,
    Help,
}

use Action::*;

pub const TABS: [Action; 9] = [Tab(1), Tab(2), Tab(3), Tab(4), Tab(5), Tab(6), Tab(7), Tab(8), Tab(9)];

impl Action {
    /// Its name in keys.toml.
    pub fn name(self) -> String {
        match self {
            FocusLeft => "focus_left".into(),
            FocusUp => "focus_up".into(),
            FocusRight => "focus_right".into(),
            FocusDown => "focus_down".into(),
            SwapLeft => "swap_left".into(),
            SwapUp => "swap_up".into(),
            SwapRight => "swap_right".into(),
            SwapDown => "swap_down".into(),
            ResizeLeft => "resize_left".into(),
            ResizeUp => "resize_up".into(),
            ResizeRight => "resize_right".into(),
            ResizeDown => "resize_down".into(),
            SplitBeside => "split_beside".into(),
            SplitBelow => "split_below".into(),
            Layout => "layout".into(),
            Zoom => "zoom".into(),
            ClosePane => "close_pane".into(),
            NewTab => "new_tab".into(),
            Tab(n) => format!("tab_{n}"),
            NextTab => "next_tab".into(),
            PreviousTab => "previous_tab".into(),
            RenameTab => "rename_tab".into(),
            Scroll => "scroll".into(),
            CloseTab => "close_tab".into(),
            NewSession => "new_session".into(),
            Sessions => "sessions".into(),
            NextSession => "next_session".into(),
            PreviousSession => "previous_session".into(),
            DeleteSession => "delete_session".into(),
            AddWorkspace => "add_workspace".into(),
            Swarm => "swarm".into(),
            Detach => "detach".into(),
            Goto => "goto".into(),
            NextWaiting => "next_waiting".into(),
            LastPlace => "last_place".into(),
            Menu => "menu".into(),
            Theme => "theme".into(),
            Board => "board".into(),
            OpenLink => "open_link".into(),
            Files => "files".into(),
            SearchFiles => "search_files".into(),
            Sidebar => "sidebar".into(),
            Help => "help".into(),
        }
    }

    fn named(name: &str) -> Option<Action> {
        ROWS.iter().flat_map(|row| row.actions).copied().find(|a| a.name() == name)
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Group {
    Panes,
    Tabs,
    Sessions,
    Go,
}

impl Group {
    pub const ALL: [Group; 4] = [Group::Panes, Group::Tabs, Group::Sessions, Group::Go];

    pub fn title(self) -> &'static str {
        match self {
            Group::Panes => "PANES",
            Group::Tabs => "TABS",
            Group::Sessions => "SESSIONS",
            Group::Go => "GO",
        }
    }
}

/// A line of the prefix panel and the help: its actions share one label.
pub struct Row {
    pub group: Group,
    pub label: &'static str,
    pub actions: &'static [Action],
}

/// Every action, in the order the panel and the help show them.
pub const ROWS: &[Row] = &[
    Row { group: Group::Panes, label: "focus that way", actions: &[FocusLeft, FocusUp, FocusRight, FocusDown] },
    Row { group: Group::Panes, label: "swap that way", actions: &[SwapLeft, SwapUp, SwapRight, SwapDown] },
    Row { group: Group::Panes, label: "resize", actions: &[ResizeLeft, ResizeUp, ResizeRight, ResizeDown] },
    Row { group: Group::Panes, label: "split beside", actions: &[SplitBeside] },
    Row { group: Group::Panes, label: "split below", actions: &[SplitBelow] },
    Row { group: Group::Panes, label: "next layout", actions: &[Layout] },
    Row { group: Group::Panes, label: "zoom", actions: &[Zoom] },
    Row { group: Group::Panes, label: "close (asks first)", actions: &[ClosePane] },
    Row { group: Group::Tabs, label: "new tab, a shell", actions: &[NewTab] },
    Row { group: Group::Tabs, label: "go to tab", actions: &TABS },
    Row { group: Group::Tabs, label: "previous / next", actions: &[PreviousTab, NextTab] },
    Row { group: Group::Tabs, label: "rename", actions: &[RenameTab] },
    Row { group: Group::Tabs, label: "scroll back, find", actions: &[Scroll] },
    Row { group: Group::Tabs, label: "close (asks first)", actions: &[CloseTab] },
    Row { group: Group::Sessions, label: "new task session", actions: &[NewSession] },
    Row { group: Group::Sessions, label: "list sessions", actions: &[Sessions] },
    Row { group: Group::Sessions, label: "previous / next", actions: &[PreviousSession, NextSession] },
    Row { group: Group::Sessions, label: "delete session", actions: &[DeleteSession] },
    Row { group: Group::Sessions, label: "add workspace", actions: &[AddWorkspace] },
    Row { group: Group::Sessions, label: "swarm: N agents", actions: &[Swarm] },
    Row { group: Group::Sessions, label: "detach (quit)", actions: &[Detach] },
    Row { group: Group::Go, label: "every agent, terminal", actions: &[Goto] },
    Row { group: Group::Go, label: "next one to read", actions: &[NextWaiting] },
    Row { group: Group::Go, label: "back to the last place", actions: &[LastPlace] },
    Row { group: Group::Go, label: "menu: every action", actions: &[Menu] },
    Row { group: Group::Go, label: "theme", actions: &[Theme] },
    Row { group: Group::Go, label: "board: all, tiled", actions: &[Board] },
    Row { group: Group::Go, label: "files", actions: &[Files] },
    Row { group: Group::Go, label: "search in files", actions: &[SearchFiles] },
    Row { group: Group::Go, label: "hide / show sidebar", actions: &[Sidebar] },
    Row { group: Group::Go, label: "every key", actions: &[Help] },
];

/// The menu: actions by name, those with a direction or a number left out.
pub const MENU: &[(Group, Action, &str)] = &[
    (Group::Go, Goto, "Go to an agent or a terminal"),
    (Group::Go, NextWaiting, "Go to the next agent with something to read"),
    (Group::Go, LastPlace, "Back to the last tab or session"),
    (Group::Go, Files, "Files"),
    (Group::Go, SearchFiles, "Search inside files"),
    (Group::Go, Sidebar, "Hide or show the sidebar"),
    (Group::Go, Theme, "Theme: follow Ghostty, or pick one"),
    (Group::Go, Board, "Board: every terminal of this repository, tiled"),
    (Group::Go, Help, "Every key"),
    (Group::Panes, SplitBeside, "Split: a shell beside"),
    (Group::Panes, SplitBelow, "Split: a shell below"),
    (Group::Panes, Layout, "Next layout: main pane left, side by side, stacked, grid, review"),
    (Group::Panes, Zoom, "Zoom the pane, or back"),
    (Group::Panes, ClosePane, "Close the pane"),
    (Group::Tabs, NewTab, "New tab with a shell"),
    (Group::Tabs, NextTab, "Next tab"),
    (Group::Tabs, PreviousTab, "Previous tab"),
    (Group::Tabs, RenameTab, "Rename the tab"),
    (Group::Tabs, Scroll, "Scroll back through the terminal, find in it"),
    (Group::Tabs, CloseTab, "Close the tab"),
    (Group::Sessions, NewSession, "New task session: its own copy, Docker and ports"),
    (Group::Sessions, Sessions, "Sessions"),
    (Group::Sessions, NextSession, "Next session"),
    (Group::Sessions, PreviousSession, "Previous session"),
    (Group::Sessions, DeleteSession, "Delete the task session"),
    (Group::Sessions, AddWorkspace, "Add a workspace"),
    (Group::Sessions, Swarm, "Swarm: N task sessions, an agent in each, on the board"),
    (Group::Sessions, Detach, "Detach: quit, terminals keep running"),
];

/// After the prefix.
fn default_keys() -> Vec<(Key, Action)> {
    let mut keys: Vec<(&str, Action)> = vec![
        ("left", FocusLeft),
        ("up", FocusUp),
        ("right", FocusRight),
        ("down", FocusDown),
        ("shift-left", SwapLeft),
        ("shift-up", SwapUp),
        ("shift-right", SwapRight),
        ("shift-down", SwapDown),
        ("alt-left", ResizeLeft),
        ("alt-up", ResizeUp),
        ("alt-right", ResizeRight),
        ("alt-down", ResizeDown),
        ("v", SplitBeside),
        ("h", SplitBelow),
        ("L", Layout),
        ("[", Scroll),
        ("S", Swarm),
        ("B", Board),
        ("-", SplitBelow),
        ("z", Zoom),
        ("x", ClosePane),
        ("c", NewTab),
        ("n", NextTab),
        ("p", PreviousTab),
        ("r", RenameTab),
        ("k", CloseTab),
        ("C", NewSession),
        ("s", Sessions),
        ("N", NextSession),
        ("P", PreviousSession),
        ("K", DeleteSession),
        ("W", AddWorkspace),
        ("d", Detach),
        ("f", Files),
        ("g", Goto),
        ("a", NextWaiting),
        ("tab", LastPlace),
        ("space", Menu),
        ("T", Theme),
        ("/", SearchFiles),
        ("e", Sidebar),
        ("?", Help),
    ];
    let digits: Vec<String> = (1..=9).map(|n| n.to_string()).collect();
    keys.extend(digits.iter().map(String::as_str).zip(TABS));
    keys.into_iter().filter_map(|(k, a)| Some((Key::parse(k)?, a))).collect()
}

/// Without the prefix, from anywhere.
fn default_chords() -> Vec<(Key, Action)> {
    let mut chords: Vec<(String, Action)> = vec![
        ("ctrl-alt-left".into(), FocusLeft),
        ("ctrl-alt-up".into(), FocusUp),
        ("ctrl-alt-right".into(), FocusRight),
        ("ctrl-alt-down".into(), FocusDown),
        ("ctrl-alt-shift-left".into(), ResizeLeft),
        ("ctrl-alt-shift-up".into(), ResizeUp),
        ("ctrl-alt-shift-right".into(), ResizeRight),
        ("ctrl-alt-shift-down".into(), ResizeDown),
        ("alt-up".into(), PreviousSession),
        ("alt-down".into(), NextSession),
    ];
    chords.extend((1..=9).map(|n| (format!("alt-{n}"), Tab(n))));
    chords.into_iter().filter_map(|(k, a)| Some((Key::parse(&k)?, a))).collect()
}

pub struct Keymap {
    pub prefix: Vec<Key>,
    /// The ctrl keys Pulpo used to take inside terminals still work.
    pub legacy: bool,
    keys: Vec<(Key, Action)>,
    chords: Vec<(Key, Action)>,
}

impl Default for Keymap {
    fn default() -> Self {
        Keymap {
            prefix: ["ctrl-space", "ctrl-b"].iter().filter_map(|k| Key::parse(k)).collect(),
            legacy: true,
            keys: default_keys(),
            chords: default_chords(),
        }
    }
}

/// One key or several, in keys.toml.
#[derive(Deserialize)]
#[serde(untagged)]
enum Keys {
    One(String),
    Many(Vec<String>),
}

impl Keys {
    fn list(&self) -> Vec<&str> {
        match self {
            Keys::One(k) => vec![k.as_str()],
            Keys::Many(ks) => ks.iter().map(String::as_str).collect(),
        }
    }
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
struct File {
    prefix: Option<Keys>,
    legacy_ctrl_keys: Option<bool>,
    after_prefix: HashMap<String, Keys>,
    chords: HashMap<String, Keys>,
}

pub fn path() -> PathBuf {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config"));
    base.join("pulpo/keys.toml")
}

impl Keymap {
    /// The defaults with keys.toml on top, and what was wrong in it.
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
        let parse = |keys: &Keys| -> Result<Vec<Key>, String> {
            keys.list().into_iter().map(|k| Key::parse(k).ok_or_else(|| format!("unknown key {k:?}"))).collect()
        };
        if let Some(prefix) = &file.prefix {
            keymap.prefix = parse(prefix)?;
        }
        if let Some(legacy) = file.legacy_ctrl_keys {
            keymap.legacy = legacy;
        }
        for (table, overrides) in [(&mut keymap.keys, &file.after_prefix), (&mut keymap.chords, &file.chords)] {
            for (name, keys) in overrides {
                let action = Action::named(name).ok_or_else(|| format!("unknown action {name:?}"))?;
                let keys = parse(keys)?;
                // Yours win: the action loses its old keys, the keys their old action.
                table.retain(|(k, a)| *a != action && !keys.contains(k));
                table.extend(keys.into_iter().map(|k| (k, action)));
            }
        }
        Ok(keymap)
    }

    pub fn is_prefix(&self, key: Key) -> bool {
        self.prefix.contains(&key)
    }

    pub fn after_prefix(&self, key: Key) -> Option<Action> {
        self.keys.iter().find(|(k, _)| *k == key).map(|&(_, a)| a)
    }

    pub fn chord(&self, key: Key) -> Option<Action> {
        self.chords.iter().find(|(k, _)| *k == key).map(|&(_, a)| a)
    }

    /// `⌃␣`: the first prefix key.
    pub fn prefix_label(&self) -> String {
        self.prefix.first().map(Key::to_string).unwrap_or_default()
    }

    /// The keys after the prefix for these actions, as one label (each
    /// action's first key).
    pub fn keys_label(&self, actions: &[Action]) -> String {
        label(&first_keys(&self.keys, actions))
    }

    pub fn chords_label(&self, actions: &[Action]) -> String {
        label(&first_keys(&self.chords, actions))
    }

    /// How to reach an action, for a hint: its chord if it has one, else
    /// the prefix and its key.
    pub fn hint(&self, actions: &[Action]) -> String {
        let chords = self.chords_label(actions);
        if !chords.is_empty() && first_keys(&self.chords, actions).len() == actions.len() {
            return chords;
        }
        let keys = self.keys_label(actions);
        if keys.is_empty() { String::new() } else { format!("{} {keys}", self.prefix_label()) }
    }

    /// The prefix panel and the help: each group's rows, as (keys, label);
    /// rows with no key left are left out.
    pub fn rows(&self, group: Group) -> Vec<(String, &'static str)> {
        ROWS.iter()
            .filter(|row| row.group == group)
            .map(|row| (self.keys_label(row.actions), row.label))
            .filter(|(keys, _)| !keys.is_empty())
            .collect()
    }

    /// The chords, as (keys, label).
    pub fn chord_rows(&self) -> Vec<(String, String)> {
        ROWS.iter()
            .map(|row| {
                let label = match row.group {
                    Group::Sessions => format!("{} session", row.label),
                    _ => row.label.to_string(),
                };
                (self.chords_label(row.actions), label)
            })
            .filter(|(keys, _)| !keys.is_empty())
            .collect()
    }
}

fn first_keys(table: &[(Key, Action)], actions: &[Action]) -> Vec<Key> {
    actions.iter().filter_map(|a| table.iter().find(|(_, b)| b == a).map(|&(k, _)| k)).collect()
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
        assert_eq!(key("ctrl-alt-left").to_string(), "⌃⌥←");
        // Shift is in the character, however the terminal reports it.
        assert_eq!(key("N"), Key::of(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::SHIFT)));
        assert_eq!(key("shift-n"), key("N"));
        assert_eq!(key("shift-tab").code, KeyCode::BackTab);
        assert!(Key::parse("ctrl-nope").is_none());
    }

    #[test]
    fn labels_runs_of_keys() {
        let keys = |ks: &[&str]| ks.iter().map(|k| key(k)).collect::<Vec<_>>();
        assert_eq!(label(&keys(&["left", "up", "right", "down"])), "←↑→↓");
        assert_eq!(label(&keys(&["ctrl-alt-left", "ctrl-alt-right"])), "⌃⌥←→");
        assert_eq!(label(&keys(&["alt-1", "alt-2", "alt-3"])), "⌥1-3");
        assert_eq!(label(&keys(&["n", "p"])), "n p");
    }

    #[test]
    fn every_key_does_one_thing() {
        let keymap = Keymap::default();
        for table in [&keymap.keys, &keymap.chords] {
            for (i, (k, _)) in table.iter().enumerate() {
                assert!(!table[i + 1..].iter().any(|(other, _)| other == k), "{k} is bound twice");
            }
        }
        // Every action has a key after the prefix.
        for row in ROWS {
            assert!(!keymap.keys_label(row.actions).is_empty(), "{} has no key", row.label);
        }
        assert_eq!(keymap.keys_label(&TABS), "1-9");
        assert_eq!(keymap.hint(&TABS), "⌥1-9");
        assert_eq!(keymap.hint(&[Zoom]), "⌃␣ z");
    }

    #[test]
    fn keys_toml_overrides() {
        let keymap = Keymap::from_toml(
            r#"
            prefix = "ctrl-a"
            legacy_ctrl_keys = false
            [after_prefix]
            zoom = ["m", "n"]
            [chords]
            tab_1 = []
            "#,
        )
        .unwrap();
        assert_eq!(keymap.prefix, vec![key("ctrl-a")]);
        assert!(!keymap.legacy);
        assert_eq!(keymap.after_prefix(key("m")), Some(Zoom));
        // n was the next tab's: now it zooms, and z does nothing.
        assert_eq!(keymap.after_prefix(key("n")), Some(Zoom));
        assert_eq!(keymap.after_prefix(key("z")), None);
        assert_eq!(keymap.chord(key("alt-1")), None);
        assert!(Keymap::from_toml("[after_prefix]\nfly = \"f\"").is_err());
        assert!(Keymap::from_toml("prefix = \"hyper-x\"").is_err());
    }
}
