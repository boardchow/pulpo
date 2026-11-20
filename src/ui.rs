use std::ops::Range;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, Paragraph};

use crate::agents::State;
use crate::app::{App, Areas, FileAsk, Focus, Pick};
use crate::search::Mode;
use crate::review::{CURSOR, Shown};
use crate::theme;
use crate::tree::Row;

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.reorder();
    frame.render_widget(Block::new().style(Style::new().fg(theme::fg()).bg(theme::bg())), frame.area());
    let [main, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    app.areas = Areas::default();
    if app.no_projects {
        draw_empty(frame, main);
    } else if app.focus == Focus::Files {
        draw_files(frame, app, main);
    } else if app.focus == Focus::Review {
        draw_review(frame, app, main);
    } else {
        draw_center(frame, app, main);
    }
    draw_footer(frame, app, footer);
    draw_picker(frame, app, main);
    draw_repos(frame, app, main);
    draw_naming(frame, app, main);
    draw_search(frame, app, main);
    draw_file_input(frame, app, main);
    draw_question(frame, app, main);
    draw_help(frame, app);
}

/// Block octants (Unicode 16): 2×4 pixels per cell, so the pixels come out square.
const PULPO: [&str; 3] = [
    "𜺠𜷡𜷥██𜷤𜷞𜺣",
    "𜴡𜷤▆██▆𜷥𜴍",
    "𜴁𜴄🮂𜴁𜴄🮂𜴁𜴄",
];

const PULPO_QUADRANTS: [&str; 3] = ["▗▄████▄▖", "▝▙▄██▄▟▘", "▞▚▀▞▚▀▞▚"];

static OWN_GLYPHS: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "ghostty") || std::env::var("TERM").is_ok_and(|t| t == "xterm-kitty")
});

fn pulpo() -> Vec<String> {
    let art = if *OWN_GLYPHS { PULPO } else { PULPO_QUADRANTS };
    art.iter().map(|row| row.to_string()).collect()
}

