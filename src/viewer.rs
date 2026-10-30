//! Read-only view of a file (syntax highlighted) or of its git diff.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style, Stylize};

use crate::theme::{ADDED_BG, BORDER, DIM, FIND_BG, GREEN, HOVER, MARK_BG, RED, REMOVED_BG, YELLOW};
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

const MAX_BYTES: usize = 2 * 1024 * 1024;
const MAX_HIGHLIGHTED_LINES: usize = 10_000;

#[derive(Clone, PartialEq)]
pub enum Source {
    File(PathBuf),
    Diff { path: PathBuf, untracked: bool },
    /// A commit, by hash.
    Commit(String),
    /// What a commit changed in one file.
    CommitFile { hash: String, path: PathBuf },
}

#[derive(Default)]
pub struct Viewer {
    pub source: Option<Source>,
    pub title: String,
    /// Rows of the single-column layout.
    lines: Vec<Line<'static>>,
    pub scroll: usize,
    /// Visible rows, set when drawing.
    pub height: usize,
    /// A highlighted line (a search match), and a line to bring into view
    /// on the next draw.
    pub mark: Option<usize>,
    pub goto: Option<usize>,
    /// Takes the whole right side instead of sharing it with the terminal.
    pub expanded: bool,
    pub find: Option<Find>,
    /// Width of the line-number column, which searching skips.
    gutter: usize,
    /// First line of each block of changes in a diff, and the one last
    /// jumped to.
    changes: Vec<usize>,
    change_at: Option<usize>,
    /// Side-by-side layout of a diff (old | new), its change starts, and the
    /// row maps between the two layouts.
    pairs: Vec<Pair>,
    pair_changes: Vec<usize>,
    to_pair: Vec<usize>,
    to_inline: Vec<usize>,
    /// Showing the side-by-side layout; rows, scroll and marks are then pair rows.
    split: bool,
    /// Columns of code scrolled off to the left (line numbers stay), and the
    /// longest line, which bounds it.
    hscroll: usize,
    widest: usize,
}

/// Old and new version of a side-by-side row; None leaves that side blank.
type Pair = (Option<Line<'static>>, Option<Line<'static>>);

/// A diff shows side by side when each side gets at least 85 columns, enough
/// for most code lines (a 14" laptop stays in one column, larger screens split).
const SPLIT_MIN_WIDTH: u16 = 2 * 85 + 1;

/// Search inside the open file.
#[derive(Default)]
pub struct Find {
    pub query: String,
    /// Still typing the query (matches update live).
    pub typing: bool,
    /// Lines with a match, and which one is selected.
    pub matches: Vec<usize>,
    pub current: usize,
    /// Where the view was when the search started; the first match after it
    /// is selected while typing.
    origin: usize,
}

impl Find {
    /// Case-insensitive unless the query has capitals.
    fn ignore_case(&self) -> bool {
        !self.query.chars().any(char::is_uppercase)
    }
}

impl Viewer {
    pub fn is_open(&self) -> bool {
        self.source.is_some()
    }

    pub fn close(&mut self) {
        *self = Viewer { height: self.height, ..Viewer::default() };
    }

    pub fn open(&mut self, source: Source, root: &Path) {
        if self.source.as_ref() != Some(&source) {
            self.source = Some(source);
            self.scroll = 0;
            self.mark = None;
            self.find = None;
            self.change_at = None;
            self.hscroll = 0;
            self.reload(root);
            // Land on the first change.
            self.next_change(1);
        }
    }

    /// Open a file with `line` highlighted and in view.
    pub fn open_at(&mut self, path: PathBuf, line: Option<usize>, root: &Path) {
        self.open(Source::File(path), root);
        self.mark = line;
        self.goto = line;
        if line.is_none() {
            self.scroll = 0;
        }
    }

