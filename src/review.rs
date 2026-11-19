use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;

use ratatui::style::Color;
use ratatui::text::{Line, Span};
use ratatui::widgets::ListState;

use crate::git::{self, Commit, History};
use crate::theme;
use crate::tree::{self, Row};
use crate::viewer::{self, Kind, ReviewDiff};

pub struct File {
    pub path: PathBuf,
    /// git's status letter, or '?' for an untracked file.
    pub status: char,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Comment {
    /// Two can end up on the same lines, so a comment is edited by this, not by its place.
    pub id: u64,
    pub path: PathBuf,
    pub commit: Option<String>,
    /// Whether they were deleted, the first and last line; none for the whole file.
    pub lines: Option<(bool, usize, usize)>,
    pub text: String,
    /// What the lines said, to follow them.
    pub code: Vec<String>,
    pub outdated: bool,
}

impl Comment {
    /// Where it is, as an agent reads it: `src/app.rs:12-14`.
    pub fn at(&self, root: &Path) -> String {
        let mut at = self.path.strip_prefix(root).unwrap_or(&self.path).display().to_string();
        if let Some((deleted, first, last)) = self.lines {
            at += &if first == last { format!(":{first}") } else { format!(":{first}-{last}") };
            if deleted {
                at += " (deleted)";
            }
        }
        if let Some(commit) = &self.commit {
            at += &format!(" in commit {commit}");
        }
        at
    }

    fn same_place(&self, other: &Comment) -> bool {
        (&self.path, &self.commit, self.lines) == (&other.path, &other.commit, other.lines)
    }
}

#[derive(Clone, PartialEq, Debug)]
pub enum Shown {
    Row(usize),
    Wrap { row: usize, piece: usize },
    /// Its index in the comments; none for the one being written.
    Note(Option<usize>, String),
    /// Under the one being written.
    Hint,
    Fold(Range<usize>),
    Done(Range<usize>),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Hide {
    No,
    Fold,
    Approved,
    Commented,
}

#[derive(Clone, Debug)]
pub struct Hunk {
    old: Range<usize>,
    new: Range<usize>,
    id: u64,
}

pub struct Changes {
    pub root: PathBuf,
    /// What the files are compared with: HEAD, or the parent of the commit shown.
    against: String,
    /// Shown instead of what isn't committed.
    pub commit: Option<Commit>,
    /// The commit a workspace Pulpo made was copied at, where its history starts; otherwise each read finds it again.
    copied: Option<String>,
    /// Where the history starts, for its title.
    pub since: String,
    loaded: bool,
    /// Bumped by each read asked for, so an older one arriving late is dropped.
    generation: u64,
    pub files: Vec<File>,
    /// Each changed file's status, and each folder's first: thousands of files are looked up per frame.
    statuses: HashMap<PathBuf, char>,
    pub rows: Vec<Row>,
    folded: HashSet<PathBuf>,
    pub state: ListState,
    pub diff: Option<(PathBuf, ReviewDiff)>,
    pub scroll: usize,
    pub height: usize,
    pub code_width: usize,
    pub cursor: usize,
    pub anchor: Option<usize>,
    /// The workspace's comments, those on its commits too.
    pub comments: Vec<Comment>,
    pub shown: Vec<Shown>,
    /// The next draw brings the cursor, or the comment being written, into view.
    pub reveal: bool,
    /// Which comment above the file ⌃b e and d take next, so each press reaches another.
    turn: usize,
    pub history: History,
    pub history_state: ListState,
    /// How many commits History reads: a page more each time the list nears its end.
    pub history_limit: usize,
    hunks: HashMap<PathBuf, Vec<Hunk>>,
    counts: HashMap<PathBuf, (usize, usize)>,
    pub approved: HashSet<u64>,
    hidden: Vec<Hide>,
}

pub const HISTORY_PAGE: usize = 100;
const CONTEXT: usize = 3;
const SCOPE_REACH: usize = 10;

impl Changes {
    pub fn new(root: &Path, copied: Option<String>) -> Changes {
        Changes {
            root: root.to_path_buf(),
            against: "HEAD".into(),
            commit: None,
            since: if copied.is_some() { "since the copy".into() } else { String::new() },
            copied,
            loaded: false,
            generation: 0,
            files: Vec::new(),
            statuses: HashMap::new(),
            rows: Vec::new(),
            folded: HashSet::new(),
            state: ListState::default().with_selected(Some(0)),
            diff: None,
            scroll: 0,
            height: 20,
            code_width: 80,
            cursor: 0,
            anchor: None,
            comments: Vec::new(),
            shown: Vec::new(),
            reveal: false,
            turn: 0,
            history: History::default(),
            history_state: ListState::default().with_selected(Some(0)),
            history_limit: HISTORY_PAGE,
            hunks: HashMap::new(),
            counts: HashMap::new(),
            approved: HashSet::new(),
            hidden: Vec::new(),
        }
    }

    pub fn of_commit(root: &Path, commit: &Commit) -> Changes {
        let mut changes = Changes::new(root, None);
        // The first commit has no parent: it's compared with the empty tree.
        let parent = git(root, &["rev-parse", "--verify", "--quiet", &format!("{}^", commit.hash)]).unwrap_or_else(|| EMPTY_TREE.into());
        changes.against = format!("{parent}..{}", commit.hash);
        changes.commit = Some(commit.clone());
        changes
    }

    #[cfg(test)]
    pub fn open(root: &Path, copied: Option<String>) -> Changes {
        let mut changes = Changes::new(root, copied);
        changes.load();
        changes
    }

    #[cfg(test)]
    fn load(&mut self) {
        let read = self.request().read();
        self.apply(read);
    }

    pub fn request(&mut self) -> Request {
        self.generation += 1;
        Request {
            root: self.root.clone(),
            against: self.against.clone(),
            copied: self.copied.clone(),
            commit: self.commit.is_some(),
            shown: self.diff.as_ref().map(|(p, _)| p.clone()),
            history_limit: self.history_limit,
            generation: self.generation,
        }
    }

    pub fn apply(&mut self, read: Read) {
        if read.root != self.root || read.against != self.against || read.generation != self.generation {
            return;
        }
        let first = !self.loaded || self.files.is_empty();
        self.loaded = true;
        let mut selected = self.selected().map(|r| r.path.clone());
        if first {
            selected = read.diff.as_ref().map(|(p, _)| p.clone()).or(selected);
        }
        let (was, scroll) = (self.diff.take().map(|(p, _)| p), self.scroll);
        (self.since, self.files, self.history, self.hunks) = (read.since, read.files, read.history, read.hunks);
        self.statuses.clear();
        for f in &self.files {
            for at in self.root.join(&f.path).ancestors().take_while(|a| a.starts_with(&self.root)) {
                self.statuses.entry(at.to_path_buf()).or_insert(f.status);
            }
        }
        self.recount();
        self.layout(selected);
        if let Some((path, diff)) = read.diff {
            let same = was.as_ref() == Some(&path);
            let lines = diff.lines.len();
            self.diff = Some((path, diff));
            self.follow_shown();
            if same {
                self.scroll = scroll;
                self.cursor = self.cursor.min(lines.saturating_sub(1));
                self.refold();
            } else {
                self.start(true, true);
            }
            self.scroll_by(0);
        }
        let selected_file = self.selected().filter(|r| !r.is_dir).map(|r| r.path.clone());
        if self.diff.as_ref().map(|(p, _)| p) != selected_file.as_ref() {
            self.diff = None;
            self.follow();
        }
    }

    fn layout(&mut self, selected: Option<PathBuf>) {
        // A file committed while it has comments stays, so they can still be read and sent.
        let commented = self.comments.iter().filter(|c| self.in_view(c)).filter_map(|c| c.path.strip_prefix(&self.root).ok());
        let mut paths: Vec<&Path> = self.files.iter().map(|f| f.path.as_path()).chain(commented).collect();
        paths.sort();
        paths.dedup();
        let all = tree::of_paths(&self.root, paths.iter().map(|p| p.to_str().unwrap_or_default()));
        self.rows = all.into_iter().filter(|r| !r.path.ancestors().skip(1).any(|a| self.folded.contains(a))).collect();
        for row in &mut self.rows {
            row.expanded = row.is_dir && !self.folded.contains(&row.path);
        }
        let at = selected.and_then(|p| self.rows.iter().position(|r| r.path == p));
        let at = at.unwrap_or_else(|| self.state.selected().unwrap_or(0));
        self.state.select(Some(at.min(self.rows.len().saturating_sub(1))));
    }

    pub fn selected(&self) -> Option<&Row> {
        self.rows.get(self.state.selected()?)
    }

    pub fn follow(&mut self) {
        match self.selected().filter(|r| !r.is_dir).map(|r| r.path.clone()) {
            Some(path) => self.show(&path),
            None => self.diff = None,
        }
    }

    fn show(&mut self, path: &Path) {
        if self.diff.as_ref().is_some_and(|(p, _)| p == path) {
            return;
        }
        self.diff = Some((path.to_path_buf(), self.read_diff(path)));
        self.follow_shown();
        self.start(true, true);
    }

    fn read_diff(&self, path: &Path) -> ReviewDiff {
        let untracked = self.files.iter().any(|f| self.root.join(&f.path) == path && f.status == '?');
        viewer::review_diff(&self.root, &self.against, path, untracked)
    }

    fn start(&mut self, down: bool, unapproved: bool) {
        self.refold();
        let first = self.diff.as_ref().and_then(|(path, diff)| self.change(path, diff, down, None, unapproved).or_else(|| self.change(path, diff, down, None, false)));
        self.go_to(first.unwrap_or(0));
    }