fn framed(name: &str, focused: bool, rest: Vec<Span<'static>>) -> Block<'static> {
    Block::bordered().border_type(BorderType::Rounded).title(title(name, focused, rest)).border_style(border(focused)).style(Style::new().bg(theme::bg()))
}

fn draw_center(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Terminal;
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let ws = app.ws();
    let project = if ws.is_worktree() { format!(" {}", app.project_name(app.current)) } else { String::new() };
    let name = format!(" {}", ws.name);
    let name_width = (name.chars().count() + project.chars().count() + 1) as u16;
    app.areas.name = Rect { x: area.x + 1, y: area.y, width: name_width, height: 1 };
    let mut spans = vec![Span::from(name).fg(theme::fg()).bold(), Span::from(project).fg(theme::dim()), Span::from("  ")];
    let waiting: Vec<usize> = (0..app.workspaces.len())
        .filter(|&i| i != app.current && matches!(app.workspace_state(i), Some(State::Waiting | State::Done | State::Ready | State::Failed)))
        .collect();
    let waiting = match waiting[..] {
        [] => String::new(),
        [i] => format!(" ◆ {} waiting ", app.workspaces[i].name),
        _ => format!(" ◆ {} waiting ", waiting.len()),
    };
    let left = app.changes.as_ref().map_or(0, |c| c.left());
    let to_review = if left > 0 { format!(" {left} to review ") } else { String::new() };
    let to_review_width = to_review.chars().count() as u16;
    let waiting_width = waiting.chars().count() as u16;
    if waiting_width > 0 {
        let at = Rect { x: area.right().saturating_sub(waiting_width + 1), y: area.y, width: waiting_width, height: 1 };
        frame.render_widget(Line::from(waiting).fg(theme::magenta()).bold(), at);
        app.areas.waiting = at;
    }
    if to_review_width > 0 {
        let at = Rect { x: area.right().saturating_sub(waiting_width + to_review_width + 1), y: area.y, width: to_review_width, height: 1 };
        frame.render_widget(Line::from(to_review).fg(theme::on_accent()).bg(theme::accent()).bold(), at);
        app.areas.to_review = at;
    }
    let waiting_width = waiting_width + to_review_width;
    let mut x = area.x + 2 + name_width;
    let tabs: Vec<usize> = app.workspace_terms();
    let room = area.width.saturating_sub(2 + 1 + 3 + name_width + waiting_width) as usize;
    let tab_width = |n: usize, cap: usize| 1 + app.terms[tabs[n]].label().chars().count().min(cap) + 1 + 3;
    let cap = (0..=20).rev().find(|&cap| (0..tabs.len()).map(|n| tab_width(n, cap)).sum::<usize>() <= room).unwrap_or(0);
    for &i in &tabs {
        let term = &app.terms[i];
        let on = app.active == Some(term.id);
        let (icon, color) = state_icon(app.term_state(term));
        let label: String = term.label().chars().take(cap).collect();
        let text = if label.is_empty() { " ".to_string() } else { format!(" {label} ") };
        let width = text.chars().count() as u16 + 2;
        let style = if on { Style::new().fg(theme::fg()).bg(theme::hover()).bold() } else { Style::new().fg(theme::muted()) };
        spans.extend([Span::styled(text, style), Span::styled(format!("{icon} "), style.fg(color)), Span::from(" ")]);
        app.areas.tabs.push((Some(term.id), Rect { x, y: area.y, width, height: 1 }));
        x += width + 1;
    }
    spans.push(Span::from(" + ").fg(theme::muted()));
    app.areas.tabs.push((None, Rect { x, y: area.y, width: 3, height: 1 }));
    frame.render_widget(Line::from(spans), Rect { x: area.x + 1, y: area.y, width: area.width.saturating_sub(2 + waiting_width), height: 1 });

    app.set_term_size(inner.height, inner.width);
    app.areas.term = inner;
    let Some(id) = app.active else {
        if app.opening(&app.ws().root) {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
            frame.render_widget(Line::from(format!("Warming up{}", ".".repeat(1 + (now.as_millis() / 400) as usize % 3))).fg(theme::muted()), inner);
            return;
        }
        let p = app.keymap.label();
        let lines = vec![Line::from("No terminal in this workspace").bold(), Line::default(), Line::from(format!("{p} t opens one, or click +")).fg(theme::muted())];
        let spot = Rect { y: inner.y + inner.height / 3, height: 3.min(inner.height), ..inner };
        frame.render_widget(Paragraph::new(lines).centered(), spot);
        return;
    };
    let prefixed = app.prefixed;
    let Some(term) = app.terms.iter_mut().find(|t| t.id == id) else { return };
    term.resize(inner.height.max(1), inner.width.max(1));
    if let Some(cursor) = term.render(inner, frame.buffer_mut())
        && focused
        && !prefixed
    {
        frame.set_cursor_position(cursor);
    }
    underline_links(frame.buffer_mut(), inner);
    if let Some(selecting) = app.selecting.as_ref().filter(|s| s.id == id) {
        let (from, to) = if selecting.from <= selecting.to { (selecting.from, selecting.to) } else { (selecting.to, selecting.from) };
        for row in from.0..=to.0.min(inner.height.saturating_sub(1)) {
            let start = if row == from.0 { from.1 } else { 0 };
            let end = if row == to.0 { to.1 } else { inner.width.saturating_sub(1) };
            for col in start..=end {
                if let Some(cell) = frame.buffer_mut().cell_mut((inner.x + col, inner.y + row)) {
                    cell.set_bg(theme::text_selection());
                }
            }
        }
    }
}

fn underline_links(buf: &mut ratatui::buffer::Buffer, area: Rect) {
    for y in area.top()..area.bottom() {
        let row: String = (area.left()..area.right()).map(|x| buf.cell((x, y)).and_then(|c| c.symbol().chars().next()).unwrap_or(' ')).collect();
        let chars: Vec<char> = row.chars().collect();
        let mut from = 0;
        while let Some(found) = ["https://", "http://"].iter().filter_map(|scheme| row_find(&chars, scheme, from)).min() {
            let end = chars[found..].iter().position(|c| c.is_whitespace() || "\"'`<>()[]{}│".contains(*c)).map_or(chars.len(), |i| found + i);
            for x in found..end {
                if let Some(cell) = buf.cell_mut((area.x + x as u16, y)) {
                    cell.modifier.insert(Modifier::UNDERLINED);
                }
            }
            from = end;
        }
    }
}

fn row_find(chars: &[char], needle: &str, from: usize) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    (from..chars.len().saturating_sub(needle.len() - 1)).find(|&i| chars[i..i + needle.len()] == needle[..])
}

fn title(name: &str, focused: bool, rest: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::from(format!(" {name} ")).fg(if focused { theme::fg() } else { theme::muted() }).bold()];
    spans.extend(rest);
    Line::from(spans)
}

fn draw_review(frame: &mut Frame, app: &mut App, area: Rect) {
    app.areas.review = area;
    if app.editor.is_some() {
        return draw_editing(frame, app, area);
    }
    let [tree, file] = Layout::horizontal([Constraint::Percentage(30), Constraint::Fill(1)]).areas(area);
    draw_changes(frame, app, tree);
    draw_diff(frame, app, file);
}