    /// Re-read the source, keeping the scroll position (files change under
    /// us while agents work).
    pub fn reload(&mut self, root: &Path) {
        let Some(source) = &self.source else { return };
        let relative = |path: &Path| path.strip_prefix(root).unwrap_or(path).display().to_string();
        let (name, content) = match source {
            Source::File(path) => (relative(path), load_file(path)),
            Source::Diff { path, untracked } => (relative(path), load_diff(diff_command(root, path, *untracked), path)),
            Source::Commit(hash) => (format!("commit {hash}"), load_commit(root, hash)),
            Source::CommitFile { hash, path } => (format!("{hash}  {}", relative(path)), load_diff(show_command(root, hash, path), path)),
        };
        self.title = match content.counts {
            Some((added, removed)) => format!("{name}  +{added} −{removed}"),
            None => name.to_string(),
        };
        self.lines = content.lines;
        self.gutter = content.gutter;
        self.changes = content.changes;
        self.pairs = content.pairs;
        self.pair_changes = content.pair_changes;
        self.to_pair = content.to_pair;
        self.to_inline = content.to_inline;
        self.split &= !self.pairs.is_empty();
        let gutter = self.gutter;
        self.widest = self.lines.iter().map(|l| plain(l, gutter).trim_end().chars().count()).max().unwrap_or(0);
        self.hscroll = self.hscroll.min(self.widest);
        self.change_at = self.change_at.filter(|&i| i < self.changes.len());
        self.scroll_by(0);
        self.update_find(false);
    }

    /// Pick the layout for the available width, keeping the same place in view.
    pub fn set_width(&mut self, width: u16) {
        let split = width >= SPLIT_MIN_WIDTH && !self.pairs.is_empty();
        if split == self.split {
            return;
        }
        let map = if split { self.to_pair.clone() } else { self.to_inline.clone() };
        let convert = |row: usize| map.get(row).copied().unwrap_or(0);
        self.scroll = convert(self.scroll);
        self.goto = self.goto.map(convert);
        self.mark = self.mark.map(convert);
        if let Some(find) = &mut self.find {
            find.origin = convert(find.origin);
        }
        self.split = split;
        if self.find.is_some() {
            self.update_find(false);
            self.mark = self.find.as_ref().and_then(|f| f.matches.get(f.current)).copied();
        }
    }

    pub fn row_count(&self) -> usize {
        if self.split { self.pairs.len() } else { self.lines.len() }
    }

    /// The searchable text of a row, without line numbers.
    fn row_text(&self, row: usize) -> String {
        if self.split {
            let side = |line: &Option<Line>| line.as_ref().map(|l| plain(l, self.gutter)).unwrap_or_default();
            let (old, new) = &self.pairs[row];
            format!("{}\n{}", side(old), side(new))
        } else {
            plain(&self.lines[row], self.gutter)
        }
    }

    pub fn is_diff(&self) -> bool {
        matches!(self.source, Some(Source::Diff { .. } | Source::CommitFile { .. }))
    }

    /// n / p: the next search match if searching, otherwise the next change.
    pub fn next(&mut self, step: isize) {
        if self.find.is_some() {
            self.next_match(step);
        } else {
            self.next_change(step);
        }
    }

    fn next_change(&mut self, step: isize) {
        let changes = if self.split { &self.pair_changes } else { &self.changes };
        if changes.is_empty() {
            return;
        }
        let count = changes.len() as isize;
        let i = match self.change_at {
            Some(i) => (i as isize + step).rem_euclid(count) as usize,
            None => changes.iter().position(|&row| row >= self.scroll).unwrap_or(0),
        };
        self.goto = Some(changes[i]);
        self.change_at = Some(i);
    }

    pub fn start_find(&mut self) {
        self.find = Some(Find { typing: true, origin: self.scroll, ..Find::default() });
    }

    pub fn edit_find(&mut self, edit: impl FnOnce(&mut String)) {
        if let Some(find) = &mut self.find {
            edit(&mut find.query);
        }
        self.update_find(true);
    }

    /// Stop typing; a search without matches is dropped.
    pub fn finish_find(&mut self) {
        match &mut self.find {
            Some(find) if !find.matches.is_empty() => find.typing = false,
            _ => self.clear_find(),
        }
    }

    pub fn clear_find(&mut self) {
        if self.find.take().is_some() {
            self.mark = None;
        }
    }

