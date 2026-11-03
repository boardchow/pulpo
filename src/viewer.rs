//! Read-only view of a file (syntax highlighted), and the diffs Changes
//! shows: the whole file, its changes in place.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style, Stylize};

use crate::theme;
use ratatui::text::{Line, Span};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

pub(crate) const MAX_BYTES: usize = 2 * 1024 * 1024;
pub(crate) const MAX_HIGHLIGHTED_LINES: usize = 10_000;

#[derive(Clone, PartialEq)]
pub enum Source {
    File(PathBuf),
}

#[derive(Default)]
pub struct Viewer {
    pub source: Option<Source>,
    lines: Vec<Line<'static>>,
    pub scroll: usize,
    /// Visible rows, set when drawing.
    pub height: usize,
    /// A highlighted line (a search match), and a line to bring into view
    /// on the next draw.
    pub mark: Option<usize>,
    pub goto: Option<usize>,
    /// Width of the line-number column.
    gutter: usize,
    /// Columns of code scrolled off to the left (line numbers stay), and the
    /// longest line, which bounds it.
    hscroll: usize,
    widest: usize,
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
            self.hscroll = 0;
            self.reload(root);
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

    /// Re-read the file, keeping the scroll position (files change under
    /// us while agents work).
    pub fn reload(&mut self, _root: &Path) {
        let Some(Source::File(path)) = &self.source else { return };
        let content = load_file(path);
        self.lines = content.lines;
        self.gutter = content.gutter;
        let gutter = self.gutter;
        self.widest = self.lines.iter().map(|l| plain(l, gutter).trim_end().chars().count()).max().unwrap_or(0);
        self.hscroll = self.hscroll.min(self.widest);
        self.scroll_by(0);
    }

    pub fn row_count(&self) -> usize {
        self.lines.len()
    }

    /// Row `i` as drawn: scrolled sideways, the marked row on a background.
    pub fn decorated(&self, i: usize) -> Line<'static> {
        let line = shift(self.lines[i].clone(), self.gutter, self.hscroll);
        if self.mark == Some(i) { line.patch_style(Style::default().bg(theme::mark_bg())) } else { line }
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

/// What the viewer shows for a file, or Changes for a diff.
#[derive(Default)]
struct Content {
    lines: Vec<Line<'static>>,
    /// Width of the line-number column.
    gutter: usize,
    /// First line of each block of changes (diffs only).
    changes: Vec<usize>,
    /// Lines added and removed, and what each line is (diffs only).
    counts: Option<(usize, usize)>,
    kinds: Vec<Kind>,
}

impl Content {
    fn note(text: &str) -> Self {
        Content { lines: vec![note(text)], ..Content::default() }
    }
}

fn load_file(path: &Path) -> Content {
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES as u64) {
        return Content::note("file too large to preview");
    }
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
    let (syntaxes, colors) = assets();
    let mut highlighter = HighlightLines::new(syntax_for(path), &colors);
    let width = text.lines().count().to_string().len();
    let lines = LinesWithEndings::from(&text)
        .enumerate()
        .map(|(i, line)| {
            let ranges = if i < MAX_HIGHLIGHTED_LINES { highlighter.highlight_line(line, syntaxes).ok() } else { None };
            let mut spans = vec![Span::from(format!("{:>width$}  ", i + 1)).fg(theme::dim())];
            spans.extend(styled(ranges, line));
            Line::from(spans)
        })
        .collect();
    Content { lines, gutter: width + 2, ..Content::default() }
}

/// What a line of a diff is.
#[derive(Clone, Copy, PartialEq, Default, Debug)]
pub enum Kind {
    #[default]
    Same,
    Added,
    Removed,
    /// Unchanged lines git left out.
    Gap,
}

/// One file's changes, against `base`: the whole file, its lines
/// (inline, highlighted, a note when there's nothing to show), where each
/// change starts, and lines added and removed.
pub struct ReviewDiff {
    pub lines: Vec<Line<'static>>,
    pub changes: Vec<usize>,
    pub added: usize,
    pub removed: usize,
    /// Width of the line numbers and markers, and of the longest line after.
    pub gutter: usize,
    pub widest: usize,
    /// What each line is, for the marks on the right.
    pub kinds: Vec<Kind>,
}

pub(crate) fn review_diff(root: &Path, base: &str, path: &Path, untracked: bool) -> ReviewDiff {
    let mut cmd = Command::new("git");
    cmd.current_dir(root).args(["--no-optional-locks", "diff", "--no-color", "--no-ext-diff", "--no-renames", "--unified=1000000"]);
    if untracked {
        cmd.args(["--no-index", "--", "/dev/null"]).arg(path);
    } else {
        cmd.arg(base).arg("--").arg(path);
    }
    let text = cmd.output().map(|out| String::from_utf8_lossy(&out.stdout).into_owned()).unwrap_or_default();
    let content = diff_content(&text, path);
    let (added, removed) = content.counts.unwrap_or_default();
    let widest = content.lines.iter().map(|l| plain(l, content.gutter).trim_end().chars().count()).max().unwrap_or(0);
    ReviewDiff { gutter: content.gutter, widest, lines: content.lines, changes: content.changes, added, removed, kinds: content.kinds }
}