fn draw_changes(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Review && !app.reading;
    let Some(changes) = &mut app.changes else { return };
    let count = Span::from(format!("{} ", changes.files.len())).fg(theme::blue());
    let block = framed("Review", focused, vec![count]);
    let tree = block.inner(area);
    frame.render_widget(block, area);
    app.areas.changes_tree = tree;
    let items: Vec<ListItem> = changes.rows.iter().map(|row| tree_item(row, changes.status_of(&row.path), changes.count(&row.path), changes.progress(&row.path))).collect();
    if items.is_empty() {
        frame.render_widget(Line::from(" no changes").fg(theme::dim()), tree);
    }
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(focused)), tree, &mut changes.state);
}

fn draw_files(frame: &mut Frame, app: &mut App, area: Rect) {
    let [tree, file] = Layout::horizontal([Constraint::Percentage(30), Constraint::Fill(1)]).areas(area);
    let in_tree = app.editor.is_none() && !app.reading;
    let block = framed("Files", in_tree, Vec::new());
    app.areas.files_tree = block.inner(tree);
    frame.render_widget(block, tree);
    let changes = app.changes.as_ref();
    let ws = &mut app.workspaces[app.current];
    let items: Vec<ListItem> = ws.tree.rows.iter().map(|row| tree_item(row, changes.and_then(|c| c.status_of(&row.path)), 0, (0, 0))).collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(in_tree)), app.areas.files_tree, &mut ws.files_state);

    let open = app.editor.as_ref().map(|e| e.path.clone()).or_else(|| app.viewer.source.as_ref().map(|crate::viewer::Source::File(path)| path.clone()));
    let root = &app.ws().root;
    let name = open.map_or("no file".into(), |path| path.strip_prefix(root).unwrap_or(&path).display().to_string());
    let rest = if app.editor.as_ref().is_some_and(|e| e.dirty) { vec![Span::from("● unsaved ").fg(theme::yellow())] } else { Vec::new() };
    let block = framed(&name, !in_tree, rest);
    let inner = block.inner(file);
    frame.render_widget(block, file);
    app.areas.files_file = inner;
    if app.editor.is_some() {
        draw_editor(frame, app, inner);
    } else if app.viewer.is_open() {
        draw_viewer(frame, app, inner);
    } else {
        frame.render_widget(Line::from(" select a file").fg(theme::dim()), inner);
    }
}

fn draw_viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    app.viewer.height = area.height as usize;
    if let Some(line) = app.viewer.goto.take() {
        app.viewer.scroll = line.saturating_sub(area.height as usize / 3);
    }
    app.viewer.scroll_by(0);
    if app.reading && app.viewer.mark.is_none() {
        app.viewer.mark = Some(app.viewer.scroll);
    }
    let (scroll, end) = (app.viewer.scroll, (app.viewer.scroll + area.height as usize).min(app.viewer.row_count()));
    let lines: Vec<Line> = (scroll..end).map(|i| app.viewer.decorated(i, app.reading)).collect();
    frame.render_widget(Paragraph::new(lines).style(Style::new().bg(theme::editor_bg())), area);
}

fn draw_search(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(search) = &mut app.search else { return };
    let (title, hint) = match search.mode {
        Mode::Text => (" Search inside files ", " tab file names · ⏎ open · esc close "),
        _ => (" Find a file ", " tab inside files · ⏎ open · esc close "),
    };
    let (popup, inner) = popup(frame, area, title, hint, app.message.as_deref(), search.rows.len().clamp(1, 14) + 2);
    let [input, _, results] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    (app.areas.popup, app.areas.popup_list) = (popup, results);
    let placeholder = match search.rows.len() {
        _ if search.busy => "searching…".to_string(),
        _ if search.mode == Mode::Text && search.query.chars().count() < 2 => "at least 2 characters".to_string(),
        _ if search.query.is_empty() => format!("{} files: type part of a name", search.file_count()),
        n => format!("{n} matches"),
    };
    frame.render_widget(field("", &search.query, &placeholder, true), input);
    let items: Vec<ListItem> = search
        .rows
        .iter()
        .map(|hit| match hit.line {
            Some(line) => ListItem::new(Line::from(vec![
                Span::from(hit.rel.clone()).fg(theme::muted()),
                Span::from(format!(":{}", line + 1)).fg(theme::yellow()),
                Span::from(format!("  {}", hit.text)),
            ])),
            None => ListItem::new(hit.rel.clone()),
        })
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), results, &mut search.state);
}