    pub fn next_match(&mut self, step: isize) {
        if let Some(find) = &mut self.find
            && !find.matches.is_empty()
        {
            let count = find.matches.len() as isize;
            find.current = (find.current as isize + step).rem_euclid(count) as usize;
        }
        self.show_match();
    }

    fn update_find(&mut self, jump: bool) {
        let Some((needle, ignore_case)) = self.find.as_ref().map(|f| (fold(&f.query, f.ignore_case()), f.ignore_case()))
        else {
            return;
        };
        let matches = if needle.is_empty() {
            Vec::new()
        } else {
            (0..self.row_count()).filter(|&row| fold(&self.row_text(row), ignore_case).contains(&needle)).collect()
        };
        let Some(find) = &mut self.find else { return };
        find.matches = matches;
        find.current = if jump {
            let origin = find.origin;
            find.matches.iter().position(|&line| line >= origin).unwrap_or(0)
        } else {
            find.current.min(find.matches.len().saturating_sub(1))
        };
        if jump {
            self.show_match();
        }
    }

    /// Mark the selected match and bring it into view.
    fn show_match(&mut self) {
        let line = self.find.as_ref().and_then(|f| f.matches.get(f.current)).copied();
        self.mark = line;
        if let Some(line) = line
            && (line < self.scroll || line >= self.scroll + self.height.max(1))
        {
            self.goto = Some(line);
        }
    }

    /// Row `i` as drawn at `width`: search matches are highlighted, the
    /// marked row gets a background, side-by-side halves are fitted.
    pub fn decorated(&self, i: usize, width: u16) -> Line<'static> {
        let find = self.find.as_ref().filter(|f| !f.query.is_empty());
        let highlight = |line: Line<'static>| {
            let line = match find {
                Some(f) => highlight_matches(line, &f.query, f.ignore_case(), self.gutter),
                None => line,
            };
            shift(line, self.gutter, self.hscroll)
        };
        let mut line = if self.split {
            let left = (width.saturating_sub(1) / 2) as usize;
            let (old, new) = &self.pairs[i];
            let mut spans = fit(old.clone().map(highlight), left);
            spans.push(Span::styled("│", Style::default().fg(BORDER)));
            spans.extend(fit(new.clone().map(highlight), (width as usize).saturating_sub(left + 1)));
            Line::from(spans)
        } else {
            highlight(self.lines[i].clone())
        };
        if self.mark == Some(i) {
            line = line.patch_style(Style::default().bg(MARK_BG));
        }
        line
    }

    /// The title, with the column when scrolled sideways.
    pub fn heading(&self) -> String {
        match self.hscroll {
            0 => self.title.clone(),
            n => format!("{}  · from column {}", self.title, n + 1),
        }
    }

    /// Scroll sideways; 0 columns goes back to the start of the lines.
    pub fn scroll_sideways(&mut self, columns: isize) {
        self.hscroll = if columns == 0 { 0 } else { self.hscroll.saturating_add_signed(columns).min(self.widest) };
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.row_count().saturating_sub(self.height.max(1));
        self.scroll = self.scroll.saturating_add_signed(rows).min(max);
    }
}

/// What the viewer shows for a source.
#[derive(Default)]
struct Content {
    lines: Vec<Line<'static>>,
    /// Width of the line-number column.
    gutter: usize,
    /// First line of each block of changes (diffs only).
    changes: Vec<usize>,
    /// Lines added and removed (diffs only).
    counts: Option<(usize, usize)>,
    /// Side-by-side layout (diffs only), see Viewer.
    pairs: Vec<Pair>,
    pair_changes: Vec<usize>,
    to_pair: Vec<usize>,
    to_inline: Vec<usize>,
}

impl Content {
    fn note(text: &str) -> Self {
        Content { lines: vec![note(text)], ..Content::default() }
    }
}

