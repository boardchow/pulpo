use std::path::PathBuf;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph};

use crate::agents::State;
use crate::app::{App, Areas, Ask, Focus};
use crate::theme;
use crate::search::Mode;
use crate::tree::Row;

pub fn draw(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Block::new().style(Style::new().fg(theme::fg()).bg(theme::bg())), frame.area());
    let [main, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let left = if app.left_hidden { 0 } else { app.left_width.unwrap_or((main.width * 18 / 100).clamp(26, 36)).clamp(20.min(main.width / 3), main.width / 3) };
    let rest = main.width - left;
    let least = 24.min(rest);
    let right = if app.right_hidden { 0 } else { app.right_width.unwrap_or(rest * 42 / 100).clamp(least, rest.saturating_sub(30).max(least)) };
    let expanded = match app.focus {
        Focus::Files => app.files_expanded || app.editor.is_some(),
        Focus::Changes => app.changes_expanded,
        _ => false,
    };
    let (center, right) = if expanded { (0, main.width - left) } else { (main.width - left - right, right) };
    let [left_area, center_area, right_area] = Layout::horizontal([Constraint::Length(left), Constraint::Length(center), Constraint::Length(right)]).areas(main);
    let border_at = |area: Rect| Rect { x: area.x.saturating_sub(1), width: 2, ..area };
    app.areas = Areas {
        width: main.width,
        left_divider: if left > 0 { border_at(if center > 0 { center_area } else { right_area }) } else { Rect::default() },
        divider: if right > 0 && center > 0 { border_at(right_area) } else { Rect::default() },
        ..Areas::default()
    };
    if left > 0 {
        draw_left(frame, app, left_area);
    }
    if center > 0 {
        draw_center(frame, app, center_area);
    }
    if right > 0 {
        draw_right(frame, app, right_area);
    }
    draw_footer(frame, app, footer);
    draw_search(frame, app);
    draw_repos(frame, app);
    draw_help(frame, app);
}

/// Block octants (Unicode 16): 2×4 pixels per cell, so the pixels come out square.
const PULPO: [&str; 3] = [
    "𜺠𜷡𜷥██𜷤𜷞𜺣",
    "𜴡𜷤▆██▆𜷥𜴍",
    "𜴁𜴄🮂𜴁𜴄🮂𜴁𜴄",
];

fn pulpo(wiggle: bool) -> Vec<String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let mut rows: Vec<String> = PULPO.iter().map(|row| row.to_string()).collect();
    if wiggle && (now.as_millis() / 300) % 2 == 1 {
        let mut legs: Vec<char> = rows[2].chars().collect();
        legs.rotate_right(1);
        rows[2] = legs.into_iter().collect();
    }
    rows
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let art_width = PULPO[0].chars().count() as u16;
    let [_, art, text] = Layout::horizontal([Constraint::Length(0), Constraint::Length(art_width), Constraint::Fill(1)]).spacing(2).areas(area);
    let working = app.terms.iter().any(|term| app.term_state(term) == State::Running);
    let lines: Vec<Line> = pulpo(working).into_iter().map(|row| Line::from(row).fg(theme::blue())).collect();
    frame.render_widget(Paragraph::new(lines), art);

    let [_, title, _] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Length(1)]).areas(text);
    let name = Line::from(vec![Span::from("Pulpo").fg(theme::fg()).bold(), Span::from(concat!(" v", env!("CARGO_PKG_VERSION"))).fg(theme::muted())]);
    frame.render_widget(name, title);
}

fn panel(frame: &mut Frame, block: Block<'static>, area: Rect, focused: bool, items: Vec<ListItem>, state: &mut ListState) -> Rect {
    let body = block.inner(area);
    frame.render_widget(block, area);
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(focused)), body, state);
    body
}