fn draw_file_input(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(input) = &app.file_input else { return };
    let (title, hint) = match &input.ask {
        FileAsk::New(dir) => {
            let dir = dir.strip_prefix(&app.ws().root).unwrap_or(dir).display().to_string();
            (if dir.is_empty() { " New file ".to_string() } else { format!(" New file in {dir}/ ") }, " end with / for a folder · ⏎ make it · esc close ")
        }
        FileAsk::Rename(_) => (" Rename ".to_string(), " ⏎ rename · esc close "),
    };
    let (popup, inner) = popup(frame, area, &title, hint, app.message.as_deref(), 1);
    frame.render_widget(field("", &input.text, "name", true), inner);
    app.areas.popup = popup;
}

fn draw_editing(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(editor) = &app.editor else { return };
    let name = editor.path.strip_prefix(&app.ws().root).unwrap_or(&editor.path).display().to_string();
    let rest = if editor.dirty { vec![Span::from("● unsaved ").fg(theme::yellow())] } else { Vec::new() };
    let block = framed(&name, app.focus == Focus::Review, rest).title_bottom(Line::from(" ⌃s save · esc back to the review ").fg(theme::dim()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    draw_editor(frame, app, inner);
}

fn tree_item(row: &Row, kind: Option<char>, comments: usize, (approved, all): (usize, usize)) -> ListItem<'static> {
    let [open, shut, file] = if *OWN_GLYPHS { ["\u{f07c} ", "\u{f07b} ", "\u{f15b} "] } else { ["▾ ", "▸ ", "  "] };
    let (icon, color) = match (row.is_dir, row.expanded) {
        (true, true) => (open, theme::blue()),
        (true, false) => (shut, theme::blue()),
        _ => (file, theme::fg()),
    };
    let style = kind.map_or_else(Style::default, |k| Style::default().fg(kind_color(k)));
    let mut spans = vec![Span::from(format!(" {}{icon}", "  ".repeat(row.depth))).fg(color), Span::styled(row.name.clone(), style)];
    if comments > 0 {
        spans.push(Span::from(format!(" ✎{comments}")).fg(theme::yellow()));
    }
    if all > 0 && approved == all {
        spans.push(Span::from(" ✓").fg(theme::green()));
    } else if approved > 0 {
        spans.push(Span::from(format!(" {approved}/{all}")).fg(theme::dim()));
    }
    ListItem::new(Line::from(spans))
}

fn draw_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Review && app.reading;
    let Some(changes) = &app.changes else { return };
    let mut rest = Vec::new();
    let name = match &changes.diff {
        Some((path, diff)) => {
            rest.push(Span::from(format!("+{} ", diff.added)).fg(theme::green()));
            rest.push(Span::from(format!("−{} ", diff.removed)).fg(theme::red()));
            let (approved, all) = changes.progress(&changes.root);
            if all > 0 {
                rest.push(Span::from(format!("· {approved} of {all} approved ")).fg(if approved == all { theme::green() } else { theme::muted() }));
            }
            path.strip_prefix(&changes.root).unwrap_or(path).display().to_string()
        }
        None => "Review".into(),
    };
    if !changes.comments.is_empty() {
        rest.push(Span::from(format!("· ✎{} ", changes.comments.len())).fg(theme::yellow()));
    }
    let block = Block::bordered().border_type(BorderType::Rounded).title(title(&name, focused, rest)).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let file = block.inner(area);
    frame.render_widget(block, area);
    let Some(changes) = &mut app.changes else { return };
    let [file, ruler] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(1)]).areas(file);
    app.areas.changes_file = file;
    app.areas.changes_ruler = ruler;
    changes.height = file.height as usize;
    changes.scroll_by(0);
    for (i, (color, in_view)) in changes.ruler(ruler.height as usize).into_iter().enumerate() {
        if let Some(cell) = frame.buffer_mut().cell_mut((ruler.x, ruler.y + i as u16)) {
            cell.set_symbol(if color.is_some() { "▐" } else { " " });
            cell.set_style(Style::new().fg(color.unwrap_or(theme::editor_bg())).bg(if in_view { theme::border() } else { theme::editor_bg() }));
        }
    }
    let bg = Style::new().bg(theme::editor_bg());
    frame.render_widget(Paragraph::new("").style(bg), file);
    changes.fit(file.width as usize, app.writing.as_ref().map(|w| (w, app.writing_at)));
    let Some((_, diff)) = &changes.diff else { return };
    let (first, last) = changes.selection();
    let mark = |mut line: Line<'static>, i: usize| {
        if let Some(gutter) = line.spans.first_mut().filter(|_| diff.kinds.get(i).is_some_and(|k| *k != crate::viewer::Kind::Gap)) {
            if changes.commented(i) {
                gutter.style = gutter.style.fg(theme::yellow()).add_modifier(Modifier::BOLD);
            }
            if focused && (first..=last).contains(&i) {
                gutter.style = gutter.style.bg(theme::selection());
            }
        }
        line
    };
    let shown = |i: usize, piece: usize| {
        let mut line = match changes.piece_start(i, piece) {
            Some(start) => crate::viewer::shift(diff.lines[i].clone(), diff.gutter, start),
            None => Line::from(String::new()).style(diff.lines[i].style),
        };
        if piece > 0 && let Some(gutter) = line.spans.first_mut() {
            gutter.content = " ".repeat(diff.gutter).into();
        }
        mark(line, i)
    };
    let indent = || Span::from(" ".repeat(diff.gutter.saturating_sub(2)));
    let note = |text: Vec<Span<'static>>| Line::from([vec![indent(), Span::from("▎ ").fg(theme::yellow())], text].concat());
    let section = changes.section();
    let mut in_section = false;
    for (y, item) in changes.shown.iter().enumerate() {
        let at = Rect { y: file.y + y as u16, height: 1, ..file };
        // A comment goes with the row above it.
        match *item {
            Shown::Row(row) | Shown::Wrap { row, .. } | Shown::Done(Range { start: row, .. }) => in_section = section.contains(&row),
            Shown::Fold(_) => in_section = false,
            Shown::Note(..) | Shown::Hint => {}
        }
        match *item {
            Shown::Row(row) | Shown::Wrap { row, .. } => {
                let piece = if let Shown::Wrap { piece, .. } = *item { piece } else { 0 };
                frame.render_widget(Paragraph::new(shown(row, piece)).style(bg), at);
            }
            Shown::Note(i, ref text) => {
                let text = match text.split_once(CURSOR).filter(|_| i.is_none()) {
                    Some((before, after)) => vec![Span::from(before.to_string()), Span::from("█").fg(theme::blue()), Span::from(after.to_string())],
                    None => vec![Span::from(text.clone())],
                };
                frame.render_widget(Paragraph::new(note(text)).style(Style::new().fg(theme::fg()).bg(theme::bg())), at);
            }
            Shown::Hint => {
                let hint = Span::from("⏎ keep, empty deletes · ⌥⏎ or ⌃j new line · esc cancel").fg(theme::muted());
                frame.render_widget(Paragraph::new(note(vec![hint])).style(Style::new().bg(theme::bg())), at);
            }
            Shown::Fold(ref rows) => {
                let (lines, scopes) = changes.fold_label(rows);
                frame.render_widget(Paragraph::new(fold_line(indent(), lines, scopes, file.width as usize)).style(Style::new().bg(theme::bg())), at);
            }
            Shown::Done(ref rows) => {
                let (commented, label) = changes.done_label(rows);
                let style = Style::new().fg(if commented { theme::yellow() } else { theme::green() });
                let style = if focused && rows.contains(&changes.cursor) { style.bg(theme::selection()) } else { style };
                let line = Line::from(vec![indent(), Span::styled(label, style)]);
                frame.render_widget(Paragraph::new(line).style(bg), at);
            }
        }
        if focused && in_section && let Some(cell) = frame.buffer_mut().cell_mut((at.x, at.y)) {
            cell.set_symbol("▌").set_fg(theme::accent());
        }
    }
}

