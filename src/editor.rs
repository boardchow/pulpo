//! Editing a file inside Pulpo, drawn exactly like the viewer: the same
//! syntax definitions, theme and highlighter, so a file looks the same
//! whether you read it or change it. The keys are the common ones (micro,
//! VS Code): arrows, shift to select, ctrl-z / ctrl-y, ctrl-c / x / v, ctrl-s.
//!
//! Highlighting keeps the highlighter's state at the start of every line it
//! went through, so an edit only redoes the lines from there to the screen's
//! end.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use syntect::highlighting::{HighlightIterator, HighlightState, Highlighter};
use syntect::parsing::{ParseState, ScopeStack, SyntaxReference};

use crate::theme::{DIM, HOVER, TEXT_SELECTION};
use crate::viewer::{self, MAX_BYTES, MAX_HIGHLIGHTED_LINES};

/// Typing within this long of the last edit undoes together.
const UNDO_GROUP: Duration = Duration::from_secs(1);
/// Tabs show as this many columns, as in the viewer.
const TAB: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Default)]
pub struct Pos {
    pub line: usize,
    /// In characters.
    pub col: usize,
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<String>,
    cursor: Pos,
}

/// What an edit was, to group typing for undo.
#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Insert,
    Delete,
    Other,
}

pub struct Editor {
    pub path: PathBuf,
    lines: Vec<String>,
    /// Kept as the file had them.
    crlf: bool,
    final_newline: bool,
    /// What tab inserts: a tab, or spaces.
    indent: String,
    pub cursor: Pos,
    /// The other end of the selection.
    anchor: Option<Pos>,
    /// The column up and down aim for, kept across short lines.
    goal: Option<usize>,
    pub scroll: usize,
    hscroll: usize,
    /// Rows and columns of the view, set when drawing.
    pub view: (usize, usize),
    pub dirty: bool,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_edit: Option<(Kind, Instant)>,
    /// Typing a search, and the text searched.
    pub find: Option<String>,
    pub find_typing: bool,
    highlight: Highlight,
    /// When the file was last read or written, to notice others changing it.
    modified: Option<std::time::SystemTime>,
}