    fn follow_shown(&mut self) {
        if let Some((path, diff)) = &self.diff {
            follow_code(&mut self.comments, path, self.commit.as_ref().map(|c| c.hash.as_str()), diff);
        }
    }

    pub fn follow_draft(&self, draft: &mut Comment) {
        if let Some((path, diff)) = &self.diff {
            follow_code(std::slice::from_mut(draft), path, self.commit.as_ref().map(|c| c.hash.as_str()), diff);
        }
    }

    pub fn follow_all(&mut self) {
        let mut paths: Vec<PathBuf> = self.comments.iter().filter(|c| c.commit.is_none() && c.lines.is_some()).map(|c| c.path.clone()).collect();
        paths.sort();
        paths.dedup();
        for path in paths {
            let diff = viewer::review_diff(&self.root, "HEAD", &path, !git::is_tracked(&self.root, &path));
            follow_code(&mut self.comments, &path, None, &diff);
        }
    }

    pub fn status_of(&self, path: &Path) -> Option<char> {
        self.statuses.get(path).copied()
    }

    pub fn step(&mut self, delta: isize) {
        let max = self.rows.len().saturating_sub(1);
        let next = self.state.selected().unwrap_or(0).saturating_add_signed(delta).min(max);
        self.state.select(Some(next));
        self.follow();
    }

    pub fn fold(&mut self, open: bool) {
        let Some(row) = self.selected() else { return };
        let (path, is_dir) = (row.path.clone(), row.is_dir);
        if is_dir && self.folded.contains(&path) == open {
            if open {
                self.folded.remove(&path);
            } else {
                self.folded.insert(path.clone());
            }
            self.layout(Some(path));
        } else if !open && let Some(parent) = path.parent().filter(|p| *p != self.root) {
            let parent = parent.to_path_buf();
            self.layout(Some(parent));
        }
        self.follow();
    }

    fn wrap_points(&self, line: usize) -> Vec<usize> {
        self.diff.as_ref().map_or_else(|| vec![0], |(_, diff)| wrap_points(&diff.lines[line], diff.gutter, self.code_width))
    }

    pub fn piece_start(&self, line: usize, piece: usize) -> Option<usize> {
        self.wrap_points(line).get(piece).copied()
    }

    fn screen_lines(&self, row: usize) -> usize {
        let lines = self.wrap_points(row).len();
        // shortcut: a row taller than the view shows its first screenful, scroll by screen lines if that bites.
        lines.min(self.height.max(1))
    }

    fn rows_len(&self) -> usize {
        self.diff.as_ref().map_or(0, |(_, d)| d.lines.len())
    }

    pub fn ruler(&self, cells: usize) -> Vec<(Option<Color>, bool)> {
        let Some((_, diff)) = &self.diff else { return Vec::new() };
        let rows = self.rows_len();
        (0..cells)
            .map(|cell| {
                let (start, end) = cell_lines(cell, cells, rows);
                let in_view = start < self.view_end() && end > self.scroll;
                let lines: Vec<usize> = (start..end.min(diff.kinds.len())).collect();
                let kinds: Vec<Kind> = lines.iter().map(|&i| diff.kinds[i]).collect();
                let color = match (kinds.contains(&Kind::Added), kinds.contains(&Kind::Removed)) {
                    _ if lines.iter().any(|&i| self.commented(i)) => Some(theme::yellow()),
                    (true, true) => Some(theme::blue()),
                    (true, false) => Some(theme::green()),
                    (false, true) => Some(theme::red()),
                    _ => None,
                };
                (color, in_view)
            })
            .collect()
    }

    fn view_end(&self) -> usize {
        let end = |s: &Shown| match s {
            Shown::Row(row) | Shown::Wrap { row, .. } => Some(row + 1),
            Shown::Fold(rows) | Shown::Done(rows) => Some(rows.end),
            _ => None,
        };
        self.shown.iter().rev().find_map(end).unwrap_or(self.scroll + self.height)
    }

    pub fn jump_to(&mut self, cell: usize, cells: usize) {
        let (start, _) = cell_lines(cell, cells, self.rows_len());
        self.scroll = start.saturating_sub(self.height / 3);
        self.scroll_by(0);
    }

    /// From the cursor's row, or the nearest one in view after scrolling it out.
    pub fn move_cursor(&mut self, rows: isize, extend: bool) {
        let in_view: Vec<usize> = self.shown.iter().filter_map(|s| if let Shown::Row(row) | Shown::Done(Range { start: row, .. }) = s { Some(*row) } else { None }).collect();
        let mut row = self.cursor;
        if let (Some(&first), Some(&last)) = (in_view.first(), in_view.last()) {
            row = row.clamp(first, last);
        }
        for _ in 0..rows.unsigned_abs().min(self.rows_len()) {
            match self.step_row(row, rows > 0) {
                Some(next) => row = next,
                None => break,
            }
        }
        self.select_row(row, extend);
    }

    pub fn click(&mut self, at: usize, extend: bool) {
        if let Some(Shown::Row(row) | Shown::Wrap { row, .. } | Shown::Done(Range { start: row, .. })) = self.shown.get(at) {
            self.select_row(*row, extend);
        }
    }

    fn select_row(&mut self, row: usize, extend: bool) {
        if self.diff.is_none() {
            return;
        }
        self.anchor = if extend { self.anchor.or(Some(self.cursor)) } else { None };
        self.cursor = row;
        let top = (1..self.height.max(1)).fold(row, |r, _| self.prev_unit(r));
        self.scroll = self.scroll.min(row).max(top);
        self.scroll_by(0);
        self.reveal = true;
    }

    pub fn selection(&self) -> (usize, usize) {
        let anchor = self.anchor.unwrap_or(self.cursor);
        (anchor.min(self.cursor), anchor.max(self.cursor))
    }

    /// The lines selected, or the file in the tree; a comment already there comes back to edit.
    pub fn commenting(&self, reading: bool) -> Option<Comment> {
        if reading && self.anchor.is_none() && let Some(comment) = self.comment_folded(self.cursor) {
            return Some(comment.clone());
        }
        let (path, lines) = match self.diff.as_ref().filter(|_| reading) {
            // A file with no lines to show, binary or too big, still takes a comment.
            Some((path, diff)) => (path.clone(), if diff.kinds.is_empty() { None } else { Some(self.lines_selected()?) }),
            None => (self.selected().filter(|r| !r.is_dir)?.path.clone(), None),
        };
        let code = self.diff.as_ref().zip(lines).map(|((_, diff), lines)| side(diff, lines.0).filter(|(n, _)| (lines.1..=lines.2).contains(n)).map(|(_, code)| code).collect());
        let commit = self.commit.as_ref().map(|c| c.hash.clone());
        let comment = Comment { id: next_id(), path, commit, lines, text: String::new(), code: code.unwrap_or_default(), outdated: false };
        Some(self.comments.iter().find(|c| !c.outdated && c.same_place(&comment)).cloned().unwrap_or(comment))
    }

    /// At the cursor; in the tree, or on a file with no lines shown, the next of those above the file.
    pub fn comment_at(&mut self, reading: bool) -> Option<Comment> {
        if reading && self.diff.as_ref().is_some_and(|(_, diff)| !diff.kinds.is_empty()) {
            let at = self.commenting(true)?;
            return self.comments.iter().find(|c| c.id == at.id).cloned();
        }
        let above = self.above();
        let comment = above.get(self.turn % above.len().max(1)).map(|c| (*c).clone());
        self.turn = self.turn.wrapping_add(1);
        comment
    }

    /// The shown file's own comments and those whose lines it doesn't show.
    pub fn above(&self) -> Vec<&Comment> {
        self.comments.iter().filter(|c| self.row_under(c) == Some(None)).collect()
    }

    fn lines_selected(&self) -> Option<(bool, usize, usize)> {
        let (path, diff) = self.diff.as_ref()?;
        let (first, last) = self.selection();
        let at = |i| number(diff, i);
        if self.anchor.is_none()
            && let Some(comment) = self.comments.iter().find(|c| self.here(c, path) && at(first).is_some_and(|line| covers(c, line)))
        {
            return comment.lines;
        }
        let numbers = |deleted: bool| (first..=last).filter_map(at).filter(|&(d, _)| d == deleted).map(|(_, n)| n).collect::<Vec<_>>();
        let (deleted, numbers) = match numbers(false) {
            new if new.is_empty() => (true, numbers(true)),
            new => (false, new),
        };
        Some((deleted, *numbers.first()?, *numbers.last()?))
    }

    fn here(&self, comment: &Comment, path: &Path) -> bool {
        comment.path == path && self.in_view(comment)
    }

    fn in_view(&self, comment: &Comment) -> bool {
        comment.commit.as_deref() == self.commit.as_ref().map(|c| c.hash.as_str())
    }

    /// Saves the text on the comment with its id, wherever it moved, or adds it; empty, deletes it.
    pub fn keep(&mut self, comment: Comment) {
        if comment.text.trim().is_empty() {
            return self.delete(&comment);
        }
        match self.comments.iter_mut().find(|c| c.id == comment.id) {
            Some(kept) => kept.text = comment.text,
            None => self.comments.push(comment),
        }
        self.anchor = None;
        self.relayout();
    }

    pub fn delete(&mut self, comment: &Comment) {
        self.comments.retain(|c| c.id != comment.id);
        self.relayout();
    }