fn load_file(path: &Path) -> Content {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) => return Content::note(&format!("cannot read file: {e}")),
    };
    if bytes.len() > MAX_BYTES {
        return Content::note("file too large to preview");
    }
    if bytes[..bytes.len().min(8000)].contains(&0) {
        return Content::note("binary file");
    }
    let text = String::from_utf8_lossy(&bytes);
    let (syntaxes, theme) = assets();
    let mut highlighter = HighlightLines::new(syntax_for(path), theme);
    let width = text.lines().count().to_string().len();
    let lines = LinesWithEndings::from(&text)
        .enumerate()
        .map(|(i, line)| {
            let ranges = if i < MAX_HIGHLIGHTED_LINES { highlighter.highlight_line(line, syntaxes).ok() } else { None };
            let mut spans = vec![Span::from(format!("{:>width$}  ", i + 1)).fg(DIM)];
            spans.extend(styled(ranges, line));
            Line::from(spans)
        })
        .collect();
    Content { lines, gutter: width + 2, ..Content::default() }
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Same,
    Added,
    Removed,
    /// Unchanged lines git left out.
    Gap,
}

/// A file's uncommitted changes, with enough context to get the whole file
/// in one hunk.
fn diff_command(root: &Path, path: &Path, untracked: bool) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root).args(["--no-optional-locks", "diff", "--no-color", "--no-ext-diff", "--unified=1000000"]);
    if untracked {
        cmd.args(["--no-index", "--", "/dev/null"]).arg(path);
    } else {
        cmd.args(["HEAD", "--"]).arg(path);
    }
    cmd
}

/// What a commit changed in a file (a merge: compared to its first parent).
fn show_command(root: &Path, hash: &str, path: &Path) -> Command {
    let mut cmd = Command::new("git");
    cmd.current_dir(root)
        .args(["--no-optional-locks", "show", "--no-color", "--no-ext-diff", "--diff-merges=first-parent", "--format=", "--unified=1000000"])
        .args([hash, "--"])
        .arg(path);
    cmd
}