fn fold_line(indent: Span<'static>, lines: String, scopes: Vec<String>, width: usize) -> Line<'static> {
    let mut spans = vec![indent, Span::from(lines).fg(theme::dim())];
    if scopes.is_empty() {
        return Line::from(spans);
    }
    let used = spans.iter().map(Span::width).sum::<usize>() + 3;
    let fits = |k: usize| used + if k > 0 { 4 } else { 0 } + Span::from(scopes[k..].join(" › ")).width() <= width;
    let skip = (0..scopes.len() - 1).find(|&k| fits(k)).unwrap_or(scopes.len() - 1);
    spans.push(Span::from("   "));
    if skip > 0 {
        spans.push(Span::from("… › ").fg(theme::dim()));
    }
    for (i, scope) in scopes[skip..].iter().enumerate() {
        if i > 0 {
            spans.push(Span::from(" › ").fg(theme::dim()));
        }
        spans.push(Span::from(scope.clone()).fg(if skip + i + 1 == scopes.len() { theme::fg() } else { theme::muted() }));
    }
    Line::from(spans)
}

fn draw_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus != Focus::Terminal && !app.prefixed;
    let Some(editor) = app.editor.as_mut() else { return };
    editor.view = (area.height as usize, area.width as usize);
    let rows = editor.rows(area.height as usize);
    let cursor = editor.cursor_cell();
    app.areas.editor = area;
    frame.render_widget(Paragraph::new(rows).style(Style::new().bg(theme::editor_bg())), area);
    if focused
        && let Some((row, col)) = cursor
        && row < area.height as usize
        && col < area.width as usize
    {
        frame.set_cursor_position((area.x + col as u16, area.y + row as u16));
    }
}