    /// Deleting the last comment on a committed file takes the file out of the tree.
    fn relayout(&mut self) {
        self.layout(self.selected().map(|r| r.path.clone()));
        self.follow();
        self.refold();
    }

    pub fn commented(&self, line: usize) -> bool {
        let Some((path, diff)) = &self.diff else { return false };
        number(diff, line).is_some_and(|at| self.comments.iter().any(|c| self.here(c, path) && covers(c, at)))
    }

    pub fn count(&self, path: &Path) -> usize {
        self.comments.iter().filter(|c| c.path.starts_with(path) && self.in_view(c)).count()
    }

    pub fn review(&self) -> String {
        let one = |c: &Comment| match c.outdated {
            true => format!("{} (outdated, it was on:)\n{}\n{}", c.at(&self.root), c.code.iter().map(|l| format!("> {l}")).collect::<Vec<_>>().join("\n"), c.text.trim()),
            false => format!("{}\n{}", c.at(&self.root), c.text.trim()),
        };
        let mut comments: Vec<&Comment> = self.comments.iter().collect();
        comments.sort_by_key(|c| (c.commit.clone(), c.path.clone(), c.lines.map(|(_, first, _)| first)));
        let review = comments.into_iter().map(one).collect::<Vec<_>>().join("\n\n");
        // Paths and pasted text could end the paste early and type into the agent.
        review.chars().filter(|&c| matches!(c, '\n' | '\t') || !c.is_control()).collect()
    }

    /// The row a comment shows under: none above the file, for a file comment or one whose lines are gone.
    fn row_under(&self, comment: &Comment) -> Option<Option<usize>> {
        let (path, diff) = self.diff.as_ref()?;
        if !self.here(comment, path) {
            return None;
        }
        let last = (0..diff.lines.len()).rev().find(|&i| number(diff, i).is_some_and(|at| covers(comment, at)));
        Some(last)
    }

    /// The one being written shows `CURSOR` at its byte index.
    pub fn fit(&mut self, width: usize, writing: Option<(&Comment, usize)>) {
        self.refold_for(writing.map(|(w, _)| w));
        let Some((_, diff)) = &self.diff else { return self.shown.clear() };
        self.code_width = width.saturating_sub(diff.gutter).max(1);
        let (width, cursor) = (width.saturating_sub(diff.gutter + 2).max(10), self.cursor);
        let wrap = |text: &str, i: Option<usize>| -> Vec<Shown> { text.split('\n').flat_map(|line| wrap(line, width)).map(|line| Shown::Note(i, line)).collect() };
        let mut notes: Vec<(Option<usize>, Vec<Shown>)> = Vec::new();
        for (i, comment) in self.comments.iter().enumerate().filter(|(_, c)| writing.is_none_or(|(w, _)| w.id != c.id)) {
            if let Some(row) = self.row_under(comment) {
                let text = if comment.outdated { format!("outdated · {}", comment.text) } else { comment.text.clone() };
                notes.push((row, wrap(&text, Some(i))));
            }
        }
        let written = writing.and_then(|(w, at)| Some((self.row_under(w)?, w, at)));
        if let Some((row, w, at)) = written {
            let mut text = w.text.clone();
            text.insert(at.min(text.len()), CURSOR);
            let mut lines = wrap(&text, None);
            lines.push(Shown::Hint);
            notes.push((row, lines));
        }
        let target = written.map_or(cursor, |(row, _, _)| row.unwrap_or(0));
        let writing = written.is_some();
        let under = |row: usize| notes.iter().filter(|(r, _)| *r == Some(row)).map(|(_, lines)| lines.len()).sum::<usize>();
        let (mut top, mut used) = (self.rows_len(), 0);
        while top > 0 && used < self.height {
            top = self.prev_unit(top);
            used += if self.collapsed(top) { 1 } else { self.screen_lines(top) + under(top) };
        }
        self.scroll = self.scroll.min(if used > self.height { self.next_unit(top) } else { top }).min(self.rows_len().saturating_sub(1));
        self.scroll = self.fold_at(self.scroll).start;
        loop {
            self.shown = self.lay(&notes);
            let seen = self.shown.iter().any(|s| matches!(s, Shown::Row(r) if *r == target) || matches!(s, Shown::Done(rows) if rows.contains(&target))) || (target == 0 && self.scroll == 0);
            if !self.reveal || (seen && (!writing || self.shown.contains(&Shown::Hint))) {
                break;
            }
            match self.scroll.cmp(&target) {
                std::cmp::Ordering::Greater => self.scroll = target,
                std::cmp::Ordering::Less => self.scroll = self.next_unit(self.scroll),
                std::cmp::Ordering::Equal => break,
            }
        }
        self.reveal = false;
    }

    fn lay(&self, notes: &[(Option<usize>, Vec<Shown>)]) -> Vec<Shown> {
        let mut shown = Vec::new();
        let under = |row: Option<usize>, shown: &mut Vec<Shown>| {
            for (_, lines) in notes.iter().filter(|(r, _)| *r == row) {
                shown.extend(lines.iter().cloned());
            }
        };
        if self.scroll == 0 {
            under(None, &mut shown);
        }
        let mut row = self.scroll;
        while row < self.rows_len() && shown.len() < self.height {
            if self.collapsed(row) {
                let fold = self.fold_at(row);
                row = fold.end;
                shown.push(if self.hidden[fold.start] == Hide::Fold { Shown::Fold(fold) } else { Shown::Done(fold) });
                continue;
            }
            shown.push(Shown::Row(row));
            shown.extend((1..self.screen_lines(row)).map(|piece| Shown::Wrap { row, piece }));
            under(Some(row), &mut shown);
            row += 1;
        }
        shown.truncate(self.height);
        shown
    }

    /// Down, from the second to last line on screen; up, back as many lines.
    pub fn page(&mut self, down: bool) {
        if down {
            let mut last = self.scroll;
            let mut units = self.shown.iter().map(|s| {
                match s {
                    Shown::Row(row) | Shown::Wrap { row, .. } => last = *row,
                    Shown::Fold(rows) | Shown::Done(rows) => last = rows.start,
                    Shown::Note(..) | Shown::Hint => {}
                }
                last
            });
            let top = units.nth(self.shown.len().saturating_sub(2)).filter(|&r| r > self.scroll);
            self.scroll = top.unwrap_or_else(|| self.next_unit(self.scroll)).min(self.rows_len().saturating_sub(1));
            return;
        }
        let (mut row, mut used) = (self.scroll, 0);
        while row > 0 && used < self.height.saturating_sub(2) {
            row = self.prev_unit(row);
            used += if self.collapsed(row) { 1 } else { self.screen_lines(row) };
        }
        self.scroll = row;
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let mut row = self.fold_at(self.scroll.min(self.rows_len().saturating_sub(1))).start;
        for _ in 0..rows.unsigned_abs().min(self.rows_len()) {
            row = if rows < 0 { self.prev_unit(row) } else { Some(self.next_unit(row)).filter(|&r| r < self.rows_len()).unwrap_or(row) };
        }
        self.scroll = row;
    }

    fn refold(&mut self) {
        self.refold_for(None);
    }

    fn refold_for(&mut self, draft: Option<&Comment>) {
        let Some((path, diff)) = &self.diff else { return self.hidden.clear() };
        let hunks = self.hunks.get(path).map_or(&[][..], Vec::as_slice);
        let len = diff.kinds.len();
        let changed = |i: usize| matches!(diff.kinds[i], Kind::Added | Kind::Removed);
        let hunk_of: Vec<Option<&Hunk>> = (0..len).map(|i| hunk_at(diff, hunks, i)).collect();
        let drafted = |i: usize| draft.is_some_and(|d| self.here(d, path) && number(diff, i).is_some_and(|at| covers(d, at)));
        let noted: Vec<bool> = (0..len).map(|i| self.commented(i) || drafted(i)).collect();
        let drafted_on: HashSet<u64> = (0..len).filter(|&i| drafted(i)).filter_map(|i| hunk_of[i]).map(|h| h.id).collect();
        let commented = |h: &Hunk| self.comment_on(path, h).next().is_some();
        let shut = |i: usize| hunk_of[i].is_some_and(|h| !drafted_on.contains(&h.id) && (commented(h) || self.approved.contains(&h.id)));
        let kept: Vec<bool> = (0..len).map(|i| (changed(i) || noted[i]) && !shut(i)).collect();
        let mut near = vec![false; len];
        let mut last = None;
        for i in 0..len {
            last = if kept[i] { Some(i) } else { last };
            near[i] = last.is_some_and(|l| i - l <= CONTEXT);
        }
        last = None;
        for i in (0..len).rev() {
            last = if kept[i] { Some(i) } else { last };
            near[i] |= last.is_some_and(|l| l - i <= CONTEXT);
            if kept[i] && (i == 0 || !kept[i - 1])
                && let Some(&start) = scopes(diff, i, i.saturating_sub(SCOPE_REACH)).first()
            {
                near[start..i].fill(true);
            }
        }
        let mut hidden: Vec<Hide> = (0..len).map(|i| if near[i] || diff.kinds[i] == Kind::Gap { Hide::No } else { Hide::Fold }).collect();
        let mut spans: HashMap<u64, (usize, usize, Hide)> = HashMap::new();
        for i in (0..len).filter(|&i| changed(i) && shut(i)) {
            let Some(h) = hunk_of[i] else { continue };
            let hide = if commented(h) { Hide::Commented } else { Hide::Approved };
            spans.entry(h.id).and_modify(|span| span.1 = i).or_insert((i, i, hide));
        }
        for (first, last, hide) in spans.into_values() {
            hidden[first..=last].fill(hide);
        }
        self.hidden = hidden;
        if matches!(self.hidden.get(self.cursor), Some(Hide::Approved | Hide::Commented)) {
            self.cursor = self.fold_at(self.cursor).start;
        } else if self.collapsed(self.cursor)
            && let Some(row) = self.step_row(self.cursor, true).or_else(|| self.step_row(self.cursor, false))
        {
            (self.cursor, self.anchor) = (row, None);
        }
    }