/// The whole file with its changes inline: deleted lines in red where they
/// were, added lines in green, both syntax highlighted.
fn load_diff(mut cmd: Command, path: &Path) -> Content {
    let text = match cmd.output() {
        Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
        Err(e) => return Content::note(&format!("git diff failed: {e}")),
    };
    if text.is_empty() {
        return Content::note("no textual changes");
    }
    if text.len() > 2 * MAX_BYTES {
        return Content::note("diff too large to show");
    }
    if text.lines().any(|l| l.starts_with("Binary files ")) {
        return Content::note("binary file changed");
    }

    // (kind, old line number, new line number, text)
    let mut rows: Vec<(Kind, usize, usize, &str)> = Vec::new();
    let (mut old, mut new) = (0, 0);
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if in_hunk {
                rows.push((Kind::Gap, 0, 0, ""));
            }
            in_hunk = true;
            (old, new) = hunk_starts(header);
            continue;
        }
        if !in_hunk {
            continue;
        }
        match line.chars().next() {
            Some('+') => {
                rows.push((Kind::Added, 0, new, &line[1..]));
                new += 1;
            }
            Some('-') => {
                rows.push((Kind::Removed, old, 0, &line[1..]));
                old += 1;
            }
            Some('\\') => {} // "\ No newline at end of file"
            _ => {
                rows.push((Kind::Same, old, new, line.get(1..).unwrap_or("")));
                (old, new) = (old + 1, new + 1);
            }
        }
    }

    // One highlighter per side, so each follows its own version of the file.
    let (syntaxes, theme) = assets();
    let syntax = syntax_for(path);
    let (mut new_side, mut old_side) = (HighlightLines::new(syntax, theme), HighlightLines::new(syntax, theme));
    let width = rows.iter().map(|r| r.1.max(r.2)).max().unwrap_or(0).to_string().len();
    let mut content = Content { gutter: width + 3, ..Content::default() };
    let (mut added, mut removed) = (0, 0);
    // Unchanged rows as the old side shows them (with old line numbers).
    let mut old_same: Vec<Option<Line<'static>>> = vec![None; rows.len()];
    for (i, &(kind, old_number, new_number, text)) in rows.iter().enumerate() {
        if kind == Kind::Gap {
            content.lines.push(note("⋯"));
            continue;
        }
        let number = if kind == Kind::Removed { old_number } else { new_number };
        let with_newline = format!("{text}\n");
        let ranges = match kind {
            _ if i >= MAX_HIGHLIGHTED_LINES => None,
            Kind::Added => new_side.highlight_line(&with_newline, syntaxes).ok(),
            Kind::Removed => old_side.highlight_line(&with_newline, syntaxes).ok(),
            _ => {
                let _ = old_side.highlight_line(&with_newline, syntaxes);
                new_side.highlight_line(&with_newline, syntaxes).ok()
            }
        };
        let (marker, fg, bg) = match kind {
            Kind::Added => ('+', GREEN, Some(ADDED_BG)),
            Kind::Removed => ('-', RED, Some(REMOVED_BG)),
            _ => (' ', DIM, None),
        };
        if bg.is_some() && (i == 0 || matches!(rows[i - 1].0, Kind::Same | Kind::Gap)) {
            content.changes.push(content.lines.len());
        }
        match kind {
            Kind::Added => added += 1,
            Kind::Removed => removed += 1,
            _ => {}
        }
        let code = styled(ranges, &with_newline);
        if kind == Kind::Same {
            let mut spans = vec![Span::styled(format!("{old_number:>width$}   "), Style::default().fg(fg))];
            spans.extend(code.clone());
            old_same[i] = Some(Line::from(spans));
        }
        let mut spans = vec![Span::styled(format!("{number:>width$} {marker} "), Style::default().fg(fg))];
        spans.extend(code);
        let mut line = Line::from(spans);
        if let Some(bg) = bg {
            // Pad so the background reaches across the view.
            line.spans.push(Span::from(" ".repeat(ROW_WIDTH.saturating_sub(text.chars().count()))));
            line = line.style(Style::default().bg(bg));
        }
        content.lines.push(line);
    }
    content.counts = Some((added, removed));

    // Side by side: the same rows, removed ones next to what replaced them.
    let kinds: Vec<Kind> = rows.iter().map(|r| r.0).collect();
    content.to_pair = vec![0; rows.len()];
    let mut was_change = false;
    for (old, new) in pair_rows(&kinds) {
        let p = content.pairs.len();
        for i in old.into_iter().chain(new) {
            content.to_pair[i] = p;
        }
        content.to_inline.push(old.into_iter().chain(new).min().unwrap_or(0));
        let is_change = [old, new].iter().flatten().any(|&i| matches!(kinds[i], Kind::Added | Kind::Removed));
        if is_change && !was_change {
            content.pair_changes.push(p);
        }
        was_change = is_change;
        let left = old.map(|i| old_same[i].clone().unwrap_or_else(|| content.lines[i].clone()));
        let right = new.map(|i| content.lines[i].clone());
        content.pairs.push((left, right));
    }
    content
}

/// A commit's message and the files it touched, with git's +/- bars.
fn load_commit(root: &Path, hash: &str) -> Content {
    let out = Command::new("git")
        .current_dir(root)
        .args(["--no-optional-locks", "show", "--no-color", "--diff-merges=first-parent", "--stat", hash])
        .output();
    let text = match out {
        Ok(out) => String::from_utf8_lossy(&out.stdout).into_owned(),
        Err(e) => return Content::note(&format!("git show failed: {e}")),
    };
    let lines = text
        .lines()
        .map(|line| {
            let line = clean(line);
            if line.starts_with("commit ") {
                return Line::from(line).fg(YELLOW);
            }
            // " src/a.rs | 12 +++---"
            if let Some((file, bar)) = line.split_once(" | ")
                && let Some((count, marks)) = bar.split_once(' ')
                && marks.chars().all(|c| c == '+' || c == '-')
            {
                let plus = marks.chars().filter(|&c| c == '+').count();
                return Line::from(vec![
                    Span::from(format!("{file} | {count} ")),
                    Span::from("+".repeat(plus)).fg(GREEN),
                    Span::from("-".repeat(marks.len() - plus)).fg(RED),
                ]);
            }
            Line::from(line)
        })
        .collect();
    Content { lines, ..Content::default() }
}

