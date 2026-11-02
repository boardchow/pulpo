//! The pickers: every agent and terminal across workspaces (go to, Herdr's
//! goto), and every action by name (the menu, Omarchy's). Each typed word
//! must appear somewhere in an entry; tab narrows go to by state.

use ratatui::widgets::ListState;

use crate::agents::State;
use crate::keymap::Action;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Goto,
    Menu,
    Theme,
}

/// Which terminals go to shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Show {
    All,
    ToRead,
    Working,
    Quiet,
}

impl Show {
    pub const ALL: [Show; 4] = [Show::All, Show::ToRead, Show::Working, Show::Quiet];

    pub fn label(self) -> &'static str {
        match self {
            Show::All => "all",
            Show::ToRead => "to read",
            Show::Working => "working",
            Show::Quiet => "quiet",
        }
    }

    fn admits(self, state: Option<State>) -> bool {
        match (self, state) {
            (Show::All, _) | (_, None) => true,
            (Show::ToRead, Some(s)) => matches!(s, State::Waiting | State::Done),
            (Show::Working, Some(s)) => s == State::Running,
            (Show::Quiet, Some(s)) => s == State::Idle,
        }
    }

    fn step(self, delta: isize) -> Show {
        let at = Show::ALL.iter().position(|&s| s == self).unwrap_or(0) as isize;
        Show::ALL[(at + delta).rem_euclid(Show::ALL.len() as isize) as usize]
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Target {
    Term(usize),
    Action(Action),
    /// A theme, by its index in the list the picker was made from.
    Theme(usize),
}

pub struct Entry {
    /// The heading it goes under: a repository, or a group of actions.
    pub group: String,
    /// A terminal's program, or an action's name.
    pub text: String,
    /// Where a terminal is (session and tab), or an action's key.
    pub place: String,
    /// What the agent asks, does or finished.
    pub detail: String,
    /// How long ago that was.
    pub age: String,
    pub state: Option<State>,
    /// Shown under the list for the selected one.
    pub path: String,
    pub target: Target,
}

impl Entry {
    fn matches(&self, words: &[String]) -> bool {
        let text = [&self.group, &self.text, &self.place, &self.detail, &self.path].map(|s| s.to_lowercase()).join(" ");
        words.iter().all(|word| text.contains(word.as_str()))
    }
}

/// A line of the list: a heading, or an entry (by index).
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Row {
    Heading(String),
    Entry(usize),
}

pub struct Picker {
    pub kind: Kind,
    pub query: String,
    pub show: Show,
    pub entries: Vec<Entry>,
    pub rows: Vec<Row>,
    pub state: ListState,
}

impl Picker {
    /// On `selected` if it's there, else the first entry.
    pub fn new(kind: Kind, entries: Vec<Entry>, show: Show, selected: Option<Target>) -> Picker {
        let mut picker = Picker { kind, query: String::new(), show, entries, rows: Vec::new(), state: ListState::default() };
        picker.update();
        if let Some(row) = picker.rows.iter().position(|r| matches!(r, Row::Entry(i) if Some(picker.entries[*i].target) == selected)) {
            picker.state.select(Some(row));
        }
        picker
    }

    /// The rows for what is typed and shown, on the first entry.
    pub fn update(&mut self) {
        let words: Vec<String> = self.query.to_lowercase().split_whitespace().map(String::from).collect();
        self.rows.clear();
        for (i, entry) in self.entries.iter().enumerate() {
            if !self.show.admits(entry.state) || !entry.matches(&words) {
                continue;
            }
            if self.rows.iter().rev().find_map(|r| if let Row::Entry(j) = r { Some(&self.entries[*j].group) } else { None }) != Some(&entry.group) {
                self.rows.push(Row::Heading(entry.group.clone()));
            }
            self.rows.push(Row::Entry(i));
        }
        self.state.select(self.rows.iter().position(|r| matches!(r, Row::Entry(_))));
    }

    pub fn shown(&self) -> usize {
        self.rows.iter().filter(|r| matches!(r, Row::Entry(_))).count()
    }

    pub fn next_show(&mut self, delta: isize) {
        self.show = self.show.step(delta);
        self.update();
    }

    /// Move by `delta` entries, over the headings; it stops at the ends.
    pub fn step(&mut self, delta: isize) {
        let entries: Vec<usize> = (0..self.rows.len()).filter(|&r| matches!(self.rows[r], Row::Entry(_))).collect();
        let Some(at) = self.state.selected().and_then(|r| entries.iter().position(|&e| e == r)) else { return };
        let next = (at as isize + delta).clamp(0, entries.len() as isize - 1) as usize;
        self.state.select(Some(entries[next]));
    }

    pub fn selected(&self) -> Option<&Entry> {
        match self.rows.get(self.state.selected()?)? {
            Row::Entry(i) => self.entries.get(*i),
            Row::Heading(_) => None,
        }
    }
}

/// `2m`, `3h`: how long ago, in the largest unit.
pub fn age(seconds: u64) -> String {
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m", seconds / 60),
        3600..86400 => format!("{}h", seconds / 3600),
        _ => format!("{}d", seconds / 86400),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(group: &str, text: &str, state: State, id: usize) -> Entry {
        Entry {
            group: group.into(),
            text: text.into(),
            place: "main · tab 1".into(),
            detail: String::new(),
            age: String::new(),
            state: Some(state),
            path: format!("~/code/{group}"),
            target: Target::Term(id),
        }
    }

    fn picker() -> Picker {
        let entries = vec![
            entry("backbone", "claude", State::Waiting, 1),
            entry("backbone", "zsh", State::Idle, 2),
            entry("pulpo", "claude", State::Running, 3),
        ];
        Picker::new(Kind::Goto, entries, Show::All, Some(Target::Term(3)))
    }

    #[test]
    fn groups_under_headings_and_starts_on_the_one_asked() {
        let picker = picker();
        assert_eq!(picker.rows, vec![Row::Heading("backbone".into()), Row::Entry(0), Row::Entry(1), Row::Heading("pulpo".into()), Row::Entry(2)]);
        assert_eq!(picker.selected().map(|e| e.target), Some(Target::Term(3)));
    }

    #[test]
    fn filters_by_words_and_state() {
        let mut picker = picker();
        picker.query = "claude pul".into();
        picker.update();
        assert_eq!(picker.shown(), 1);
        picker.query.clear();
        picker.next_show(1);
        assert_eq!(picker.show, Show::ToRead);
        assert_eq!(picker.selected().map(|e| e.target), Some(Target::Term(1)));
        assert_eq!(picker.shown(), 1);
        picker.next_show(-2);
        assert_eq!(picker.show, Show::Quiet);
    }

    #[test]
    fn steps_over_headings() {
        let mut picker = picker();
        picker.state.select(Some(1));
        picker.step(1);
        picker.step(1);
        assert_eq!(picker.selected().map(|e| e.target), Some(Target::Term(3)));
        picker.step(5);
        assert_eq!(picker.selected().map(|e| e.target), Some(Target::Term(3)));
        picker.step(-9);
        assert_eq!(picker.selected().map(|e| e.target), Some(Target::Term(1)));
    }

    #[test]
    fn says_how_long_ago() {
        assert_eq!(age(42), "42s");
        assert_eq!(age(150), "2m");
        assert_eq!(age(7300), "2h");
        assert_eq!(age(200_000), "2d");
    }
}