fn framed(name: &str, focused: bool) -> Block<'static> {
    Block::bordered().border_type(BorderType::Rounded).title(title(name, focused, Vec::new())).border_style(border(focused)).style(Style::new().bg(theme::bg()))
}

fn draw_left(frame: &mut Frame, app: &mut App, area: Rect) {
    let rows = |n: usize| n.max(1) as u16 + 2;
    let [_, header, _, workspaces, sessions, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(PULPO.len() as u16),
        Constraint::Length(1),
        Constraint::Length(rows(app.repos().len())),
        Constraint::Length(rows(app.sessions().len())),
        Constraint::Fill(1),
    ])
    .areas(area);
    draw_header(frame, app, header);
    let focused = app.focus == Focus::Workspaces;
    let block = framed("Workspaces", focused);
    let rows = block.inner(workspaces);
    frame.render_widget(block, workspaces);

    let current_repo = app.ws().repo.clone();
    let items: Vec<ListItem> = app
        .repos()
        .into_iter()
        .map(|i| {
            let w = &app.workspaces[i];
            let current = w.repo == current_repo;
            let mut spans = vec![Span::from(if current { " ● " } else { "   " }).fg(theme::accent())];
            let name = Span::from(w.name.clone());
            spans.push(if current { name.bold() } else { name });
            let states = app.workspaces.iter().enumerate().filter(|(_, s)| s.repo == w.repo).filter_map(|(s, _)| app.session_state(s));
            if let Some(state @ (State::Running | State::Waiting | State::Done)) = states.max_by_key(|state| state.urgency()) {
                let (icon, color) = state_icon(state);
                spans.push(Span::from(format!(" {icon}")).fg(color).bold());
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(focused)), rows, &mut app.workspaces_state);
    app.areas.workspaces = rows;

    let items: Vec<ListItem> = app
        .sessions()
        .into_iter()
        .map(|i| {
            let w = &app.workspaces[i];
            let state = app.session_state(i);
            let (icon, color) = state.map_or((" ", theme::dim()), state_icon);
            let name = Span::from(app.session_name(i).to_string());
            let name = match state {
                Some(State::Waiting | State::Done) => name.fg(color).bold(),
                _ if i == app.current => name.fg(theme::fg()).bold(),
                _ => name.fg(theme::muted()),
            };
            let mut spans = vec![Span::from(format!(" {icon} ")).fg(color).bold(), name];
            if !w.status.changes.is_empty() {
                spans.push(Span::from(format!(" ±{}", w.status.changes.len())).fg(theme::blue()));
            }
            if app.setup_failed.contains_key(&w.root) {
                spans.push(Span::from(" ✗ setup failed").fg(theme::red()));
            } else if app.setting_up.contains_key(&w.root) {
                spans.push(Span::from(format!(" {} setting up", state_icon(State::Running).0)).fg(theme::yellow()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let focused = app.focus == Focus::Sessions;
    app.areas.sessions = panel(frame, framed("Sessions", focused), sessions, focused, items, &mut app.sessions_state);
}

fn draw_center(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Terminal;
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = vec![Span::from(" ")];
    let mut x = area.x + 2;
    let tabs: Vec<usize> = app.session_terms();
    let room = area.width.saturating_sub(2 + 1 + 3) as usize;
    let tab_width = |n: usize, cap: usize| format!(" {} ", n + 1).chars().count() + app.terms[tabs[n]].label().chars().count().min(cap) + 1 + 3;
    let cap = (0..=20).rev().find(|&cap| (0..tabs.len()).map(|n| tab_width(n, cap)).sum::<usize>() <= room).unwrap_or(0);
    for (n, &i) in tabs.iter().enumerate() {
        let term = &app.terms[i];
        let on = app.active == Some(term.id);
        let (icon, color) = state_icon(app.term_state(term));
        let label: String = term.label().chars().take(cap).collect();
        let label = if label.is_empty() { String::new() } else { format!("{label} ") };
        let text = format!(" {} {label}", n + 1);
        let width = text.chars().count() as u16 + 2;
        let style = if on { Style::new().fg(theme::fg()).bg(theme::hover()).bold() } else { Style::new().fg(theme::muted()) };
        spans.extend([Span::styled(text, style), Span::styled(format!("{icon} "), style.fg(color)), Span::from(" ")]);
        app.areas.tabs.push((Some(term.id), Rect { x, y: area.y, width, height: 1 }));
        x += width + 1;
    }
    spans.push(Span::from(" + ").fg(theme::muted()));
    app.areas.tabs.push((None, Rect { x, y: area.y, width: 3, height: 1 }));
    frame.render_widget(Line::from(spans), Rect { x: area.x + 1, y: area.y, width: area.width.saturating_sub(2), height: 1 });

    app.set_term_size(inner.height, inner.width);
    app.areas.term = inner;
    let Some(id) = app.active else {
        let p = app.keymap.label();
        let lines = vec![Line::from("No terminal in this session").bold(), Line::default(), Line::from(format!("{p} t opens one, or click +")).fg(theme::muted())];
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

fn draw_right(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.focus == Focus::Files && (app.files_expanded || app.editor.is_some()) {
        draw_files(frame, app, area);
    } else if app.focus == Focus::Changes && app.changes_expanded {
        draw_changes(frame, app, area);
    } else {
        let [files, changes] = Layout::vertical([Constraint::Percentage(55), Constraint::Fill(1)]).areas(area);
        draw_files(frame, app, files);
        draw_changes(frame, app, changes);
    }
}

fn split_panel(frame: &mut Frame, app: &mut App, area: Rect, title: Line<'static>, focused: bool) -> (Rect, Rect) {
    let block = Block::bordered().border_type(BorderType::Rounded).title(title).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let fallback = (inner.width / 4).clamp(16, 32);
    let width = app.tree_width.unwrap_or(fallback).clamp(8.min(inner.width / 2), inner.width.saturating_sub(20).max(8));
    let [tree, line, file] = Layout::horizontal([Constraint::Length(width), Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    app.areas.tree_dividers.push((line, tree.x));
    for y in line.top()..line.bottom() {
        if let Some(cell) = frame.buffer_mut().cell_mut((line.x, y)) {
            cell.set_symbol("│").set_fg(theme::border());
        }
    }
    (tree, file)
}

fn title(name: &str, focused: bool, rest: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::from(format!(" {name} ")).fg(if focused { theme::fg() } else { theme::muted() }).bold()];
    spans.extend(rest);
    Line::from(spans)
}

fn draw_files(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Files;
    let root = app.ws().root.clone();
    let mut rest = Vec::new();
    let open = app.editor.as_ref().map(|e| e.path.clone()).or_else(|| match &app.viewer.source {
        Some(crate::viewer::Source::File(path)) => Some(path.clone()),
        _ => None,
    });
    if let Some(path) = open {
        rest.push(Span::from(format!("{} ", path.strip_prefix(&root).unwrap_or(&path).display())).fg(theme::muted()));
    }
    if let Some(editor) = &app.editor {
        if editor.dirty {
            rest.push(Span::from("● unsaved ").fg(theme::yellow()));
        }
    }
    let (tree, file) = split_panel(frame, app, area, title("Files", focused, rest), focused);
    app.areas.files_tree = tree;
    app.areas.files_file = file;

    let in_tree = focused && app.editor.is_none() && !app.reading;
    let ws = &mut app.workspaces[app.current];
    let changed: Vec<(PathBuf, char)> = ws.status.changes.iter().map(|c| (ws.root.join(&c.path), c.kind())).collect();
    frame.render_stateful_widget(List::new(file_items(&ws.tree.rows, &changed)).highlight_style(highlight(in_tree)), tree, &mut ws.files_state);

    if app.editor.is_some() {
        draw_editor(frame, app, file);
    } else if app.viewer.is_open() {
        draw_viewer(frame, app, file);
    } else {
        frame.render_widget(Line::from(" select a file").fg(theme::dim()), file);
    }
}

fn file_items(rows: &[Row], changed: &[(PathBuf, char)]) -> Vec<ListItem<'static>> {
    rows.iter()
        .map(|row| {
            let kind = changed.iter().find(|(p, _)| if row.is_dir { p.starts_with(&row.path) } else { *p == row.path }).map(|(_, k)| *k);
            tree_item(row, kind)
        })
        .collect()
}

fn tree_item(row: &Row, kind: Option<char>) -> ListItem<'static> {
    let icon = match (row.is_dir, row.expanded) {
        (true, true) => "▾ ",
        (true, false) => "▸ ",
        _ => "  ",
    };
    let style = kind.map_or_else(Style::default, |k| Style::default().fg(kind_color(k)));
    ListItem::new(Line::from(vec![Span::from(format!(" {}{icon}", "  ".repeat(row.depth))).fg(theme::dim()), Span::styled(row.name.clone(), style)]))
}

fn draw_changes(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Changes;
    let Some(changes) = &app.changes else { return };
    let mut rest = vec![Span::from(format!("{} ", changes.files.len())).fg(theme::blue()), Span::from(format!("{} ", changes.since)).fg(theme::muted())];
    if let Some((path, diff)) = &changes.diff {
        rest.push(Span::from(format!("· {} ", path.strip_prefix(&changes.root).unwrap_or(path).display())).fg(theme::muted()));
        rest.push(Span::from(format!("+{} ", diff.added)).fg(theme::green()));
        rest.push(Span::from(format!("−{} ", diff.removed)).fg(theme::red()));
    }
    let (tree, file) = split_panel(frame, app, area, title("Changes", focused, rest), focused);
    app.areas.changes_tree = tree;
    app.areas.changes_file = file;
    let in_tree = focused && !app.reading;
    let Some(changes) = &mut app.changes else { return };
    let items: Vec<ListItem> = changes.rows.iter().map(|row| tree_item(row, changes.status_of(&row.path))).collect();
    if items.is_empty() {
        frame.render_widget(Line::from(" nothing changed").fg(theme::dim()), tree);
    }
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(in_tree)), tree, &mut changes.state);
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
    let (scroll, hscroll) = (changes.scroll, changes.hscroll);
    let lines: Vec<Line> = changes.diff.as_ref().map_or_else(Vec::new, |(_, diff)| {
        diff.lines.iter().skip(scroll).take(file.height as usize).map(|l| crate::viewer::shift(l.clone(), diff.gutter, hscroll)).collect()
    });
    frame.render_widget(Paragraph::new(lines).style(Style::new().bg(theme::editor_bg())), file);
}

fn draw_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Files && !app.prefixed;
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

fn draw_viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    app.viewer.height = area.height as usize;
    if let Some(line) = app.viewer.goto.take() {
        app.viewer.scroll = line.saturating_sub(area.height as usize / 3);
    }
    app.viewer.scroll_by(0);
    let (scroll, end) = (app.viewer.scroll, (app.viewer.scroll + area.height as usize).min(app.viewer.row_count()));
    let lines: Vec<Line> = (scroll..end).map(|i| app.viewer.decorated(i)).collect();
    frame.render_widget(Paragraph::new(lines).style(Style::new().bg(theme::editor_bg())), area);
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
    } else if let Some(prompt) = &app.prompt {
        match &prompt.ask {
            Ask::Session => input("new session", &prompt.text, "a worktree on its own branch, from the remote's main · ⏎ make it · esc cancel"),
            Ask::File(dir) => {
                let dir = dir.strip_prefix(&app.ws().root).unwrap_or(dir).display().to_string();
                let at = if dir.is_empty() { String::new() } else { format!("in {dir}/ · ") };
                input("new file", &prompt.text, &format!("{at}end with / for a folder · ⏎ make it · esc cancel"))
            }
            Ask::Rename(_) => input("rename", &prompt.text, "⏎ rename · esc cancel"),
        }
    } else if let Some((question, _)) = &app.asking {
        let text = format!(" {question}  ");
        let at = area.x + text.chars().count() as u16;
        app.areas.yes = Rect { x: at, y: area.y, width: 7, height: 1 };
        Line::from(vec![
            Span::from(text).fg(theme::yellow()).bold(),
            Span::from(" y yes ").fg(theme::on_accent()).bg(theme::accent()).bold(),
            Span::from(" "),
            Span::from(" n no ").fg(theme::fg()).bg(theme::hover()),
        ])
    } else if let Some(editor) = app.editor.as_ref().filter(|e| e.find_typing && app.focus == Focus::Files) {
        input("find", editor.find.as_deref().unwrap_or_default(), "⏎ next · ⇧⏎ previous · esc done")
    } else if let Some(message) = &app.message {
        Line::from(format!(" {message}")).fg(theme::yellow()).bold()
    } else {
        Line::default()
    };
    frame.render_widget(line, area);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let (width, height) = (width.min(area.width), height.min(area.height));
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 3, width, height }
}

fn draw_search(frame: &mut Frame, app: &mut App) {
    let Some(search) = &mut app.search else { return };
    let area = frame.area();
    let popup = centered(area, (area.width * 7 / 10).max(40), (area.height * 7 / 10).max(10));
    frame.render_widget(Clear, popup);
    let (title, hint) = match search.mode {
        Mode::Text => (" Search inside files ", " tab: file names · ↑↓ move · ⏎ open · esc close "),
        _ => (" Files ", " tab: search inside files · ←→ fold · ⏎ open · esc close "),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(title)
        .title_style(Style::new().fg(theme::fg()).bold())
        .title_bottom(Line::from(hint).fg(theme::dim()))
        .border_style(border(true))
        .style(Style::new().bg(theme::bg()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, results] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    (app.areas.popup, app.areas.popup_list) = (popup, results);
    let status = match search.mode {
        _ if search.busy => "searching…".to_string(),
        Mode::Text if search.query.chars().count() < 2 => "type at least 2 characters".to_string(),
        Mode::Files if search.query.is_empty() => format!("{} files · type to filter", search.file_count()),
        _ if search.hits.len() >= 500 => "500+ matches".to_string(),
        _ => format!("{} matches", search.hits.len()),
    };
    frame.render_widget(
        Line::from(vec![
            Span::from(" › ").fg(theme::blue()).bold(),
            Span::from(search.query.clone()),
            Span::from("█").fg(theme::blue()),
            Span::from(format!("   {status}")).fg(theme::muted()),
        ]),
        input,
    );
    if search.mode == Mode::Files {
        let ws = &mut app.workspaces[app.current];
        let changed: Vec<(PathBuf, char)> = ws.status.changes.iter().map(|c| (ws.root.join(&c.path), c.kind())).collect();
        let list = |items| List::new(items).highlight_style(highlight(true));
        if search.query.is_empty() {
            frame.render_stateful_widget(list(file_items(&ws.tree.rows, &changed)), results, &mut ws.files_state);
        } else {
            frame.render_stateful_widget(list(file_items(&search.tree, &changed)), results, &mut search.state);
        }
        return;
    }
    let items: Vec<ListItem> = search
        .hits
        .iter()
        .map(|hit| {
            ListItem::new(Line::from(vec![
                Span::from(format!(" {}", hit.rel)).fg(theme::muted()),
                Span::from(format!(":{}", hit.line.unwrap_or(0) + 1)).fg(theme::yellow()),
                Span::from(format!("  {}", hit.text)),
            ]))
        })
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), results, &mut search.state);
}

fn draw_help(frame: &mut Frame, app: &mut App) {
    if !app.help {
        return;
    }
    let p = app.keymap.label();
    let prefixed = |keys: &str| format!("{p} {keys}");
    // A ⌃ prefix pressed twice goes to the program (⌘ never does).
    let twice = app.keymap.prefix.iter().find(|k| k.mods.contains(ratatui::crossterm::event::KeyModifiers::CONTROL)).map(|k| format!("{k} {k}"));
    let sections: Vec<(&str, Vec<(String, &str)>)> = vec![
        (
            "Panels",
            vec![
                (prefixed("w s t"), "workspaces, sessions, the terminal"),
                (prefixed("f c"), "files, changes"),
                (prefixed("< >"), "hide or show the left side, the right side"),
                (prefixed("[ ]"), "previous, next terminal tab"),
                (prefixed("a"), "the next terminal waiting for you"),
            ],
        ),
        (
            "Make and close",
            vec![
                (prefixed("n"), "new workspace, session, terminal or file, by panel"),
                (prefixed("x"), "remove the workspace, delete the session, close the terminal"),
            ]
            .into_iter()
            .chain(twice.map(|keys| (keys, "the key itself, to the program")))
            .collect(),
        ),
        (
            "Files",
            vec![
                (prefixed("/"), "search: files by name, tab for inside them; opens full screen"),
                (prefixed("n"), "new file (ending with / for a folder)"),
                (prefixed("e r d"), "edit, rename, delete"),
                ("↑↓ ←→ ⏎".into(), "move, fold, open full screen"),
            ],
        ),
        (
            "Full screen",
            vec![
                ("esc".into(), "back to the tree, then to the three columns"),
                ("⏎ tab".into(), "from the tree into the file, and back"),
                ("pgup pgdn".into(), "scroll the file"),
                ("⌘s esc".into(), "editing: save, leave (twice to throw changes away)"),
            ],
        ),
        (
            "Mouse",
            vec![
                ("click".into(), "go there; double-click a file: full screen"),
                ("drag".into(), "select to copy in a terminal; a border to resize"),
                ("wheel swipe".into(), "scroll, and sideways in files"),
                ("ctrl-click".into(), "open a link, or a path:line"),
            ],
        ),
        ("Quit", vec![(prefixed("q"), "quit, terminals keep running"), (prefixed("Q"), "quit and stop every terminal")]),
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

fn draw_repos(frame: &mut Frame, app: &mut App) {
    let Some(search) = &mut app.repos else { return };
    let area = frame.area();
    let popup = centered(area, (area.width * 6 / 10).max(40), (area.height * 6 / 10).max(10));
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(" New workspace ")
        .title_style(Style::new().fg(theme::fg()).bold())
        .title_bottom(Line::from(" git repositories in your home folder · type to filter, or a path (~/…) · ⏎ add · esc close ").fg(theme::dim()))
        .border_style(border(true))
        .style(Style::new().bg(theme::bg()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, results] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    (app.areas.popup, app.areas.popup_list) = (popup, results);
    let status = if search.busy { "looking for git repositories in your home folder…".to_string() } else { format!("{} found", search.hits.len()) };
    frame.render_widget(
        Line::from(vec![
            Span::from(" › ").fg(theme::blue()).bold(),
            Span::from(search.query.clone()),
            Span::from("█").fg(theme::blue()),
            Span::from(format!("   {status}")).fg(theme::muted()),
        ]),
        input,
    );
    let items: Vec<ListItem> = search.hits.iter().map(|hit| ListItem::new(Line::from(format!(" {}", hit.rel)))).collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), results, &mut search.state);
}

fn state_icon(state: State) -> (&'static str, Color) {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    match state {
        State::Running => (SPINNER[(now.as_millis() / 100) as usize % SPINNER.len()], theme::yellow()),
        State::Waiting | State::Done => ("◆", theme::magenta()),
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