fn draw_footer(frame: &mut Frame, app: &mut App, area: Rect) {
    let input = |label: &str, text: &str, hint: &str| {
        Line::from(vec![
            Span::from(format!(" {label} ")).fg(theme::on_accent()).bg(theme::accent()).bold(),
            Span::from(" › ").fg(theme::blue()).bold(),
            Span::from(text.to_string()),
            Span::from("█").fg(theme::blue()),
            Span::from(format!("   {hint}")).fg(theme::muted()),
        ])
    };
    let line = if app.prefixed {
        Line::from(vec![
            Span::from(format!(" {} ", app.keymap.label())).fg(theme::on_accent()).bg(theme::accent()).bold(),
            Span::from("  ? keybinds").fg(theme::muted()),
        ])
    } else if let Some(editor) = app.editor.as_ref().filter(|e| e.find_typing && app.focus != Focus::Terminal) {
        input("find", editor.find.as_deref().unwrap_or_default(), "⏎ next · ⇧⏎ previous · esc done")
    } else if let Some(message) = app.message.as_ref().filter(|_| app.picker.is_none() && app.repos.is_none() && app.naming.is_none() && app.search.is_none() && app.file_input.is_none()) {
        Line::from(format!(" {message}")).fg(theme::yellow()).bold()
    } else if let Some(version) = app.release.as_ref().filter(|_| !app.no_projects) {
        Line::from(format!(" Pulpo {version} is out · {} u to update", app.keymap.label())).fg(theme::blue()).bold()
    } else {
        Line::default()
    };
    let used = line.width();
    frame.render_widget(line, area);
    if let Some(branch) = app.ws().status.branch.as_deref() {
        let branch = Line::from(format!("⎇ {branch} ")).fg(theme::muted()).right_aligned();
        if used + branch.width() + 2 <= area.width as usize {
            frame.render_widget(branch, area);
        }
    }
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let (width, height) = (width.min(area.width), height.min(area.height));
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 3, width, height }
}

fn veil(frame: &mut Frame) {
    for cell in &mut frame.buffer_mut().content {
        (cell.fg, cell.bg) = (theme::veiled(cell.fg, theme::dim()), theme::veiled(cell.bg, theme::bg()));
    }
}

fn draw_picker(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.picker.is_none() {
        return;
    }
    let picks = app.picks();
    let items: Vec<ListItem> = picks
        .iter()
        .map(|pick| match pick {
            Pick::Go(i) => {
                let w = &app.workspaces[*i];
                let mut spans = state_name(app.workspace_state(*i), &w.name, *i == app.current);
                let project = if w.is_worktree() { app.project_name(*i).to_string() } else { "project".to_string() };
                let room = (area.width.min(POPUP_WIDTH) as usize).saturating_sub(spans.iter().map(Span::width).sum::<usize>() + project.chars().count() + 6);
                spans.extend([Span::from(" ".repeat(room.max(1))), Span::from(project).fg(theme::dim())]);
                ListItem::new(Line::from(spans))
            }
            Pick::New => ListItem::new(Line::from(vec![Span::from(" + ").fg(theme::blue()).bold(), Span::from("new workspace").fg(theme::muted())])),
        })
        .collect();
    let (popup, inner) = popup(frame, area, "", " ⏎ go · ⌃x delete it · esc close ", app.message.as_deref(), items.len().clamp(1, 14) + 2);
    let [input, _, list] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    (app.areas.popup, app.areas.popup_list) = (popup, list);
    let Some(picker) = &mut app.picker else { return };
    frame.render_widget(field("", &picker.query, "filter…", true), input);
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), list, &mut picker.state);
}