/// Lines from a diff of `path`: inline, both sides highlighted.
fn diff_content(text: &str, path: &Path) -> Content {
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
    let (syntaxes, colors) = assets();
    let syntax = syntax_for(path);
    let (mut new_side, mut old_side) = (HighlightLines::new(syntax, &colors), HighlightLines::new(syntax, &colors));
    let width = rows.iter().map(|r| r.1.max(r.2)).max().unwrap_or(0).to_string().len();
    let mut content = Content { gutter: width + 3, ..Content::default() };
    let (mut added, mut removed) = (0, 0);
    for (i, &(kind, old_number, new_number, text)) in rows.iter().enumerate() {
        if kind == Kind::Gap {
            content.lines.push(note("⋯"));
            content.kinds.push(Kind::Gap);
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
            Kind::Added => ('+', theme::green(), Some(theme::added_bg())),
            Kind::Removed => ('-', theme::red(), Some(theme::removed_bg())),
            _ => (' ', theme::dim(), None),
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
        let mut spans = vec![Span::styled(format!("{number:>width$} {marker} "), Style::default().fg(fg))];
        spans.extend(code);
        let mut line = Line::from(spans);
        if let Some(bg) = bg {
            // Pad so the background reaches across the view.
            line.spans.push(Span::from(" ".repeat(ROW_WIDTH.saturating_sub(text.chars().count()))));
            line = line.style(Style::default().bg(bg));
        }
        content.lines.push(line);
        content.kinds.push(kind);
    }
    content.counts = Some((added, removed));
    content
}

/// Drop `n` characters of code after the first `gutter` ones (the line
/// numbers), for scrolling sideways.
pub(crate) fn shift(line: Line<'static>, gutter: usize, n: usize) -> Line<'static> {
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

pub(crate) fn syntax_for(path: &Path) -> &'static SyntaxReference {
    let (syntaxes, _) = assets();
    syntaxes
        .find_syntax_for_file(path)
        .ok()
        .flatten()
        .or_else(|| syntaxes.find_syntax_by_extension(&path.extension()?.to_string_lossy()))
        .unwrap_or_else(|| syntaxes.find_syntax_plain_text())
}

/// Highlighted pieces as spans, or the plain line if highlighting failed.
pub(crate) fn styled(ranges: Option<Vec<(syntect::highlighting::Style, &str)>>, line: &str) -> Vec<Span<'static>> {
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

/// Split the line's spans so occurrences of `query` (after `skip`
/// characters) stand out.
pub(crate) fn highlight_matches(line: Line<'static>, query: &str, ignore_case: bool, skip: usize) -> Line<'static> {
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

    let found = Style::default().bg(theme::find_bg());
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
    Line::from(format!(" {text}")).fg(theme::dim()).italic()
}

fn clean(text: &str) -> String {
    text.trim_end_matches(['\n', '\r']).replace('\t', "    ")
}

/// Load the syntax definitions in the background so the first file opens fast.
pub fn warm_up() {
    std::thread::spawn(|| {
        assets();
    });
}

/// The syntaxes, and the colors of the theme on screen.
pub(crate) fn assets() -> (&'static SyntaxSet, std::sync::Arc<Theme>) {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    (SYNTAXES.get_or_init(two_face::syntax::extra_newlines), crate::theme::syntax())
}

fn color(c: syntect::highlighting::Color) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lays_out_a_diff_inline_with_its_changes() {
        let diff = "diff --git a/a.rs b/a.rs\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";
        let content = diff_content(diff, Path::new("a.rs"));
        assert_eq!(content.counts, Some((1, 1)));
        assert_eq!(content.changes, [1]);
        assert_eq!(content.kinds, [Kind::Same, Kind::Removed, Kind::Added, Kind::Same]);
        let text: Vec<String> = content.lines.iter().map(|l| plain(l, 0).trim_end().to_string()).collect();
        assert_eq!(text, ["1   one", "2 - two", "2 + TWO", "3   three"]);
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
            out.spans.iter().map(|s| (s.content.as_ref(), s.style.bg == Some(theme::find_bg()))).collect();
        assert_eq!(pieces, [("12  ", false), ("let ", false), ("fo", true), ("o", true), (" = ", false), ("Foo", true), (";", false)]);
    }
}