    fn collapsed(&self, row: usize) -> bool {
        self.hidden.get(row).is_some_and(|h| *h != Hide::No)
    }

    fn fold_at(&self, row: usize) -> Range<usize> {
        let Some(&hide) = self.hidden.get(row).filter(|h| **h != Hide::No) else { return row..row + 1 };
        let start = (0..row).rev().find(|&r| self.hidden[r] != hide).map_or(0, |r| r + 1);
        start..(row..self.rows_len()).find(|&r| self.hidden.get(r) != Some(&hide)).unwrap_or(self.rows_len())
    }

    fn next_unit(&self, row: usize) -> usize {
        self.fold_at(row).end
    }

    fn prev_unit(&self, row: usize) -> usize {
        row.checked_sub(1).map_or(0, |r| self.fold_at(r).start)
    }

    fn step_row(&self, row: usize, down: bool) -> Option<usize> {
        let mut next = row;
        loop {
            next = if down { self.fold_at(next).end } else { self.fold_at(next.checked_sub(1)?).start };
            if next >= self.rows_len() {
                return None;
            }
            if self.hidden.get(next) != Some(&Hide::Fold) {
                return Some(next);
            }
        }
    }

    fn go_to(&mut self, line: usize) {
        (self.cursor, self.anchor, self.reveal) = (line, None, true);
        let mut top = line;
        while top > 0 && !self.collapsed(top - 1) && line - top < self.height / 2 {
            top -= 1;
        }
        self.scroll = self.prev_unit(top);
    }

    pub fn approve(&mut self) -> Option<String> {
        let (path, diff) = self.diff.as_ref()?;
        let Some(hunks) = self.hunks.get(path) else { return Some("this file's changes can't be approved".into()) };
        let Some(id) = hunk_at(diff, hunks, self.cursor).map(|h| h.id) else { return Some("no change here to approve".into()) };
        if self.approved.remove(&id) {
            self.save_approved(Some(id));
            self.recount();
            self.refold();
            return None;
        }
        self.approved.insert(id);
        self.save_approved(None);
        self.recount();
        self.next_change(true, true)
    }

    /// Merged with what another window saved for these changes; ids of changes gone are dropped.
    fn save_approved(&self, taken_back: Option<u64>) {
        use std::os::fd::AsRawFd;
        let key = match &self.commit {
            Some(commit) => format!("{} {}", self.root.display(), commit.hash),
            None => self.root.display().to_string(),
        };
        let dir = crate::agents::state_dir();
        let _ = std::fs::create_dir_all(&dir);
        let Ok(lock) = std::fs::File::create(dir.join("approved.lock")) else { return };
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return;
        }
        let mut all = read_approved();
        let saved = all.remove(&key).unwrap_or_default();
        let kept = |id: &u64| Some(*id) != taken_back && (self.approved.contains(id) || saved.contains(id));
        let ids: Vec<u64> = self.hunks.values().flatten().map(|h| h.id).filter(kept).collect();
        if !ids.is_empty() {
            all.insert(key, ids);
        }
        let path = approved_path();
        let tmp = path.with_extension("json.tmp");
        if serde_json::to_vec(&all).is_ok_and(|bytes| std::fs::write(&tmp, bytes).is_ok()) {
            let _ = std::fs::rename(&tmp, &path);
        }
    }

    fn change(&self, path: &Path, diff: &ReviewDiff, down: bool, from: Option<usize>, unapproved: bool) -> Option<usize> {
        let hunks = self.hunks.get(path).map_or(&[][..], Vec::as_slice);
        let mut seen = HashSet::new();
        let firsts = diff.changes.iter().copied().filter(|&line| match hunk_at(diff, hunks, line) {
            Some(h) => !(unapproved && self.done(path, h)) && seen.insert(h.id),
            None => true,
        });
        let firsts: Vec<usize> = firsts.collect();
        match (from, down) {
            (None, true) => firsts.first().copied(),
            (None, false) => firsts.last().copied(),
            (Some(at), true) => firsts.into_iter().find(|&l| l > at),
            (Some(at), false) => firsts.into_iter().rev().find(|&l| l < at),
        }
    }

    pub fn next_change(&mut self, down: bool, unapproved: bool) -> Option<String> {
        self.refold();
        let from = self.fold_at(self.cursor).start;
        if let Some((path, diff)) = &self.diff
            && let Some(line) = self.change(path, diff, down, Some(from), unapproved)
        {
            self.go_to(line);
            return None;
        }
        let files: Vec<PathBuf> = tree::of_paths(&self.root, self.files.iter().map(|f| f.path.to_str().unwrap_or_default())).into_iter().filter(|r| !r.is_dir).map(|r| r.path).collect();
        let at = self.diff.as_ref().and_then(|(shown, _)| files.iter().position(|f| f == shown));
        let order: Vec<&PathBuf> = match (down, at) {
            (true, Some(at)) => files[at + 1..].iter().collect(),
            (true, None) => files.iter().collect(),
            (false, Some(at)) => files[..at].iter().rev().collect(),
            (false, None) => files.iter().rev().collect(),
        };
        for path in order {
            let skip = match self.hunks.get(path) {
                Some(hunks) => hunks.is_empty() || (unapproved && hunks.iter().all(|h| self.done(path, h))),
                None => unapproved,
            };
            if skip {
                continue;
            }
            let diff = self.read_diff(path);
            if self.change(path, &diff, down, None, unapproved).is_none() {
                continue;
            }
            self.folded.retain(|f| !path.starts_with(f));
            self.diff = Some((path.clone(), diff));
            self.follow_shown();
            self.layout(Some(path.clone()));
            self.start(down, unapproved);
            return None;
        }
        let (approved, all) = self.progress(&self.root);
        Some(match unapproved {
            true if all == 0 => "no changes".into(),
            true if approved == all => {
                let unread = self.files.iter().filter(|f| !self.hunks.contains_key(&self.root.join(&f.path))).count();
                match unread {
                    0 => format!("all {all} changes approved"),
                    n => format!("all {all} changes approved; {n} file{} can't be", if n == 1 { "" } else { "s" }),
                }
            }
            true => format!("no change left to approve {}", if down { "below" } else { "above" }),
            false => format!("no change {}", if down { "below" } else { "above" }),
        })
    }

    pub fn first_unapproved(&mut self) -> Option<String> {
        let shown = self.diff.take();
        let message = self.next_change(true, true);
        if self.diff.is_none() {
            self.diff = shown;
            self.refold();
        }
        message
    }

    pub fn progress(&self, path: &Path) -> (usize, usize) {
        self.counts.get(path).copied().unwrap_or_default()
    }

    pub fn recount(&mut self) {
        self.counts.clear();
        for (path, hunks) in &self.hunks {
            let approved = hunks.iter().filter(|h| self.approved.contains(&h.id)).count();
            for at in path.ancestors().take_while(|a| a.starts_with(&self.root)) {
                let count = self.counts.entry(at.to_path_buf()).or_default();
                (count.0, count.1) = (count.0 + approved, count.1 + hunks.len());
            }
        }
    }

    pub fn fold_label(&self, rows: &Range<usize>) -> (String, Vec<String>) {
        let Some((_, diff)) = &self.diff else { return Default::default() };
        let n = rows.len();
        let scopes = if rows.end < diff.lines.len() { scopes(diff, rows.end, rows.start) } else { Vec::new() };
        let text = |i: usize| viewer::plain(&diff.lines[i], diff.gutter).trim().to_string();
        (format!("⋯ {n} line{}", if n == 1 { "" } else { "s" }), scopes.into_iter().map(text).collect())
    }

    pub fn section(&self) -> Range<usize> {
        let Some((path, diff)) = self.diff.as_ref().filter(|(_, diff)| !diff.kinds.is_empty()) else { return 0..0 };
        if self.collapsed(self.cursor) {
            return self.fold_at(self.cursor);
        }
        let start = (0..self.cursor).rev().find(|&r| self.collapsed(r)).map_or(0, |r| r + 1);
        let block = start..(self.cursor..self.rows_len()).find(|&r| self.collapsed(r)).unwrap_or(self.rows_len());
        // Two changes near enough share a block: only the cursor's is the section, with the lines up to the next one.
        let hunks = self.hunks.get(path).map_or(&[][..], Vec::as_slice);
        let Some(id) = hunk_at(diff, hunks, self.cursor).map(|h| h.id) else { return block };
        let other = |i: usize| hunk_at(diff, hunks, i).is_some_and(|h| h.id != id);
        let first = (block.start..self.cursor).rev().find(|&i| other(i)).map_or(block.start, |i| i + 1);
        first..(self.cursor..block.end).find(|&i| other(i)).unwrap_or(block.end)
    }