/// Side-by-side rows as indices into `kinds`: unchanged rows on both sides, a
/// run of removed rows next to the added rows that replaced it.
fn pair_rows(kinds: &[Kind]) -> Vec<(Option<usize>, Option<usize>)> {
    fn flush(pairs: &mut Vec<(Option<usize>, Option<usize>)>, removed: &mut Vec<usize>, added: &mut Vec<usize>) {
        for k in 0..removed.len().max(added.len()) {
            pairs.push((removed.get(k).copied(), added.get(k).copied()));
        }
        removed.clear();
        added.clear();
    }
    let (mut pairs, mut removed, mut added) = (Vec::new(), Vec::new(), Vec::new());
    for (i, &kind) in kinds.iter().enumerate() {
        match kind {
            Kind::Removed => removed.push(i),
            Kind::Added => added.push(i),
            Kind::Same | Kind::Gap => {
                flush(&mut pairs, &mut removed, &mut added);
                pairs.push((Some(i), Some(i)));
            }
        }
    }
    flush(&mut pairs, &mut removed, &mut added);
    pairs
}

/// Drop `n` characters of code after the first `gutter` ones (the line
/// numbers), for scrolling sideways.
fn shift(line: Line<'static>, gutter: usize, n: usize) -> Line<'static> {
    if n == 0 {
        return line;
    }
    let Line { spans: old, style, alignment } = line;
    let mut spans = Vec::new();
    let mut start = 0;
    for span in old {
        let chars: Vec<char> = span.content.chars().collect();
        let end = start + chars.len();
        let from = gutter.clamp(start, end) - start;
        let to = (gutter + n).clamp(start, end) - start;
        let kept: String = chars[..from].iter().chain(&chars[to..]).collect();
        if !kept.is_empty() {
            spans.push(Span::styled(kept, span.style));
        }
        start = end;
    }
    Line { spans, style, alignment }
}

/// A side-by-side half: the line cut or padded to `width` columns, keeping
/// its background; a missing line is a shaded blank.
fn fit(line: Option<Line<'static>>, width: usize) -> Vec<Span<'static>> {
    let Some(Line { spans, style, .. }) = line else {
        return vec![Span::styled(" ".repeat(width), Style::default().bg(HOVER))];
    };
    let mut out = Vec::new();
    let mut used = 0;
    for span in spans {
        if used >= width {
            break;
        }
        let text: String = span.content.chars().take(width - used).collect();
        used += text.chars().count();
        out.push(Span::styled(text, style.patch(span.style)));
    }
    if used < width {
        out.push(Span::styled(" ".repeat(width - used), style));
    }
    out
}

const ROW_WIDTH: usize = 400;

/// Where a hunk starts in the old and new file, from "-a,b +c,d @@".
fn hunk_starts(header: &str) -> (usize, usize) {
    let start = |prefix: char| {
        header
            .split(' ')
            .find_map(|part| part.strip_prefix(prefix))
            .and_then(|range| range.split(',').next()?.parse().ok())
            .unwrap_or(1)
    };
    (start('-'), start('+'))
}

fn syntax_for(path: &Path) -> &'static SyntaxReference {
    let (syntaxes, _) = assets();
    syntaxes
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| syntaxes.find_syntax_by_extension(&path.extension()?.to_string_lossy()))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

/// Highlighted pieces as spans, or the plain line if highlighting failed.
fn styled(ranges: Option<Vec<(syntect::highlighting::Style, &str)>>, line: &str) -> Vec<Span<'static>> {
    match ranges {
        Some(ranges) => ranges
            .into_iter()
            .map(|(style, piece)| {
                let mut modifier = Modifier::empty();
                if style.font_style.contains(FontStyle::ITALIC) {
                    modifier |= Modifier::ITALIC;
                }
                if style.font_style.contains(FontStyle::BOLD) {
                    modifier |= Modifier::BOLD;
                }
                Span::styled(clean(piece), Style::default().fg(color(style.foreground)).add_modifier(modifier))
            })
            .collect(),
        None => vec![Span::from(clean(line))],
    }
}