fn popup(frame: &mut Frame, area: Rect, title: &str, hint: &str, message: Option<&str>, rows: usize) -> (Rect, Rect) {
    veil(frame);
    let popup = centered(area, POPUP_WIDTH.min(area.width.saturating_sub(2)), rows as u16 + 4);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .padding(ratatui::widgets::Padding::new(2, 2, 1, 1))
        .title(Line::from(title.to_string()).fg(theme::fg()).bold())
        .title_bottom(match message {
            Some(message) => Line::from(format!(" {message} ")).fg(theme::yellow()).bold(),
            None => Line::from(hint.to_string()).fg(theme::muted()),
        })
        .border_style(border(true))
        .style(Style::new().bg(theme::editor_bg()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    (popup, inner)
}

const POPUP_WIDTH: u16 = 64;

fn wrapped_lines(text: &str, width: usize) -> usize {
    let mut lines = 1;
    let mut used = 0;
    for word in text.split(' ').map(|w| w.chars().count()) {
        if used > 0 && used + 1 + word > width {
            (lines, used) = (lines + 1, word);
        } else {
            used += word + usize::from(used > 0);
        }
    }
    lines
}

fn field(label: &str, text: &str, placeholder: &str, typing: bool) -> Line<'static> {
    let mut spans = vec![Span::from(format!("{label}› ")).fg(theme::blue()).bold()];
    let cursor = Span::from(if typing { "█" } else { "" }).fg(theme::blue());
    if text.is_empty() {
        spans.extend([cursor, Span::from(placeholder.to_string()).fg(theme::dim())]);
    } else {
        spans.extend([Span::from(text.to_string()), cursor]);
    }
    Line::from(spans)
}

fn draw_naming(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(naming) = &app.naming else { return };
    let title = format!(" New workspace in {} ", naming.repo.file_name().unwrap_or_default().to_string_lossy());
    let (popup, inner) = popup(frame, area, &title, " tab branch · ⏎ make it · esc close ", app.message.as_deref(), 2);
    let [name, branch] = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    frame.render_widget(field("name   ", &naming.name, "like fix-login, or none to work in the project", !naming.on_branch), name);
    frame.render_widget(field("branch ", &naming.branch, "from the name", naming.on_branch), branch);
    app.areas.popup = popup;
}

fn draw_question(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some((question, _)) = &app.asking else { return };
    let width = POPUP_WIDTH.min(area.width.saturating_sub(2)).saturating_sub(8).max(1) as usize;
    let lines = wrapped_lines(question, width);
    let (popup, inner) = popup(frame, area, "", "", None, lines + 2);
    let [text, _, answers] = Layout::vertical([Constraint::Length(lines as u16), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    frame.render_widget(Paragraph::new(question.clone()).wrap(ratatui::widgets::Wrap { trim: true }).fg(theme::yellow()).bold(), text);
    frame.render_widget(
        Line::from(vec![Span::from(" y yes ").fg(theme::on_accent()).bg(theme::accent()).bold(), Span::from(" "), Span::from(" n no ").fg(theme::fg()).bg(theme::hover())]),
        answers,
    );
    app.areas.yes = Rect { x: answers.x, y: answers.y, width: 7, height: 1 };
    app.areas.popup = popup;
}

fn state_name(state: Option<State>, name: &str, current: bool) -> Vec<Span<'static>> {
    let (icon, color) = state.map_or((" ", theme::dim()), state_icon);
    let name = Span::from(name.to_string());
    let name = match state {
        Some(State::Waiting | State::Done | State::Ready | State::Failed) => name.fg(color).bold(),
        _ if current => name.fg(theme::fg()).bold(),
        _ => name.fg(theme::muted()),
    };
    vec![Span::from(format!(" {icon} ")).fg(color).bold(), name]
}

fn draw_empty(frame: &mut Frame, area: Rect) {
    let [_, logo, _, title, _, hint, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(PULPO.len() as u16),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(2),
    ])
    .areas(area);
    let lines: Vec<Line> = pulpo().into_iter().map(|row| Line::from(row).fg(theme::blue())).collect();
    frame.render_widget(Paragraph::new(lines).centered(), logo);
    frame.render_widget(Line::from("No workspaces").bold().centered(), title);
    frame.render_widget(Line::from(vec![Span::from("⏎ ").fg(theme::blue()).bold(), Span::from("new workspace").fg(theme::muted())]).centered(), hint);
}

fn draw_help(frame: &mut Frame, app: &mut App) {
    if !app.help {
        return;
    }
    veil(frame);
    let p = app.keymap.label();
    let prefixed = |keys: &str| format!("{p} {keys}");
    let twice = app.keymap.prefix.iter().find(|k| k.mods.contains(ratatui::crossterm::event::KeyModifiers::CONTROL)).map(|k| format!("{k} {k}"));
    let sections: Vec<(&str, Vec<(String, &str)>)> = vec![
        (
            "Workspaces",
            vec![
                (prefixed("w"), "go to one: type to filter"),
                (prefixed("n"), "new workspace: pick its repository, then name it"),
                (prefixed("X"), "delete this workspace, after asking"),
                (prefixed("1-9"), "the first to ninth workspace in the list"),
                (prefixed("a"), "the next one waiting for you"),
            ],
        ),
        (
            "Terminals",
            vec![(prefixed("t"), "new terminal"), (prefixed("x"), "close the terminal"), (prefixed("[ ]"), "previous, next terminal")]
                .into_iter()
                .chain(twice.map(|keys| (keys, "the key itself, to the program")))
                .collect(),
        ),
        (
            "Review",
            vec![
                (prefixed("r"), "open or close the review, full screen"),
                ("⏎ tab esc".into(), "into the file, back to the files, close"),
                ("n p".into(), "next, previous change"),
                ("a c".into(), "approve the change, or take it back; comment on the lines"),
                ("⇧↑↓ J K".into(), "select lines, also with ⇧click or drag"),
                (prefixed("n d"), "comment on the file, delete a comment"),
                (prefixed("e"), "edit the file at that line: ⌃s saves, esc goes back"),
            ],
        ),
        (
            "Files",
            vec![
                (prefixed("f"), "open or close Files, full screen"),
                (prefixed("/"), "find a file by name, tab for inside files"),
                (prefixed("n"), "new file in the selected folder (ending with / for a folder)"),
                (prefixed("e r d"), "edit, rename, delete"),
                ("↑↓ ←→ ⏎ tab".into(), "move, fold, read the file, back to the tree"),
            ],
        ),
        (
            "Mouse",
            vec![
                ("click".into(), "go there; the workspace's name opens the list"),
                ("drag".into(), "select to copy in a terminal"),
                ("ctrl-click".into(), "open a link, or a path:line"),
            ],
        ),
        (
            "Quit",
            vec![
                (prefixed("q"), "quit, terminals keep running"),
                (prefixed("Q"), "quit and stop every terminal"),
                (prefixed("u"), "update to the new release, terminals keep running"),
            ],
        ),
    ];
    let mut lines = Vec::new();
    for (title, keys) in sections {
        lines.push(Line::from(format!(" {title}")).bold());
        for (key, text) in keys {
            lines.push(Line::from(vec![Span::from(format!("   {key:<12}")).fg(theme::blue()).bold(), Span::from(text).fg(theme::fg())]));
        }
        lines.push(Line::default());
    }
    lines.pop();
    let area = frame.area();
    let popup = centered(area, 80, lines.len() as u16 + 2);
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(" Keybinds ")
        .title_style(Style::new().fg(theme::fg()).bold())
        .title_bottom(Line::from(" any key closes ").fg(theme::dim()))
        .border_style(border(true))
        .style(Style::new().bg(theme::bg()));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_repos(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(search) = &mut app.repos else { return };
    let hint = if search.busy { " looking for repositories… " } else { " ⏎ pick it · esc close " };
    let (popup, inner) = popup(frame, area, " New workspace ", hint, app.message.as_deref(), search.rows.len().clamp(1, 14) + 2);
    let [input, _, results] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    (app.areas.popup, app.areas.popup_list) = (popup, results);
    frame.render_widget(field("", &search.query, "its repository, or a path (~/…)", true), input);
    let items: Vec<ListItem> = search
        .rows
        .iter()
        .map(|hit| {
            let new = hit.path.is_absolute() && !hit.path.exists();
            ListItem::new(Line::from(vec![Span::from(format!(" {}", hit.rel)), Span::from(if new { "  new folder" } else { "" }).fg(theme::muted())]))
        })
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), results, &mut search.state);
}