    pub fn done_label(&self, rows: &Range<usize>) -> (bool, String) {
        let Some((_, diff)) = &self.diff else { return Default::default() };
        let count = |kind: Kind, sign: char| Some(rows.clone().filter(|&i| diff.kinds[i] == kind).count()).filter(|&n| n > 0).map(|n| format!(" {sign}{n}"));
        let counts = format!("{}{}", count(Kind::Added, '+').unwrap_or_default(), count(Kind::Removed, '−').unwrap_or_default());
        match self.comment_folded(rows.start) {
            Some(comment) => {
                let more = match self.folded_comments(rows.start) {
                    n if n > 1 => format!(" ({n} comments)"),
                    _ => String::new(),
                };
                (true, format!("✎{counts} · {}{more}", comment.text.lines().next().unwrap_or_default()))
            }
            None => (false, format!("✓{counts} · {}", viewer::plain(&diff.lines[rows.start], diff.gutter).trim())),
        }
    }

    pub fn left(&self) -> usize {
        self.hunks.iter().map(|(path, hunks)| hunks.iter().filter(|h| !self.done(path, h)).count()).sum()
    }

    pub fn on_commented(&self) -> bool {
        self.hidden.get(self.cursor) == Some(&Hide::Commented)
    }

    fn done(&self, path: &Path, hunk: &Hunk) -> bool {
        self.approved.contains(&hunk.id) || self.comment_on(path, hunk).next().is_some()
    }

    fn comment_on<'a>(&'a self, path: &'a Path, hunk: &'a Hunk) -> impl Iterator<Item = &'a Comment> {
        let on = |(deleted, first, last): (bool, usize, usize)| {
            let side = if deleted { &hunk.old } else { &hunk.new };
            first < side.end && last >= side.start
        };
        self.comments.iter().filter(move |c| !c.outdated && self.here(c, path) && c.lines.is_some_and(on))
    }

    fn comment_folded(&self, row: usize) -> Option<&Comment> {
        let (path, diff) = self.diff.as_ref().filter(|_| self.hidden.get(row) == Some(&Hide::Commented))?;
        self.comment_on(path, hunk_at(diff, self.hunks.get(path)?, row)?).next()
    }

    fn folded_comments(&self, row: usize) -> usize {
        let Some((path, diff)) = &self.diff else { return 0 };
        self.hunks.get(path).and_then(|hunks| hunk_at(diff, hunks, row)).map_or(0, |h| self.comment_on(path, h).count())
    }
}

pub struct Request {
    root: PathBuf,
    against: String,
    copied: Option<String>,
    commit: bool,
    shown: Option<PathBuf>,
    history_limit: usize,
    generation: u64,
}

pub struct Read {
    root: PathBuf,
    against: String,
    since: String,
    files: Vec<File>,
    diff: Option<(PathBuf, ReviewDiff)>,
    history: History,
    hunks: HashMap<PathBuf, Vec<Hunk>>,
    generation: u64,
}

impl Request {
    pub fn read(self) -> Read {
        let files: Vec<File> = changed(&self.root, &self.against, !self.commit).into_iter().map(|(path, status)| File { path, status }).collect();
        let shown = self.shown.or_else(|| {
            let rows = tree::of_paths(&self.root, files.iter().map(|f| f.path.to_str().unwrap_or_default()));
            rows.into_iter().find(|r| !r.is_dir).map(|r| r.path)
        });
        let diff = shown.map(|path| {
            let untracked = files.iter().any(|f| self.root.join(&f.path) == path && f.status == '?');
            let diff = viewer::review_diff(&self.root, &self.against, &path, untracked);
            (path, diff)
        });
        let (history, since) = if self.commit {
            (History::default(), String::new())
        } else {
            let (base, since) = self.copied.map(|c| (c, "since the copy".to_string())).unwrap_or_else(|| self::base(&self.root));
            (git::history(&self.root, &base, self.history_limit), since)
        };
        let hunks = hunks(&self.root, &self.against, &files);
        Read { root: self.root, against: self.against, since, files, diff, history, hunks, generation: self.generation }
    }
}

/// A line's number in its file, and whether it was deleted.
fn number(diff: &ReviewDiff, line: usize) -> Option<(bool, usize)> {
    match diff.kinds.get(line)? {
        Kind::Removed => Some((true, diff.old_numbers[line])),
        Kind::Same | Kind::Added => Some((false, diff.new_numbers[line])),
        Kind::Gap => None,
    }
}

/// Marks the cursor in the comment being written: a private-use character, so no text has it.
pub const CURSOR: char = '\u{E000}';

fn next_id() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

fn wrap(line: &str, width: usize) -> Vec<String> {
    let mut pieces = vec![String::new()];
    let mut used = 0;
    for c in line.chars() {
        let cells = Span::raw(c.to_string()).width();
        if used + cells > width && used > 0 {
            pieces.push(String::new());
            used = 0;
        }
        if let Some(piece) = pieces.last_mut() {
            piece.push(c);
        }
        used += cells;
    }
    pieces
}

fn side(diff: &ReviewDiff, deleted: bool) -> impl Iterator<Item = (usize, String)> + '_ {
    (0..diff.lines.len()).filter_map(move |i| {
        let (d, n) = number(diff, i)?;
        (d == deleted).then(|| (n, viewer::plain(&diff.lines[i], diff.gutter).trim_end().to_string()))
    })
}

type Side = Option<Vec<(usize, String)>>;

/// The new side's lines and the deleted ones, by number; none where they can't be read.
fn sides(diff: &ReviewDiff, file: Option<&Path>) -> [Side; 2] {
    if !diff.kinds.is_empty() {
        return [Some(side(diff, false).collect()), Some(side(diff, true).collect())];
    }
    // No change left to read them in, as when the agent took it back: the file is the new side.
    let small = |f: &&Path| std::fs::metadata(f).map_or(true, |m| m.len() <= viewer::MAX_BYTES as u64);
    let lines = match file.filter(small).map(std::fs::read) {
        Some(Ok(bytes)) => Some(String::from_utf8_lossy(&bytes).lines().enumerate().map(|(i, line)| (i + 1, viewer::clean(line).trim_end().to_string())).collect()),
        Some(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        _ => None,
    };
    [lines, None]
}

/// Keeps each comment on its code: where it is, or where it went if it's there once; outdated otherwise.
fn follow_code(comments: &mut [Comment], path: &Path, commit: Option<&str>, diff: &ReviewDiff) {
    let mut both: Option<[Side; 2]> = None;
    for comment in comments.iter_mut().filter(|c| c.path == path && c.commit.as_deref() == commit && !c.code.is_empty()) {
        let Some((deleted, first, _)) = comment.lines else { continue };
        let both = both.get_or_insert_with(|| sides(diff, commit.is_none().then_some(path)));
        let Some(side) = &both[usize::from(deleted)] else { continue };
        let n = comment.code.len();
        let matches = |p: &usize| side.get(*p..*p + n).is_some_and(|w| w.iter().map(|(_, code)| code).eq(&comment.code));
        let here = side.iter().position(|(number, _)| *number == first).filter(matches);
        let found = here.or_else(|| match (0..side.len()).filter(matches).take(2).collect::<Vec<_>>()[..] {
            [only] => Some(only),
            _ => None,
        });
        comment.outdated = found.is_none();
        if let Some(p) = found {
            comment.lines = Some((deleted, side[p].0, side[p + n - 1].0));
        }
    }
}

fn covers(comment: &Comment, (deleted, line): (bool, usize)) -> bool {
    !comment.outdated && comment.lines.is_some_and(|(d, first, last)| d == deleted && (first..=last).contains(&line))
}

/// Character offsets after the gutter.
fn wrap_points(line: &Line, gutter: usize, width: usize) -> Vec<usize> {
    let (mut points, mut used, mut kept) = (vec![0], 0, 1);
    for (i, c) in line.spans.iter().flat_map(|s| s.content.chars()).skip(gutter).enumerate() {
        let cells = if c.is_ascii() { 1 } else { Span::raw(c.to_string()).width() };
        if used + cells > width && used > 0 {
            points.push(i);
            used = 0;
        }
        used += cells;
        if !c.is_whitespace() {
            kept = points.len();
        }
    }
    points.truncate(kept);
    points
}

fn approved_path() -> PathBuf {
    crate::agents::state_dir().join("approved.json")
}

fn read_approved() -> HashMap<String, Vec<u64>> {
    std::fs::read(approved_path()).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
}

pub fn load_approved() -> HashSet<u64> {
    read_approved().into_values().flatten().collect()
}

fn hunk_at<'a>(diff: &ReviewDiff, hunks: &'a [Hunk], line: usize) -> Option<&'a Hunk> {
    let (deleted, n) = number(diff, line)?;
    let side = |h: &Hunk| if deleted { h.old.clone() } else { h.new.clone() };
    // Hunks are in file order on both sides; this runs for every line on every frame.
    let at = hunks.partition_point(|h| side(h).end <= n);
    hunks.get(at).filter(|h| side(h).contains(&n))
}

/// Outermost first: each the nearest line above indented less.
fn scopes(diff: &ReviewDiff, line: usize, floor: usize) -> Vec<usize> {
    let Some(&kind) = diff.kinds.get(line).filter(|k| **k != Kind::Gap) else { return Vec::new() };
    let kind = if kind == Kind::Removed { Kind::Removed } else { Kind::Added };
    let on_side = |i: &usize| diff.kinds[*i] == Kind::Same || diff.kinds[*i] == kind;
    let indent = |i: usize| {
        let mut chars = diff.lines[i].spans.iter().flat_map(|s| s.content.chars()).skip(diff.gutter);
        let mut spaces = 0;
        loop {
            match chars.next() {
                Some(' ') => spaces += 1,
                Some(c) if !c.is_whitespace() => return Some((spaces, c)),
                _ => return None,
            }
        }
    };
    let Some((mut want, _)) = (line..diff.kinds.len()).filter(on_side).take(50).find_map(indent) else { return Vec::new() };
    let mut chain = Vec::new();
    for i in (floor..line).rev().filter(on_side) {
        if want == 0 {
            break;
        }
        // A closing line ends a block before this one: `} else {` too, whose `if` is the scope.
        if let Some((n, _)) = indent(i).filter(|&(n, c)| n < want && !matches!(c, '}' | ')' | ']')) {
            chain.push(i);
            want = n;
        }
    }
    chain.reverse();
    chain
}