impl Editor {
    /// Open `path`, with the cursor on `line` (from 1).
    pub fn open(path: PathBuf, line: Option<usize>) -> Result<Editor, String> {
        let bytes = std::fs::read(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        if bytes.len() > MAX_BYTES {
            return Err("this file is too large to edit here".into());
        }
        if bytes[..bytes.len().min(8000)].contains(&0) {
            return Err("this is a binary file".into());
        }
        let text = String::from_utf8(bytes).map_err(|_| "this file isn't UTF-8 text".to_string())?;
        let mut editor = Editor {
            highlight: Highlight::new(viewer::syntax_for(&path)),
            path,
            lines: Vec::new(),
            crlf: false,
            final_newline: true,
            indent: String::new(),
            cursor: Pos::default(),
            anchor: None,
            goal: None,
            scroll: 0,
            hscroll: 0,
            view: (24, 80),
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: None,
            find: None,
            find_typing: false,
            modified: None,
        };
        editor.set_text(&text);
        editor.modified = std::fs::metadata(&editor.path).and_then(|m| m.modified()).ok();
        let line = line.unwrap_or(1).saturating_sub(1).min(editor.lines.len() - 1);
        editor.cursor = Pos { line, col: 0 };
        editor.scroll = line.saturating_sub(5);
        Ok(editor)
    }

    fn set_text(&mut self, text: &str) {
        self.crlf = text.contains("\r\n");
        self.final_newline = text.is_empty() || text.ends_with('\n');
        self.lines = text.lines().map(String::from).collect();
        if self.lines.is_empty() || text.ends_with("\n\n") || (text.ends_with('\n') && text.lines().count() == 0) {
            self.lines.push(String::new());
        }
        self.indent = guess_indent(&self.lines);
        self.highlight.invalidate(0);
    }

    pub fn text(&self) -> String {
        let newline = if self.crlf { "\r\n" } else { "\n" };
        let mut text = self.lines.join(newline);
        if self.final_newline && !text.is_empty() {
            text.push_str(newline);
        }
        text
    }

    /// Save; if the file changed on disk since (an agent), only when asked
    /// twice.
    pub fn save(&mut self) -> Result<(), String> {
        let modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if modified != self.modified && !self.dirty {
            self.reload_if_changed();
            return Err(format!("{} changed on disk: reloaded", self.path.display()));
        }
        if modified != self.modified {
            self.modified = modified;
            return Err(format!("{} changed on disk since: ctrl-s again overwrites it", self.path.display()));
        }
        std::fs::write(&self.path, self.text()).map_err(|e| format!("cannot save {}: {e}", self.path.display()))?;
        self.dirty = false;
        self.modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        Ok(())
    }

    /// Someone else (an agent) changed the file: take their version, unless
    /// there are changes here. True if it was taken.
    pub fn reload_if_changed(&mut self) -> bool {
        let modified = std::fs::metadata(&self.path).and_then(|m| m.modified()).ok();
        if self.dirty || modified == self.modified {
            return false;
        }
        let Ok(text) = std::fs::read_to_string(&self.path) else { return false };
        self.modified = modified;
        self.set_text(&text);
        self.clamp();
        true
    }

    // Moving

    fn len(&self, line: usize) -> usize {
        self.lines[line].chars().count()
    }

    fn clamp(&mut self) {
        self.cursor.line = self.cursor.line.min(self.lines.len() - 1);
        self.cursor.col = self.cursor.col.min(self.len(self.cursor.line));
        self.anchor = self.anchor.filter(|a| a.line < self.lines.len());
    }

    /// Move the cursor; with `select`, the selection follows.
    fn go(&mut self, to: Pos, select: bool) {
        if select {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
        self.cursor = to;
        self.clamp();
        if self.anchor == Some(self.cursor) {
            self.anchor = None;
        }
    }

    pub fn left(&mut self, select: bool) {
        let Pos { line, col } = self.cursor;
        self.goal = None;
        if let Some((start, _)) = self.selection().filter(|_| !select) {
            return self.go(start, false);
        }
        let to = match (col, line) {
            (0, 0) => self.cursor,
            (0, _) => Pos { line: line - 1, col: self.len(line - 1) },
            _ => Pos { line, col: col - 1 },
        };
        self.go(to, select);
    }

    pub fn right(&mut self, select: bool) {
        let Pos { line, col } = self.cursor;
        self.goal = None;
        if let Some((_, end)) = self.selection().filter(|_| !select) {
            return self.go(end, false);
        }
        let to = match () {
            _ if col < self.len(line) => Pos { line, col: col + 1 },
            _ if line + 1 < self.lines.len() => Pos { line: line + 1, col: 0 },
            _ => self.cursor,
        };
        self.go(to, select);
    }

    /// Up (negative) or down by `rows`, keeping the column aimed for.
    pub fn vertical(&mut self, rows: isize, select: bool) {
        let goal = *self.goal.get_or_insert(self.display_col(self.cursor));
        let line = self.cursor.line.saturating_add_signed(rows).min(self.lines.len() - 1);
        let col = self.col_at(line, goal);
        self.go(Pos { line, col }, select);
        self.goal = Some(goal);
    }

    /// Home: to the first non-blank character, then to the very start.
    pub fn home(&mut self, select: bool) {
        let line = self.cursor.line;
        let first = self.lines[line].chars().take_while(|c| c.is_whitespace()).count();
        let col = if self.cursor.col == first { 0 } else { first };
        self.goal = None;
        self.go(Pos { line, col }, select);
    }

    pub fn end(&mut self, select: bool) {
        self.goal = None;
        self.go(Pos { line: self.cursor.line, col: self.len(self.cursor.line) }, select);
    }

    pub fn page(&mut self, pages: isize, select: bool) {
        let rows = pages * self.view.0.saturating_sub(2).max(1) as isize;
        self.scroll = self.scroll.saturating_add_signed(rows).min(self.lines.len().saturating_sub(1));
        self.vertical(rows, select);
    }

    /// To the start of the previous word, or past the end of the next one.
    pub fn word(&mut self, forward: bool, select: bool) {
        self.goal = None;
        let to = self.word_from(self.cursor, forward);
        self.go(to, select);
    }

    fn word_from(&self, from: Pos, forward: bool) -> Pos {
        let chars: Vec<char> = self.lines[from.line].chars().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut col = from.col;
        if forward {
            if col == chars.len() {
                return if from.line + 1 < self.lines.len() { Pos { line: from.line + 1, col: 0 } } else { from };
            }
            while col < chars.len() && !word(chars[col]) {
                col += 1;
            }
            while col < chars.len() && word(chars[col]) {
                col += 1;
            }
        } else {
            if col == 0 {
                return if from.line > 0 { Pos { line: from.line - 1, col: self.len(from.line - 1) } } else { from };
            }
            while col > 0 && !word(chars[col - 1]) {
                col -= 1;
            }
            while col > 0 && word(chars[col - 1]) {
                col -= 1;
            }
        }
        Pos { line: from.line, col }
    }

    /// The start of the file, or its end.
    pub fn edge(&mut self, end: bool, select: bool) {
        let line = if end { self.lines.len() - 1 } else { 0 };
        let col = if end { self.len(line) } else { 0 };
        self.goal = None;
        self.go(Pos { line, col }, select);
    }

    /// Put the cursor at `pos`, ending any selection.
    pub fn click_at(&mut self, pos: Pos) {
        self.go(pos, false);
    }

    pub fn select_all(&mut self) {
        let last = self.lines.len() - 1;
        self.anchor = Some(Pos::default());
        self.cursor = Pos { line: last, col: self.len(last) };
    }

    /// Put the cursor where a click landed: a row of the file and a column
    /// of the screen (both from 0, the column after the line numbers).
    pub fn click(&mut self, row: usize, column: usize, select: bool) {
        let line = (self.scroll + row).min(self.lines.len() - 1);
        let col = self.col_at(line, column + self.hscroll);
        self.goal = None;
        self.go(Pos { line, col }, select);
    }

    pub fn scroll_by(&mut self, rows: isize) {
        self.scroll = self.scroll.saturating_add_signed(rows).min(self.lines.len().saturating_sub(1));
    }

    /// The ordered ends of the selection, if any.
    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let anchor = self.anchor.filter(|a| *a != self.cursor)?;
        Some((anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        Some(self.slice(start, end))
    }

    fn slice(&self, start: Pos, end: Pos) -> String {
        let take = |line: usize, from: usize, to: Option<usize>| -> String {
            let chars = self.lines[line].chars().skip(from);
            match to {
                Some(to) => chars.take(to - from).collect(),
                None => chars.collect(),
            }
        };
        if start.line == end.line {
            return take(start.line, start.col, Some(end.col));
        }
        let mut out = take(start.line, start.col, None);
        for line in start.line + 1..end.line {
            out.push('\n');
            out.push_str(&self.lines[line]);
        }
        out.push('\n');
        out.push_str(&take(end.line, 0, Some(end.col)));
        out
    }

    // Changing

    /// Before an edit: keep what to undo to. Typing in a row undoes as one.
    fn checkpoint(&mut self, kind: Kind) {
        self.checkpoint_after(kind, false);
    }

    /// `new_group`: this edit starts a new undo step even while typing (a
    /// line break); what is typed next joins it.
    fn checkpoint_after(&mut self, kind: Kind, new_group: bool) {
        let grouped = !new_group
            && self.last_edit.is_some_and(|(last, at)| last == kind && kind != Kind::Other && at.elapsed() < UNDO_GROUP);
        if !grouped {
            self.undo.push(Snapshot { lines: self.lines.clone(), cursor: self.cursor });
            if self.undo.len() > 500 {
                self.undo.remove(0);
            }
        }
        self.redo.clear();
        self.last_edit = Some((kind, Instant::now()));
        self.dirty = true;
    }

    pub fn undo(&mut self) {
        self.step_history(true);
    }

    pub fn redo(&mut self) {
        self.step_history(false);
    }

    fn step_history(&mut self, back: bool) {
        let (from, to) = if back { (&mut self.undo, &mut self.redo) } else { (&mut self.redo, &mut self.undo) };
        let Some(snapshot) = from.pop() else { return };
        to.push(Snapshot { lines: std::mem::replace(&mut self.lines, snapshot.lines), cursor: self.cursor });
        self.cursor = snapshot.cursor;
        self.anchor = None;
        self.last_edit = None;
        self.dirty = true;
        self.highlight.invalidate(0);
        self.clamp();
    }

    /// Remove the selection's text; the cursor goes where it started.
    fn remove_selection(&mut self) -> bool {
        let Some((start, end)) = self.selection() else { return false };
        let tail: String = self.lines[end.line].chars().skip(end.col).collect();
        let head: String = self.lines[start.line].chars().take(start.col).collect();
        self.lines.splice(start.line..=end.line, [head + &tail]);
        self.cursor = start;
        self.anchor = None;
        self.highlight.invalidate(start.line);
        true
    }

    /// Type or paste `text` at the cursor, over the selection.
    pub fn insert(&mut self, text: &str) {
        let typed = text.chars().count() == 1 && !text.contains('\n');
        self.insert_as(text, if typed { Kind::Insert } else { Kind::Other }, false);
    }

    fn insert_as(&mut self, text: &str, kind: Kind, new_group: bool) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        self.checkpoint_after(kind, new_group);
        self.remove_selection();
        let Pos { line, col } = self.cursor;
        let current = &self.lines[line];
        let at = byte_at(current, col);
        let (head, tail) = (current[..at].to_string(), current[at..].to_string());
        let mut pieces: Vec<String> = text.split('\n').map(String::from).collect();
        let last = pieces.len() - 1;
        let end_col = if last == 0 { col + pieces[0].chars().count() } else { pieces[last].chars().count() };
        pieces[0] = head + &pieces[0];
        pieces[last].push_str(&tail);
        self.lines.splice(line..=line, pieces);
        self.cursor = Pos { line: line + last, col: end_col };
        self.goal = None;
        self.highlight.invalidate(line);
    }

    /// Enter: a new line, indented like this one.
    pub fn newline(&mut self) {
        let indent: String = self.lines[self.cursor.line].chars().take_while(|c| *c == ' ' || *c == '\t').collect();
        let indent = &indent[..byte_at(&indent, self.cursor.col.min(indent.chars().count()))];
        self.insert_as(&format!("\n{indent}"), Kind::Insert, true);
    }

    pub fn backspace(&mut self, word: bool) {
        self.checkpoint(Kind::Delete);
        if self.remove_selection() {
            return;
        }
        let from = if word { self.word_from(self.cursor, false) } else { self.before(self.cursor) };
        self.remove(from, self.cursor);
    }

    pub fn delete(&mut self, word: bool) {
        self.checkpoint(Kind::Delete);
        if self.remove_selection() {
            return;
        }
        let to = if word { self.word_from(self.cursor, true) } else { self.after(self.cursor) };
        self.remove(self.cursor, to);
    }

    fn before(&self, pos: Pos) -> Pos {
        match (pos.col, pos.line) {
            (0, 0) => pos,
            (0, line) => Pos { line: line - 1, col: self.len(line - 1) },
            (col, line) => Pos { line, col: col - 1 },
        }
    }

    fn after(&self, pos: Pos) -> Pos {
        if pos.col < self.len(pos.line) {
            Pos { line: pos.line, col: pos.col + 1 }
        } else if pos.line + 1 < self.lines.len() {
            Pos { line: pos.line + 1, col: 0 }
        } else {
            pos
        }
    }

    fn remove(&mut self, start: Pos, end: Pos) {
        if start == end {
            return;
        }
        self.anchor = Some(start);
        self.cursor = end;
        self.remove_selection();
    }

    /// Tab: indent the selected lines, or insert one level of indentation.
    pub fn tab(&mut self) {
        match self.selection() {
            Some((start, end)) if start.line != end.line => self.indent_lines(true),
            _ => self.insert(&self.indent.clone()),
        }
    }

    /// Shift-tab: one level less on the selected lines, or this one.
    pub fn untab(&mut self) {
        self.indent_lines(false);
    }

    fn indent_lines(&mut self, more: bool) {
        self.checkpoint(Kind::Other);
        let (start, end) = self.selection().unwrap_or((self.cursor, self.cursor));
        let unit = self.indent.clone();
        let width = unit.chars().count();
        for line in start.line..=end.line {
            let text = &mut self.lines[line];
            let change = if more {
                text.insert_str(0, &unit);
                width as isize
            } else {
                let remove = if text.starts_with('\t') { 1 } else { text.chars().take(width).take_while(|c| *c == ' ').count() };
                text.drain(..remove);
                -(remove as isize)
            };
            for pos in [&mut self.cursor].into_iter().chain(self.anchor.as_mut()) {
                if pos.line == line {
                    pos.col = pos.col.saturating_add_signed(change);
                }
            }
        }
        self.highlight.invalidate(start.line);
        self.clamp();
    }

    /// Ctrl-x: the selection, or the whole line, cut; what was cut.
    pub fn cut(&mut self) -> String {
        if let Some(text) = self.selected_text() {
            self.checkpoint(Kind::Other);
            self.remove_selection();
            return text;
        }
        self.checkpoint(Kind::Other);
        let line = self.cursor.line;
        let text = format!("{}\n", self.lines[line]);
        if self.lines.len() == 1 {
            self.lines[0].clear();
        } else {
            self.lines.remove(line);
        }
        self.cursor = Pos { line: line.min(self.lines.len() - 1), col: 0 };
        self.highlight.invalidate(line);
        text
    }

    /// Ctrl-c: the selection, or the whole line.
    pub fn copy(&self) -> String {
        self.selected_text().unwrap_or_else(|| format!("{}\n", self.lines[self.cursor.line]))
    }

    // Finding

    /// Go to the next place `query` appears from the cursor (or before it,
    /// backwards), selecting it. Case-insensitive unless it has capitals.
    pub fn find_next(&mut self, forward: bool) -> bool {
        let Some(query) = self.find.clone().filter(|q| !q.is_empty()) else { return false };
        let fold = |s: &str| if query.chars().any(char::is_uppercase) { s.to_string() } else { s.to_lowercase() };
        let needle = fold(&query);
        let count = self.lines.len();
        let from = if forward { self.selection().map_or(self.cursor, |(_, end)| end) } else { self.selection().map_or(self.cursor, |(start, _)| start) };
        for step in 0..=count {
            let line = if forward { (from.line + step) % count } else { (from.line + count * 2 - step) % count };
            let text: Vec<char> = fold(&self.lines[line]).chars().collect();
            let needle: Vec<char> = needle.chars().collect();
            let hits = (0..=text.len().saturating_sub(needle.len())).filter(|&i| text.len() >= needle.len() && text[i..i + needle.len()] == needle[..]);
            let hit = if forward {
                hits.into_iter().find(|&i| step > 0 || i >= from.col)
            } else {
                hits.into_iter().rfind(|&i| step > 0 || i + needle.len() <= from.col)
            };
            if let Some(col) = hit {
                self.anchor = Some(Pos { line, col });
                self.cursor = Pos { line, col: col + needle.len() };
                self.goal = None;
                return true;
            }
        }
        false
    }

    // Drawing

    /// Columns on screen up to `pos`: tabs are as wide as in the viewer.
    fn display_col(&self, pos: Pos) -> usize {
        self.lines[pos.line].chars().take(pos.col).map(|c| if c == '\t' { TAB } else { 1 }).sum()
    }

    /// The character at screen column `column` of `line`.
    fn col_at(&self, line: usize, column: usize) -> usize {
        let mut at = 0;
        for (i, c) in self.lines[line].chars().enumerate() {
            let width = if c == '\t' { TAB } else { 1 };
            if at + width > column {
                return i;
            }
            at += width;
        }
        self.len(line)
    }

    /// Width of the line numbers and the space after them.
    pub fn gutter(&self) -> usize {
        self.lines.len().to_string().len() + 2
    }

    /// Scroll so the cursor is in view.
    pub fn follow_cursor(&mut self) {
        let (height, width) = (self.view.0.max(1), self.view.1);
        if self.cursor.line < self.scroll {
            self.scroll = self.cursor.line;
        } else if self.cursor.line >= self.scroll + height {
            self.scroll = self.cursor.line + 1 - height;
        }
        let text_width = width.saturating_sub(self.gutter()).max(1);
        let col = self.display_col(self.cursor);
        if col < self.hscroll {
            self.hscroll = col;
        } else if col >= self.hscroll + text_width {
            self.hscroll = col + 1 - text_width;
        }
    }

    /// The rows to draw from `scroll`, as the viewer draws them, with the
    /// selection, the search and the cursor's line marked.
    pub fn rows(&mut self, height: usize) -> Vec<Line<'static>> {
        let gutter = self.gutter();
        let width = gutter - 2;
        let end = (self.scroll + height).min(self.lines.len());
        let selection = self.selection();
        (self.scroll..end)
            .map(|i| {
                let mut spans = vec![Span::from(format!("{:>width$}  ", i + 1)).fg(DIM)];
                spans.extend(self.highlight.spans(i, &self.lines));
                let mut line = Line::from(spans);
                if let Some(query) = self.find.as_deref().filter(|q| !q.is_empty()) {
                    let ignore_case = !query.chars().any(char::is_uppercase);
                    line = viewer::highlight_matches(line, query, ignore_case, gutter);
                }
                if selection.is_none() && i == self.cursor.line {
                    line = line.patch_style(Style::default().bg(HOVER));
                }
                if let Some((start, end)) = selection.filter(|(s, e)| (s.line..=e.line).contains(&i)) {
                    let from = if i == start.line { self.display_col(start) } else { 0 };
                    let to = if i == end.line { self.display_col(end) } else { usize::MAX };
                    // A selected line break shows as one more selected cell.
                    let to = if i == end.line { to } else { self.display_col(Pos { line: i, col: self.len(i) }) + 1 };
                    line = mark(line, gutter + from, gutter + to);
                }
                viewer::shift(line, gutter, self.hscroll)
            })
            .collect()
    }

    /// Where the cursor is drawn, from the view's top left.
    pub fn cursor_cell(&self) -> Option<(usize, usize)> {
        let row = self.cursor.line.checked_sub(self.scroll)?;
        let col = self.display_col(self.cursor).checked_sub(self.hscroll)?;
        Some((row, self.gutter() + col))
    }
}

/// The selection's background over columns `from..to` of a drawn line.
fn mark(line: Line<'static>, from: usize, to: usize) -> Line<'static> {
    let selected = Style::default().bg(TEXT_SELECTION);
    let Line { spans: old, style, alignment } = line;
    let mut spans = Vec::new();
    let mut at = 0;
    for span in old {
        let chars: Vec<char> = span.content.chars().collect();
        let (start, end) = (at, at + chars.len());
        let (a, b) = (from.clamp(start, end) - start, to.clamp(start, end) - start);
        for (range, picked) in [(0..a, false), (a..b, true), (b..chars.len(), false)] {
            if !range.is_empty() {
                let text: String = chars[range].iter().collect();
                spans.push(Span::styled(text, if picked { span.style.patch(selected) } else { span.style }));
            }
        }
        at = end;
    }
    // A selected line break past the end of the text.
    if to > at && from <= at {
        spans.push(Span::styled(" ", selected));
    }
    Line { spans, style, alignment }
}

fn byte_at(text: &str, col: usize) -> usize {
    text.char_indices().nth(col).map_or(text.len(), |(i, _)| i)
}

/// What one level of indentation is in this file: a tab if lines start with
/// tabs, else the smallest step of spaces (2 or 4).
fn guess_indent(lines: &[String]) -> String {
    let tabs = lines.iter().filter(|l| l.starts_with('\t')).count();
    let spaced: Vec<usize> = lines.iter().map(|l| l.chars().take_while(|c| *c == ' ').count()).filter(|&n| n > 0).collect();
    if tabs > spaced.len() {
        return "\t".into();
    }
    let two = spaced.iter().filter(|&&n| n % 4 == 2).count();
    " ".repeat(if two * 5 > spaced.len() { 2 } else { 4 })
}

/// Highlighting as the viewer does it (`viewer::assets`), line by line,
/// keeping the state before each line so an edit redoes only what follows.
struct Highlight {
    syntax: &'static SyntaxReference,
    /// The state before line i, for i up to what was highlighted.
    states: Vec<(ParseState, HighlightState)>,
    spans: Vec<Vec<Span<'static>>>,
}

impl Highlight {
    fn new(syntax: &'static SyntaxReference) -> Highlight {
        Highlight { syntax, states: Vec::new(), spans: Vec::new() }
    }

    /// Line `from` changed: it and everything after it is redone.
    fn invalidate(&mut self, from: usize) {
        self.states.truncate(from + 1);
        self.spans.truncate(from);
    }

    fn spans(&mut self, i: usize, lines: &[String]) -> Vec<Span<'static>> {
        let (syntaxes, theme) = viewer::assets();
        let highlighter = Highlighter::new(theme);
        if self.states.is_empty() {
            self.states.push((ParseState::new(self.syntax), HighlightState::new(&highlighter, ScopeStack::new())));
        }
        while self.spans.len() <= i {
            let n = self.spans.len();
            let line = format!("{}\n", lines[n]);
            let (mut parse, mut state) = self.states[n].clone();
            let ranges = if n < MAX_HIGHLIGHTED_LINES {
                parse.parse_line(&line, syntaxes).ok().map(|ops| HighlightIterator::new(&mut state, &ops, &line, &highlighter).collect::<Vec<_>>())
            } else {
                None
            };
            self.spans.push(viewer::styled(ranges, &line));
            self.states.truncate(n + 1);
            self.states.push((parse, state));
        }
        self.spans[i].clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str) -> Editor {
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("pulpo-editor-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.rs");
        std::fs::write(&path, text).unwrap();
        let editor = Editor::open(path, None).unwrap();
        // It holds the text; the tests don't save.
        std::fs::remove_dir_all(&dir).unwrap();
        editor
    }

    #[test]
    fn asks_before_overwriting_an_agents_change() {
        let dir = std::env::temp_dir().join(format!("pulpo-editor-save-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("f.txt");
        std::fs::write(&path, "mine\n").unwrap();
        let mut e = Editor::open(path.clone(), None).unwrap();
        e.insert("x");
        // An agent writes it a moment later.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(&path, "theirs\n").unwrap();
        assert!(e.save().is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "theirs\n");
        assert!(e.save().is_ok());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "xmine\n");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn types_deletes_and_undoes() {
        let mut e = editor("fn main() {\n    x\n}\n");
        e.vertical(1, false);
        e.end(false);
        e.insert("y");
        e.insert("z");
        e.newline();
        e.insert("w");
        assert_eq!(e.text(), "fn main() {\n    xyz\n    w\n}\n");
        // Typing undoes in one step, the line break in another.
        e.undo();
        assert_eq!(e.text(), "fn main() {\n    xyz\n}\n");
        e.undo();
        assert_eq!(e.text(), "fn main() {\n    x\n}\n");
        e.redo();
        assert_eq!(e.text(), "fn main() {\n    xyz\n}\n");
        e.backspace(true);
        assert_eq!(e.text(), "fn main() {\n    \n}\n");
        assert!(e.dirty);
    }

    #[test]
    fn selects_cuts_and_pastes_across_lines() {
        let mut e = editor("one\ntwo\nthree\n");
        e.right(false);
        e.vertical(1, true);
        assert_eq!(e.selected_text().as_deref(), Some("ne\nt"));
        let cut = e.cut();
        assert_eq!((cut.as_str(), e.text().as_str()), ("ne\nt", "owo\nthree\n"));
        e.end(false);
        e.insert(&cut);
        assert_eq!(e.text(), "owone\nt\nthree\n");
        // Without a selection, ctrl-x takes the line.
        assert_eq!(e.cut(), "t\n");
        assert_eq!(e.text(), "owone\nthree\n");
    }

    #[test]
    fn indents_like_the_file() {
        let mut e = editor("a\n  b\n    c\n");
        e.vertical(1, false);
        e.end(false);
        e.newline();
        e.tab();
        assert_eq!(e.text(), "a\n  b\n    \n    c\n");
        e.select_all();
        e.untab();
        assert_eq!(e.text(), "a\nb\n  \n  c\n");
        assert_eq!(guess_indent(&["\tx".into(), "\ty".into()]), "\t");
    }

    #[test]
    fn keeps_line_endings_and_finds() {
        let mut e = editor("a\r\nFoo b\r\nfoo\r\n");
        e.find = Some("foo".into());
        assert!(e.find_next(true));
        assert_eq!((e.cursor.line, e.selected_text().as_deref()), (1, Some("Foo")));
        assert!(e.find_next(true));
        assert_eq!(e.cursor.line, 2);
        assert_eq!(e.text(), "a\r\nFoo b\r\nfoo\r\n");
    }

    #[test]
    fn highlights_like_the_viewer() {
        let mut e = editor("let x = \"hi\";\n");
        let spans = e.highlight.spans(0, &e.lines);
        // The same pieces and colors the viewer gets for the same line.
        let (syntaxes, theme) = viewer::assets();
        let mut viewer = syntect::easy::HighlightLines::new(viewer::syntax_for(&e.path), theme);
        let expected = viewer::styled(viewer.highlight_line("let x = \"hi\";\n", syntaxes).ok(), "let x = \"hi\";\n");
        assert_eq!(spans, expected);
        assert!(spans.len() > 3);
        // An edit redoes it, still the same.
        e.insert("// ");
        assert_ne!(e.highlight.spans(0, &e.lines), expected);
    }
}