fn state_icon(state: State) -> (&'static str, Color) {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    match state {
        State::Running => (SPINNER[(now.as_millis() / 100) as usize % SPINNER.len()], theme::yellow()),
        State::Waiting | State::Done => ("◆", theme::magenta()),
        State::Ready => ("✓", theme::green()),
        State::Failed => ("✗", theme::red()),
        State::Idle => ("○", theme::dim()),
    }
}

fn border(focused: bool) -> Style {
    Style::default().fg(if focused { theme::accent() } else { theme::border() })
}

fn highlight(focused: bool) -> Style {
    if focused {
        Style::default().fg(theme::fg()).bg(theme::selection()).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(theme::hover())
    }
}

fn kind_color(kind: char) -> Color {
    match kind {
        'A' | '?' => theme::green(),
        'D' => theme::red(),
        'R' | 'C' => theme::sky(),
        'U' => theme::yellow(),
        _ => theme::blue(),
    }
}

#[cfg(test)]
mod tests {
    use super::wrapped_lines;

    #[test]
    fn counts_lines_wrapped_by_words() {
        assert_eq!(wrapped_lines("delete it?", 20), 1);
        assert_eq!(wrapped_lines("aaaa bbbb cccc", 9), 2);
        assert_eq!(wrapped_lines("aaaa bbbb cccc", 8), 3);
    }
}