fn hunks(root: &Path, against: &str, files: &[File]) -> HashMap<PathBuf, Vec<Hunk>> {
    // Paths are read off the prefixes, which a user's diff.noprefix would drop.
    let diff = git(root, &["-c", "core.quotePath=false", "diff", "--no-color", "--no-ext-diff", "--no-renames", "--src-prefix=a/", "--dst-prefix=b/", "-U3", "--inter-hunk-context=0", against]).unwrap_or_default();
    let mut all = parse_hunks(root, &diff);
    // A whole diff is at least the new file: one past the viewer's limit shows no lines to approve.
    all.retain(|path, _| !std::fs::metadata(path).is_ok_and(|m| m.len() > 2 * viewer::MAX_BYTES as u64));
    // shortcut: like the line counts, only 200 new files under 1 MB are read, so others can't be approved; read them when shown if that bites.
    for file in files.iter().filter(|f| f.status == '?').take(200) {
        let path = root.join(&file.path);
        // shortcut: a link isn't read (git shows its target's path), so can't be approved; hash `read_link` if that bites.
        if !std::fs::symlink_metadata(&path).is_ok_and(|m| m.is_file() && m.len() < 1024 * 1024) {
            continue;
        }
        let Ok(bytes) = std::fs::read(&path) else { continue };
        // git calls a file binary by its first 8000 bytes.
        if bytes.is_empty() || bytes[..bytes.len().min(8000)].contains(&0) {
            all.insert(path, Vec::new());
            continue;
        }
        let text = String::from_utf8_lossy(&bytes);
        let lines: Vec<String> = text.lines().map(|l| format!("+{l}")).collect();
        let id = hunk_id(&path, &lines.iter().map(String::as_str).collect::<Vec<_>>(), 1);
        all.insert(path, vec![Hunk { old: 0..0, new: 1..lines.len() + 1, id }]);
    }
    all
}

fn parse_hunks(root: &Path, diff: &str) -> HashMap<PathBuf, Vec<Hunk>> {
    let mut all = HashMap::new();
    for file in format!("\n{diff}").split("\ndiff --git ").skip(1) {
        let mut lines = file.lines();
        let names = lines.next().unwrap_or_default();
        // A deleted file has no size to check above; its diff is at least as big.
        if file.len() > 2 * viewer::MAX_BYTES {
            continue;
        }
        // "a/P b/P": the same path twice, with no renames.
        // shortcut: a quoted path (odd characters) is skipped, so can't be approved; unquote it if that bites.
        let Some(path) = names.get(2..2 + names.len().saturating_sub(5) / 2).filter(|_| !names.starts_with('"')) else { continue };
        let mut hunks: Vec<(Hunk, Vec<&str>)> = Vec::new();
        for line in lines {
            if let Some(header) = line.strip_prefix("@@ ") {
                let range = |prefix: char| -> Range<usize> {
                    let part = header.split(' ').find_map(|p| p.strip_prefix(prefix)).unwrap_or("1");
                    let (start, len) = part.split_once(',').unwrap_or((part, "1"));
                    let start = start.parse().unwrap_or(1);
                    start..start + len.parse().unwrap_or(1)
                };
                hunks.push((Hunk { old: range('-'), new: range('+'), id: 0 }, Vec::new()));
            } else if let Some((_, changed)) = hunks.last_mut().filter(|_| line.starts_with(['+', '-'])) {
                changed.push(line);
            }
        }
        let path = root.join(path);
        let mut seen: HashMap<u64, u64> = HashMap::new();
        let hunks = hunks
            .into_iter()
            .map(|(hunk, changed)| {
                let nth = seen.entry(hunk_id(&path, &changed, 0)).or_default();
                *nth += 1;
                Hunk { id: hunk_id(&path, &changed, *nth), ..hunk }
            })
            .collect();
        all.insert(path, hunks);
    }
    all
}

/// `nth` tells apart the same change made twice in a file.
fn hunk_id(path: &Path, changed: &[&str], nth: u64) -> u64 {
    use std::os::unix::ffi::OsStrExt;
    let mut bytes = path.as_os_str().as_bytes().to_vec();
    for line in changed {
        bytes.push(0);
        bytes.extend_from_slice(line.as_bytes());
    }
    bytes.extend_from_slice(&nth.to_le_bytes());
    crate::worktree::fnv(&bytes)
}

fn cell_lines(cell: usize, cells: usize, lines: usize) -> (usize, usize) {
    let start = cell * lines / cells.max(1);
    (start, ((cell + 1) * lines / cells.max(1)).max(start + 1).min(lines))
}

