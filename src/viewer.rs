//! Read-only view of a file (syntax highlighted) or of its git diff.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style, Stylize};

use crate::theme::{ADDED_BG, DIM, FIND_BG, GREEN, MARK_BG, RED, REMOVED_BG};
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
}

#[derive(Default)]
pub struct Viewer {
    pub source: Option<Source>,
    pub title: String,
    pub lines: Vec<Line<'static>>,
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
}

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
        let (path, content) = match source {
            Source::File(path) => (path, load_file(path)),
            Source::Diff { path, untracked } => (path, load_diff(root, path, *untracked)),
        };
        let name = path.strip_prefix(root).unwrap_or(path).display();
        self.title = match content.counts {
            Some((added, removed)) => format!("{name}  +{added} −{removed}"),
            None => name.to_string(),
        };
        self.lines = content.lines;
        self.gutter = content.gutter;
        self.changes = content.changes;
        self.change_at = self.change_at.filter(|&i| i < self.changes.len());
        self.scroll_by(0);
        self.update_find(false);
    }

    pub fn is_diff(&self) -> bool {
        matches!(self.source, Some(Source::Diff { .. }))
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
        if self.changes.is_empty() {
            return;
        }
        let count = self.changes.len() as isize;
        let i = match self.change_at {
            Some(i) => (i as isize + step).rem_euclid(count) as usize,
            None => self.changes.iter().position(|&line| line >= self.scroll).unwrap_or(0),
        };
        self.change_at = Some(i);
        self.goto = Some(self.changes[i]);
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
        let Some(find) = &mut self.find else { return };
        let ignore_case = find.ignore_case();
        let needle = fold(&find.query, ignore_case);
        find.matches = if needle.is_empty() {
            Vec::new()
        } else {
            let gutter = self.gutter;
            (0..self.lines.len()).filter(|&i| fold(&plain(&self.lines[i], gutter), ignore_case).contains(&needle)).collect()
        };
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

    /// Line `i` as drawn: the marked line gets a background, search matches
    /// are highlighted.
    pub fn decorated(&self, i: usize) -> Line<'static> {
        let mut line = self.lines[i].clone();
        if let Some(find) = self.find.as_ref().filter(|f| !f.query.is_empty()) {
            line = highlight_matches(line, &find.query, find.ignore_case(), self.gutter);
        }
        if self.mark == Some(i) {
            line = line.patch_style(Style::default().bg(MARK_BG));
        }
        line
    }

    pub fn scroll_by(&mut self, rows: isize) {
        let max = self.lines.len().saturating_sub(self.height.max(1));
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

/// The whole file with its changes inline: deleted lines in red where they
/// were, added lines in green, both syntax highlighted.
fn load_diff(root: &Path, path: &Path, untracked: bool) -> Content {
    let mut cmd = Command::new("git");
    // Enough context to get the whole file in one hunk.
    cmd.current_dir(root).args(["--no-optional-locks", "diff", "--no-color", "--no-ext-diff", "--unified=1000000"]);
    if untracked {
        cmd.args(["--no-index", "--", "/dev/null"]).arg(path);
    } else {
        cmd.args(["HEAD", "--"]).arg(path);
    }
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

    // (kind, line number: new for added and unchanged lines, old for removed, text)
    let mut rows: Vec<(Kind, usize, &str)> = Vec::new();
    let (mut old, mut new) = (0, 0);
    let mut in_hunk = false;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("@@ ") {
            if in_hunk {
                rows.push((Kind::Gap, 0, ""));
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
                rows.push((Kind::Added, new, &line[1..]));
                new += 1;
            }
            Some('-') => {
                rows.push((Kind::Removed, old, &line[1..]));
                old += 1;
            }
            Some('\\') => {} // "\ No newline at end of file"
            _ => {
                rows.push((Kind::Same, new, line.get(1..).unwrap_or("")));
                (old, new) = (old + 1, new + 1);
            }
        }
    }

    // One highlighter per side, so each follows its own version of the file.
    let (syntaxes, theme) = assets();
    let syntax = syntax_for(path);
    let (mut new_side, mut old_side) = (HighlightLines::new(syntax, theme), HighlightLines::new(syntax, theme));
    let width = rows.iter().map(|r| r.1).max().unwrap_or(0).to_string().len();
    let mut content = Content { gutter: width + 3, ..Content::default() };
    let (mut added, mut removed) = (0, 0);
    for (i, &(kind, number, text)) in rows.iter().enumerate() {
        if kind == Kind::Gap {
            content.lines.push(note("⋯"));
            continue;
        }
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
        let mut spans = vec![Span::styled(format!("{number:>width$} {marker} "), Style::default().fg(fg))];
        spans.extend(styled(ranges, &with_newline));
        let mut line = Line::from(spans);
        if let Some(bg) = bg {
            // Pad so the background reaches across the view.
            line.spans.push(Span::from(" ".repeat(ROW_WIDTH.saturating_sub(text.chars().count()))));
            line = line.style(Style::default().bg(bg));
        }
        content.lines.push(line);
    }
    content.counts = Some((added, removed));
    content
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
    fn highlights_matches_across_spans() {
        let line = Line::from(vec![Span::from("12  "), Span::from("let fo"), Span::from("o = Foo;")]);
        let out = highlight_matches(line, "foo", true, 4);
        let pieces: Vec<(&str, bool)> =
            out.spans.iter().map(|s| (s.content.as_ref(), s.style.bg == Some(FIND_BG))).collect();
        assert_eq!(pieces, [("12  ", false), ("let ", false), ("fo", true), ("o", true), (" = ", false), ("Foo", true), (";", false)]);
    }
}