/// The text of a line, without its first `skip` characters.
fn plain(line: &Line, skip: usize) -> String {
    line.spans.iter().flat_map(|s| s.content.chars()).skip(skip).collect()
}

fn fold(text: &str, ignore_case: bool) -> String {
    if ignore_case { text.to_lowercase() } else { text.to_string() }
}

/// Split the line's spans so occurrences of `query` (after `skip`
/// characters) stand out.
fn highlight_matches(line: Line<'static>, query: &str, ignore_case: bool, skip: usize) -> Line<'static> {
    let fold = |c: char| if ignore_case { c.to_lowercase().next().unwrap_or(c) } else { c };
    let needle: Vec<char> = query.chars().map(fold).collect();
    let chars: Vec<char> = line.spans.iter().flat_map(|s| s.content.chars()).map(fold).collect();
    let mut hit = vec![false; chars.len()];
    let mut i = skip;
    while i + needle.len() <= chars.len() {
        if chars[i..i + needle.len()] == needle[..] {
            hit[i..i + needle.len()].fill(true);
            i += needle.len();
        } else {
            i += 1;
        }
    }
    if !hit.contains(&true) {
        return line;
    }

    let found = Style::default().bg(FIND_BG);
    let Line { spans: old, style, alignment } = line;
    let mut spans = Vec::new();
    let mut pos = 0;
    for span in old {
        let mut run = String::new();
        let mut run_hit = None;
        for c in span.content.chars() {
            let h = hit[pos];
            pos += 1;
            if let Some(prev) = run_hit
                && prev != h
            {
                spans.push(Span::styled(std::mem::take(&mut run), if prev { span.style.patch(found) } else { span.style }));
            }
            run_hit = Some(h);
            run.push(c);
        }
        if let Some(h) = run_hit {
            spans.push(Span::styled(run, if h { span.style.patch(found) } else { span.style }));
        }
    }
    Line { spans, style, alignment }
}

fn note(text: &str) -> Line<'static> {
    Line::from(format!(" {text}")).fg(DIM).italic()
}

fn clean(text: &str) -> String {
    text.trim_end_matches(['\n', '\r']).replace('\t', "    ")
}

/// Load the syntax definitions in the background so the first file opens fast.
pub fn warm_up() {
    std::thread::spawn(assets);
}

fn assets() -> &'static (SyntaxSet, Theme) {
    static ASSETS: OnceLock<(SyntaxSet, Theme)> = OnceLock::new();
    ASSETS.get_or_init(|| {
        (two_face::syntax::extra_newlines(), crate::theme::syntax())
    })
}

fn color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_removed_rows_with_their_replacements() {
        use Kind::*;
        let pairs = pair_rows(&[Same, Removed, Removed, Added, Same, Added]);
        assert_eq!(
            pairs,
            [(Some(0), Some(0)), (Some(1), Some(3)), (Some(2), None), (Some(4), Some(4)), (None, Some(5))]
        );
    }

    #[test]
    fn scrolls_code_sideways_keeping_line_numbers() {
        let line = Line::from(vec![Span::from("12  "), Span::from("let x"), Span::from(" = 1;")]);
        let text = |l: Line| l.spans.iter().map(|s| s.content.to_string()).collect::<Vec<_>>();
        assert_eq!(text(shift(line.clone(), 4, 0)), ["12  ", "let x", " = 1;"]);
        assert_eq!(text(shift(line.clone(), 4, 4)), ["12  ", "x", " = 1;"]);
        assert_eq!(text(shift(line, 4, 7)), ["12  ", " 1;"]);
    }

    #[test]
    fn highlights_matches_across_spans() {
        let line = Line::from(vec![Span::from("12  "), Span::from("let fo"), Span::from("o = Foo;")]);
        let out = highlight_matches(line, "foo", true, 4);
        let pieces: Vec<(&str, bool)> =
            out.spans.iter().map(|s| (s.content.as_ref(), s.style.bg == Some(FIND_BG))).collect();
        assert_eq!(pieces, [("12  ", false), ("let ", false), ("fo", true), ("o", true), (" = ", false), ("Foo", true), (";", false)]);
    }
}