fn git(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").current_dir(root).args(["--no-optional-locks"]).args(args).output().ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}

const EMPTY_TREE: &str = "4b825dc642cb6eb9a060e54bf8d69288fbee4904";

/// Where a branch left the main one; the main branch itself is HEAD.
fn base(root: &Path) -> (String, String) {
    let uncommitted = || ("HEAD".to_string(), String::new());
    let branch = git(root, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    let main = git(root, &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"])
        .or_else(|| ["main", "master"].iter().find(|b| git(root, &["rev-parse", "--verify", "--quiet", b]).is_some()).map(|b| b.to_string()));
    let Some(main) = main.filter(|m| m.trim_start_matches("origin/") != branch) else { return uncommitted() };
    match git(root, &["merge-base", "HEAD", &main]) {
        Some(commit) => (commit, format!("since {main}")),
        None => uncommitted(),
    }
}

fn changed(root: &Path, against: &str, untracked: bool) -> Vec<(PathBuf, char)> {
    let mut files: Vec<(PathBuf, char)> = Vec::new();
    if let Some(out) = git(root, &["diff", "--name-status", "--no-renames", "-z", against]) {
        let mut parts = out.split('\0').filter(|p| !p.is_empty());
        while let (Some(status), Some(path)) = (parts.next(), parts.next()) {
            files.push((PathBuf::from(path), status.chars().next().unwrap_or('M')));
        }
    }
    if let Some(out) = git(root, &["ls-files", "--others", "--exclude-standard", "-z"]).filter(|_| untracked) {
        files.extend(out.split('\0').filter(|p| !p.is_empty()).map(|p| (PathBuf::from(p), '?')));
    }
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> PathBuf {
        let t = std::env::temp_dir().join(format!("pulpo-review-{}-{:?}", std::process::id(), std::thread::current().id()));
        let _ = std::fs::remove_dir_all(&t);
        std::fs::create_dir_all(t.join("src")).unwrap();
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        run(&["init", "-q", "-b", "main"]);
        run(&["config", "user.email", "t@t"]);
        run(&["config", "user.name", "t"]);
        std::fs::write(t.join("a.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::write(t.join("src/b.txt"), "b\n").unwrap();
        run(&["add", "."]);
        run(&["commit", "-qm", "start"]);
        run(&["checkout", "-qb", "task"]);
        std::fs::write(t.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        run(&["commit", "-qam", "agent"]);
        // In .git, so it's no change to show.
        crate::agents::TEST_STATE.set(Some(t.join(".git/pulpo-state")));
        std::fs::write(t.join("src/b.txt"), "b\nmore\n").unwrap();
        std::fs::write(t.join("c.txt"), "new\n").unwrap();
        t
    }

    #[test]
    fn shows_what_isnt_committed_as_a_tree() {
        let t = repo();
        let mut changes = Changes::open(&t, None);
        assert_eq!(changes.since, "since main");
        let rows: Vec<String> = changes.rows.iter().map(|r| r.name.clone()).collect();
        assert_eq!(rows, ["src", "b.txt", "c.txt"]);
        assert_eq!(changes.status_of(&t.join("src")), Some('M'));
        assert_eq!(changes.status_of(&t.join("c.txt")), Some('?'));
        assert_eq!(changes.selected().map(|r| r.name.as_str()), Some("b.txt"));
        assert_eq!(changes.diff.as_ref().map(|(p, d)| (p.clone(), d.added)), Some((t.join("src/b.txt"), 1)));
        changes.step(-1);
        assert!(changes.diff.is_none());
        changes.step(1);
        changes.height = 10;
        let marks: Vec<Option<ratatui::style::Color>> = changes.ruler(4).into_iter().map(|(color, _)| color).collect();
        assert_eq!(marks, [None, None, Some(theme::green()), Some(theme::green())]);
        assert!(changes.ruler(4).iter().all(|&(_, in_view)| in_view));
        assert_eq!(cell_lines(3, 4, 2), (1, 2));
        let stale = changes.request().read();
        let fresh = changes.request().read();
        changes.apply(stale);
        changes.apply(fresh);
        assert_eq!(changes.diff.as_ref().map(|(p, _)| p.clone()), Some(t.join("src/b.txt")));
        std::fs::write(t.join("src/b.txt"), "b\n").unwrap();
        let read = changes.request().read();
        changes.apply(read);
        assert_ne!(changes.diff.as_ref().map(|(p, _)| p.clone()), Some(t.join("src/b.txt")));
        std::fs::write(t.join("src/b.txt"), "b\nmore\n").unwrap();
        let read = changes.request().read();
        changes.apply(read);
        changes.state.select(Some(0));
        changes.fold(false);
        assert_eq!(changes.rows.len(), 2);
        changes.fold(true);
        assert_eq!(changes.rows.len(), 3);
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        run(&["stash", "-qu"]);
        run(&["checkout", "-q", "main"]);
        std::fs::write(t.join("a.txt"), "main\n").unwrap();
        let mut on_main = Changes::open(&t, None);
        assert_eq!((on_main.since.as_str(), on_main.files.len()), ("", 1));
        run(&["commit", "-qam", "done"]);
        on_main.load();
        assert!(on_main.files.is_empty());
        assert_eq!(on_main.history.commits.iter().map(|c| c.subject.as_str()).collect::<Vec<_>>(), ["done", "start"]);
        run(&["checkout", "-q", "task"]);
        run(&["stash", "pop", "-q"]);
        let head = git(&t, &["rev-parse", "HEAD"]).unwrap();
        let changes = Changes::open(&t, Some(head));
        assert_eq!(changes.files.iter().map(|f| f.path.display().to_string()).collect::<Vec<_>>(), vec!["src/b.txt", "c.txt"]);
        assert!(changes.history.commits.is_empty());
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn shows_a_commit_on_its_own() {
        let t = repo();
        let history = Changes::open(&t, None).history;
        let mut agent = Changes::of_commit(&t, &history.commits[0]);
        agent.load();
        assert_eq!(agent.files.iter().map(|f| (f.path.display().to_string(), f.status)).collect::<Vec<_>>(), [("a.txt".to_string(), 'M')]);
        assert_eq!(agent.diff.as_ref().map(|(_, d)| (d.added, d.removed)), Some((1, 1)));
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        run(&["checkout", "-q", "-f", "main"]);
        let mut start = Changes::of_commit(&t, &Changes::open(&t, None).history.commits[0]);
        start.load();
        assert_eq!(start.files.len(), 2);
        std::fs::remove_dir_all(&t).unwrap();
    }

    fn comment(changes: &mut Changes, reading: bool, text: &str) {
        let comment = changes.commenting(reading).unwrap();
        changes.keep(Comment { text: text.into(), ..comment });
    }

    #[test]
    fn long_lines_wrap() {
        let t = repo();
        std::fs::write(t.join("src/b.txt"), format!("b\n{}\n", "x".repeat(25))).unwrap();
        let mut changes = Changes::open(&t, None);
        changes.height = 10;
        changes.fit(17, None);
        assert_eq!(changes.shown, [Shown::Row(0), Shown::Row(1), Shown::Wrap { row: 1, piece: 1 }, Shown::Wrap { row: 1, piece: 2 }]);
        changes.click(3, false);
        assert_eq!(changes.cursor, 1);
        changes.page(true);
        assert_eq!(changes.scroll, 1);
        changes.page(false);
        assert_eq!(changes.scroll, 0);
        changes.height = 2;
        changes.scroll_by(100);
        changes.fit(17, None);
        assert_eq!(changes.shown, [Shown::Row(1), Shown::Wrap { row: 1, piece: 1 }]);
        assert_eq!(wrap_points(&Line::from(format!("1 + {}", "漢".repeat(6))), 4, 10), [0, 5]);
        assert_eq!(wrap_points(&Line::from(format!("1 + ab{}", " ".repeat(30))), 4, 10), [0]);
    }

    #[test]
    fn comments_say_where_they_are_and_show_under_their_lines() {
        let t = repo();
        let mut changes = Changes::open(&t, None);
        assert_eq!(changes.left(), 2);
        comment(&mut changes, true, "why?");
        assert!(changes.commented(1) && !changes.commented(0));
        assert!(changes.on_commented() && changes.left() == 1);
        (changes.cursor, changes.anchor) = (1, Some(0));
        comment(&mut changes, true, "both");
        comment(&mut changes, false, "the file");
        assert_eq!(changes.review(), "src/b.txt\nthe file\n\nsrc/b.txt:1-2\nboth\n\nsrc/b.txt:2\nwhy?");
        let why = changes.comment_at(true).unwrap();
        assert_eq!(why.text, "why?");
        changes.delete(&why);
        assert_eq!(changes.count(&t.join("src")), 2);
        changes.height = 10;
        changes.fit(80, None);
        assert_eq!(changes.shown, [Shown::Note(Some(1), "the file".into()), Shown::Fold(0..1), Shown::Done(1..2)]);
        assert_eq!(changes.done_label(&(1..2)), (true, "✎ +1 · both".to_string()));
        let writing = Comment { text: "a\nb".into(), ..changes.commenting(true).unwrap() };
        changes.fit(80, Some((&writing, 1)));
        assert_eq!(changes.shown[1..5], [Shown::Row(0), Shown::Row(1), Shown::Note(None, format!("a{CURSOR}")), Shown::Note(None, "b".into())]);
        assert_eq!(changes.shown[5], Shown::Hint);
        assert_eq!(wrap("ab漢字", 4), ["ab漢", "字"]);
        let mut agent = Changes::of_commit(&t, &changes.history.commits[0]);
        agent.load();
        let hash = agent.commit.as_ref().unwrap().hash.clone();
        assert_eq!(agent.commenting(true).map(|c| c.at(&t)), Some(format!("a.txt:2 (deleted) in commit {hash}")));
        agent.move_cursor(1, true);
        assert_eq!(agent.commenting(true).map(|c| c.at(&t)), Some(format!("a.txt:2 in commit {hash}")));
        agent.comments = std::mem::take(&mut changes.comments);
        assert_eq!(agent.count(&t.join("src")), 0);
        agent.keep(Comment { id: 0, path: t.join("a.txt"), commit: None, lines: None, text: "no\x1b[201~ escape\tok".into(), code: Vec::new(), outdated: false });
        assert!(agent.review().starts_with("a.txt\nno[201~ escape\tok\n\nsrc/b.txt"));
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        run(&["commit", "-qam", "b"]);
        changes.comments = agent.comments.split_off(1);
        changes.load();
        assert_eq!(changes.rows.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["src", "b.txt", "a.txt", "c.txt"]);
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn a_commented_change_folds_and_counts_as_done() {
        let t = repo();
        let mut changes = Changes::open(&t, None);
        comment(&mut changes, true, "why?");
        (changes.cursor, changes.anchor) = (1, Some(0));
        comment(&mut changes, true, "both");
        changes.height = 10;
        changes.fit(80, None);
        assert_eq!(changes.done_label(&(1..2)), (true, "✎ +1 · why? (2 comments)".to_string()));
        assert_eq!(changes.next_change(true, true), None);
        assert_eq!(changes.diff.as_ref().map(|(p, _)| p.clone()), Some(t.join("c.txt")));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn comments_follow_their_code() {
        let t = repo();
        let b = t.join("src/b.txt");
        let mut changes = Changes::open(&t, None);
        comment(&mut changes, true, "why?");
        let draft = Comment { text: "why now?".into(), ..changes.comments[0].clone() };
        std::fs::write(&b, "b\nnew\nmore\n").unwrap();
        changes.follow_all();
        changes.keep(draft);
        let kept = |c: &Changes| c.comments.iter().map(|c| (c.lines, c.outdated, c.text.clone())).collect::<Vec<_>>();
        assert_eq!(kept(&changes), [(Some((false, 3, 3)), false, "why now?".into())]);
        std::fs::write(&b, "b\nless\nx\n").unwrap();
        changes.load();
        assert!(changes.comments[0].outdated && !changes.commented(2));
        assert!(changes.review().ends_with("src/b.txt:3 (outdated, it was on:)\n> more\nwhy now?"));
        (changes.cursor, changes.anchor) = (2, None);
        assert_eq!(changes.commenting(true).map(|c| c.text), Some(String::new()));
        assert_eq!(changes.comment_at(false).map(|c| c.text), Some("why now?".into()));
        std::fs::write(&b, "b\nmore\n").unwrap();
        changes.load();
        assert_eq!(kept(&changes), [(Some((false, 2, 2)), false, "why now?".into())]);

        changes.comments.clear();
        std::fs::write(&b, "b\n}\nx\n}\n").unwrap();
        changes.load();
        for (cursor, text) in [(1, "one"), (3, "two")] {
            (changes.cursor, changes.anchor) = (cursor, Some(cursor));
            comment(&mut changes, true, text);
        }
        std::fs::write(&b, "b\nx\n}\n").unwrap();
        changes.load();
        let one = Comment { text: "one edited".into(), ..changes.comments[0].clone() };
        changes.keep(one);
        assert_eq!(kept(&changes), [(Some((false, 3, 3)), false, "one edited".into()), (Some((false, 3, 3)), false, "two".into())]);
        std::fs::write(&b, "}\n}\n").unwrap();
        changes.load();
        assert_eq!(changes.comments.iter().map(|c| c.outdated).collect::<Vec<_>>(), [true, true]);
        std::fs::write(&b, "b\n}\nx\n").unwrap();
        changes.load();
        assert_eq!(changes.comments.iter().map(|c| (c.lines, c.outdated)).collect::<Vec<_>>(), [(Some((false, 2, 2)), false); 2]);

        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        std::fs::write(&b, "b\nb\n}\nx\n").unwrap();
        run(&["commit", "-qam", "b"]);
        changes.follow_all();
        assert_eq!(changes.comments.iter().map(|c| (c.lines, c.outdated)).collect::<Vec<_>>(), [(Some((false, 3, 3)), false); 2]);

        changes.comments.clear();
        std::fs::write(&b, "b\n\tmore\n").unwrap();
        changes.load();
        let added = changes.diff.as_ref().and_then(|(_, d)| d.kinds.iter().position(|k| *k == Kind::Added)).unwrap();
        (changes.cursor, changes.anchor) = (added, None);
        comment(&mut changes, true, "tabbed");
        run(&["commit", "-qam", "tab"]);
        changes.follow_all();
        changes.load();
        assert_eq!((changes.comments[0].outdated, changes.comments[0].lines), (false, Some((false, 2, 2))));
        comment(&mut changes, false, "the file");
        let mut taken: Vec<String> = (0..3).filter_map(|_| changes.comment_at(false).map(|c| c.text)).collect();
        assert_eq!(taken[0], taken[2]);
        taken[..2].sort();
        assert_eq!(taken[..2], ["tabbed", "the file"]);
        assert!(changes.comment_at(true).is_some());

        let c = t.join("c.txt");
        let row = changes.rows.iter().position(|r| r.path == c).unwrap();
        changes.state.select(Some(row));
        changes.follow();
        (changes.cursor, changes.anchor) = (0, None);
        comment(&mut changes, true, "new file");
        std::fs::remove_file(&c).unwrap();
        changes.follow_all();
        assert!(changes.comments.iter().find(|c| c.text == "new file").unwrap().outdated);
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn folds_around_changes_and_keeps_approvals_through_edits() {
        let t = repo();
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        let long = t.join("src/long.rs");
        let write = |a: &str, b: &str| {
            let lines = |s: &str| format!("        {s};\n").repeat(20);
            let text = format!("impl X {{\n    fn one() {{\n{}        {a};\n{}    }}\n    fn two() {{\n{}        {b};\n    }}\n}}\n", lines("x"), lines("y"), lines("z"));
            std::fs::write(&long, text).unwrap();
        };
        write("a", "b");
        run(&["add", "src/long.rs"]);
        run(&["commit", "-qm", "long"]);
        write("a2", "b2");
        let mut changes = Changes::open(&t, None);
        let row = changes.rows.iter().position(|r| r.path == long).unwrap();
        changes.state.select(Some(row));
        changes.follow();
        changes.height = 40;
        changes.fit(80, None);
        assert_eq!(changes.shown[0], Shown::Fold(0..19));
        assert_eq!(changes.shown[1..9], (19..27).map(Shown::Row).collect::<Vec<_>>());
        assert_eq!(changes.section(), 19..27);
        assert!(matches!(changes.shown[9], Shown::Fold(_)));
        assert_eq!(changes.fold_label(&(0..19)), ("⋯ 19 lines".to_string(), vec!["impl X {".to_string(), "fn one() {".to_string()]));

        assert_eq!(changes.approve(), None);
        assert_eq!(changes.progress(&long), (1, 2));
        assert_eq!(load_approved().len(), 1);
        let mut restarted = Changes::new(&t, None);
        restarted.approved = load_approved();
        restarted.load();
        assert_eq!(restarted.progress(&long), (1, 2));
        let text = |c: &Changes| viewer::plain(&c.diff.as_ref().unwrap().1.lines[c.cursor], c.diff.as_ref().unwrap().1.gutter).trim().to_string();
        assert_eq!(text(&changes), "b;");
        changes.fit(80, None);
        assert!(changes.shown.contains(&Shown::Done(22..24)));
        changes.cursor = 22;
        assert_eq!(changes.section(), 22..24);
        let draft = changes.commenting(true).unwrap();
        changes.fit(80, Some((&draft, 0)));
        assert!(changes.shown.contains(&Shown::Row(22)) && changes.shown.contains(&Shown::Hint));
        changes.fit(80, None);
        assert!(changes.shown.contains(&Shown::Done(22..24)));
        changes.cursor = 66;
        assert_eq!(changes.done_label(&(22..24)), (false, "✓ +1 −1 · a;".to_string()));

        assert_eq!(changes.next_change(false, false), None);
        assert_eq!(changes.cursor, 22);
        assert_eq!(changes.approve(), None);
        assert_eq!(changes.progress(&long), (0, 2));
        assert!(load_approved().is_empty());
        changes.fit(80, None);
        assert!(changes.shown.contains(&Shown::Row(22)) && changes.shown.contains(&Shown::Row(23)));
        assert_eq!(changes.approve(), None);

        write("a2", "b3");
        changes.load();
        assert_eq!(changes.progress(&long), (1, 2));
        write("a3", "b3");
        changes.load();
        assert_eq!(changes.progress(&long), (0, 2));

        changes.approved.extend(changes.hunks[&long].iter().map(|h| h.id));
        changes.follow();
        changes.start(true, true);
        assert_eq!(changes.next_change(true, true), None);
        assert_eq!(changes.diff.as_ref().map(|(p, _)| p.clone()), Some(t.join("c.txt")));
        assert_eq!(changes.approve(), Some("no change left to approve below".into()));
        assert_eq!(changes.diff.as_ref().map(|(p, _)| p.clone()), Some(t.join("c.txt")));
        assert_eq!(changes.next_change(false, false), None);
        assert_eq!((changes.diff.as_ref().map(|(p, _)| p.clone()), changes.cursor), (Some(long.clone()), 66));
        assert_eq!(changes.first_unapproved(), None);
        assert_eq!(changes.selected().map(|r| r.path.clone()), Some(t.join("src/b.txt")));
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn scopes_and_hunks_hold_in_odd_cases() {
        let t = repo();
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        let f = t.join("src/f.rs");
        let body = "        x;\n".repeat(30);
        std::fs::write(&f, format!("impl X {{\n    fn bar() {{\n    }}\n    fn foo(a) {{\n{body}        old;\n    }}\n}}\n")).unwrap();
        run(&["add", "src/f.rs"]);
        run(&["commit", "-qm", "f"]);
        std::fs::write(&f, format!("impl X {{\n    fn bar() {{\n    }}\n    fn foo(a, b) {{\n{body}        new;\n    }}\n}}\n")).unwrap();
        std::fs::write(t.join("src/__init__.py"), "").unwrap();
        run(&["config", "diff.noprefix", "true"]);
        let mut changes = Changes::open(&t, None);
        assert_eq!(changes.progress(&t), (0, 4));

        changes.state.select(Some(0));
        changes.fold(false);
        changes.state.select(changes.rows.iter().position(|r| r.path == t.join("c.txt")));
        changes.follow();
        assert_eq!(changes.next_change(false, false), None);
        assert_eq!(changes.selected().map(|r| r.path.clone()), Some(f.clone()));
        let diff = &changes.diff.as_ref().unwrap().1;
        let text = |i: usize| viewer::plain(&diff.lines[i], diff.gutter).trim().to_string();
        assert_eq!(text(changes.cursor), "old;");
        let chain = |line: usize| scopes(diff, line, 0).into_iter().map(text).collect::<Vec<_>>();
        assert_eq!(chain(5), ["impl X {", "fn foo(a, b) {"]);
        assert_eq!(chain(changes.cursor), ["impl X {", "fn foo(a) {"]);
        changes.height = 40;
        changes.fit(80, None);
        assert_eq!(changes.shown[..3], [Shown::Row(0), Shown::Row(1), Shown::Row(2)]);

        let diff = "diff --git a/x b/x\n@@ -0,0 +1,2 @@\n+a\n+b\n@@ -5 +7 @@\n-c\n+d\n@@ -9 +11 @@\n-c\n+d\n";
        let hunks = &parse_hunks(&t, diff)[&t.join("x")];
        assert_eq!(hunks.iter().map(|h| (h.old.clone(), h.new.clone())).collect::<Vec<_>>(), [(0..0, 1..3), (5..6, 7..8), (9..10, 11..12)]);
        assert_ne!(hunks[1].id, hunks[2].id);
        std::fs::remove_dir_all(&t).unwrap();
    }

    #[test]
    fn history_marks_what_origin_has() {
        let t = repo();
        let run = |args: &[&str]| assert!(Command::new("git").current_dir(&t).args(args).output().unwrap().status.success());
        let subjects = |c: &Changes| c.history.commits.iter().map(|c| (c.subject.clone(), c.pushed)).collect::<Vec<_>>();
        let changes = Changes::open(&t, None);
        assert_eq!((subjects(&changes), changes.history.remote.is_none()), (vec![("agent".to_string(), false)], true));
        run(&["update-ref", "refs/remotes/origin/task", "HEAD"]);
        run(&["commit", "-qam", "more"]);
        let changes = Changes::open(&t, None);
        assert_eq!(subjects(&changes), [("more".to_string(), false), ("agent".to_string(), true)]);
        run(&["update-ref", "refs/remotes/origin/task", "HEAD"]);
        run(&["reset", "-q", "HEAD~1"]);
        let changes = Changes::open(&t, None);
        assert_eq!((subjects(&changes), changes.history.behind), (vec![("agent".to_string(), true)], 1));
        run(&["checkout", "-q", "-f", "main"]);
        run(&["commit", "-q", "--allow-empty", "-m", "later"]);
        let mut on_main = Changes::open(&t, None);
        assert_eq!((on_main.history.commits.len(), on_main.history.more), (2, false));
        on_main.history_limit = 1;
        on_main.load();
        assert_eq!((subjects(&on_main), on_main.history.more), (vec![("later".to_string(), false)], true));
        std::fs::remove_dir_all(&t).unwrap();
    }
}
