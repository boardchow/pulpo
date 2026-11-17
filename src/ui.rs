use std::collections::HashMap;
use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph};

use crate::agents::State;
use crate::app::{App, Areas, Ask, Focus};
use crate::review::{CURSOR, Shown};
use crate::theme;
use crate::search::Mode;
use crate::tree::Row;

pub fn draw(frame: &mut Frame, app: &mut App) {
    app.reorder();
    app.files_expanded &= app.focus == Focus::Files;
    app.changes_expanded &= app.focus == Focus::Changes;
    frame.render_widget(Block::new().style(Style::new().fg(theme::fg()).bg(theme::bg())), frame.area());
    let [main, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    if app.no_projects {
        draw_welcome(frame, app, main);
        draw_footer(frame, app, footer);
        return;
    }
    let left = if app.left_hidden { 0 } else { app.left_width.unwrap_or((main.width * 18 / 100).clamp(26, 36)).clamp(20.min(main.width / 3), main.width / 3) };
    let [left_area, center_area] = Layout::horizontal([Constraint::Length(left), Constraint::Fill(1)]).areas(main);
    app.areas = Areas {
        left_divider: if left > 0 { Rect { x: center_area.x.saturating_sub(1), width: 2, ..center_area } } else { Rect::default() },
        ..Areas::default()
    };
    if left > 0 {
        draw_left(frame, app, left_area);
    }
    match app.focus {
        Focus::Files if app.files_expanded || app.editor.is_some() => draw_file(frame, app, center_area),
        Focus::Changes if app.changes_expanded => draw_diff(frame, app, center_area),
        _ => draw_center(frame, app, center_area),
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

const PULPO_QUADRANTS: [&str; 3] = ["▗▄████▄▖", "▝▙▄██▄▟▘", "▞▚▀▞▚▀▞▚"];

/// Fonts rarely have octants; Ghostty and kitty draw them themselves.
static OCTANTS: std::sync::LazyLock<bool> = std::sync::LazyLock::new(|| {
    std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "ghostty") || std::env::var("TERM").is_ok_and(|t| t == "xterm-kitty")
});

fn pulpo(wiggle: bool) -> Vec<String> {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let art = if *OCTANTS { PULPO } else { PULPO_QUADRANTS };
    let mut rows: Vec<String> = art.iter().map(|row| row.to_string()).collect();
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
    frame.render_widget(name(), title);
}

fn name() -> Line<'static> {
    Line::from(vec![Span::from("Pulpo").fg(theme::fg()).bold(), Span::from(concat!(" v", env!("CARGO_PKG_VERSION"))).fg(theme::muted())])
}

fn panel(frame: &mut Frame, block: Block<'static>, area: Rect, focused: bool, items: Vec<ListItem>, state: &mut ListState) -> Rect {
    let body = block.inner(area);
    frame.render_widget(block, area);
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(focused)), body, state);
    body
}

fn framed(name: &str, focused: bool, rest: Vec<Span<'static>>) -> Block<'static> {
    Block::bordered().border_type(BorderType::Rounded).title(title(name, focused, rest)).border_style(border(focused)).style(Style::new().bg(theme::bg()))
}

fn draw_left(frame: &mut Frame, app: &mut App, area: Rect) {
    let rows = |n: usize| (n.max(1) as u16 + 2).min(area.height / 4).max(3);
    let [_, header, _, projects, workspaces, files, changes, history] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(PULPO.len() as u16),
        Constraint::Length(1),
        Constraint::Length(rows(app.projects().len())),
        Constraint::Length(rows(app.workspace_order().len())),
        Constraint::Fill(2),
        Constraint::Fill(2),
        Constraint::Fill(1),
    ])
    .areas(area);
    draw_header(frame, app, header);
    let focused = app.focus == Focus::Projects;
    app.areas.panels.extend([(Focus::Projects, projects), (Focus::Workspaces, workspaces)]);
    let block = framed("Projects", focused, Vec::new());
    let rows = block.inner(projects);
    frame.render_widget(block, projects);

    let items: Vec<ListItem> = app
        .projects()
        .into_iter()
        .map(|i| ListItem::new(Line::from(state_name(app.workspace_state(i), &app.workspaces[i].name, i == app.current))))
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(focused)), rows, &mut app.projects_state);
    app.areas.projects = rows;

    let order = app.workspace_order();
    // States change between frames, and the order with them.
    app.workspaces_state.select(order.iter().position(|&i| i == app.current));
    let items: Vec<ListItem> = order
        .into_iter()
        .enumerate()
        .map(|(n, i)| {
            let w = &app.workspaces[i];
            // Prefix and this number goes to it.
            let number = if n < 9 { format!(" {}", n + 1) } else { "  ".to_string() };
            let mut spans = vec![Span::from(number).fg(theme::dim())];
            spans.extend(state_name(app.workspace_state(i), &w.name, i == app.current));
            if !w.status.changes.is_empty() {
                spans.push(Span::from(format!(" ±{}", w.status.changes.len())).fg(theme::blue()));
            }
            let repo = app.workspaces.iter().find(|r| r.repo == w.repo && !r.is_worktree()).map_or(&w.name, |r| &r.name);
            let used = spans.iter().map(Span::width).sum::<usize>();
            let room = (workspaces.width as usize).saturating_sub(used + 4);
            if room >= 4 {
                let repo = if repo.chars().count() > room { format!("{}…", repo.chars().take(room - 1).collect::<String>()) } else { repo.clone() };
                spans.push(Span::from(" ".repeat(room + 1 - repo.chars().count())));
                spans.push(Span::from(repo).fg(theme::dim()));
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let focused = app.focus == Focus::Workspaces;
    let empty = items.is_empty();
    app.areas.workspaces = panel(frame, framed("Workspaces", focused, Vec::new()), workspaces, focused, items, &mut app.workspaces_state);
    if empty {
        frame.render_widget(Line::from(" none yet").fg(theme::dim()), app.areas.workspaces);
    }
    draw_files(frame, app, files);
    draw_changes(frame, app, changes);
    draw_history(frame, app, history);
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

fn draw_center(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Terminal;
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let mut spans = vec![Span::from(" ")];
    let mut x = area.x + 2;
    let tabs: Vec<usize> = app.workspace_terms();
    let room = area.width.saturating_sub(2 + 1 + 3) as usize;
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
    frame.render_widget(Line::from(spans), Rect { x: area.x + 1, y: area.y, width: area.width.saturating_sub(2), height: 1 });

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

fn draw_history(frame: &mut Frame, app: &mut App, area: Rect) {
    app.areas.panels.push((Focus::History, area));
    let focused = app.focus == Focus::History;
    let Some(changes) = app.shelved.as_mut().or(app.changes.as_mut()) else { return };
    let history = &changes.history;
    let unpushed = history.commits.iter().filter(|c| !c.pushed).count();
    let more = if history.more { "+" } else { "" };
    let mut rest = vec![Span::from(format!("{}{more} ", history.commits.len())).fg(theme::blue())];
    if !changes.since.is_empty() {
        rest.push(Span::from(format!("{} ", changes.since)).fg(theme::muted()));
    }
    rest.extend(match (&history.remote, unpushed) {
        (_, 0) if history.commits.is_empty() => Vec::new(),
        (None, _) => vec![Span::from("not pushed ").fg(theme::yellow())],
        (Some(_), 0) => vec![Span::from("pushed ").fg(theme::green())],
        (Some(_), n) => vec![Span::from(format!("{n} unpushed ")).fg(theme::yellow())],
    });
    if history.behind > 0 {
        rest.push(Span::from(format!("{} to pull ", history.behind)).fg(theme::yellow()));
    }
    let block = Block::bordered().border_type(BorderType::Rounded).title(title("History", focused, rest)).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let body = block.inner(area);
    app.areas.history = body;
    let items: Vec<ListItem> = history
        .commits
        .iter()
        .map(|c| {
            let mark = if c.pushed { Span::from("   ") } else { Span::from(" ↑ ").fg(theme::yellow()) };
            let when = Span::from(format!(" {}", c.when)).fg(theme::dim());
            ListItem::new(Line::from(vec![mark, Span::from(format!("{} ", c.hash)).fg(theme::dim()), Span::from(c.subject.clone()), when]))
        })
        .collect();
    if items.is_empty() {
        frame.render_widget(block, area);
        frame.render_widget(Line::from(" no commits yet").fg(theme::dim()), body);
        return;
    }
    panel(frame, block, area, focused, items, &mut changes.history_state);
}

fn title(name: &str, focused: bool, rest: Vec<Span<'static>>) -> Line<'static> {
    let mut spans = vec![Span::from(format!(" {name} ")).fg(if focused { theme::fg() } else { theme::muted() }).bold()];
    spans.extend(rest);
    Line::from(spans)
}

fn in_tree(app: &App) -> bool {
    app.focus == Focus::Files && app.editor.is_none() && !app.reading
}

fn draw_files(frame: &mut Frame, app: &mut App, area: Rect) {
    app.areas.panels.push((Focus::Files, area));
    let focused = in_tree(app);
    let unsaved = if app.editor.as_ref().is_some_and(|e| e.dirty) { vec![Span::from("● unsaved ").fg(theme::yellow())] } else { Vec::new() };
    let block = framed("Files", focused, unsaved);
    app.areas.files_tree = block.inner(area);
    frame.render_widget(block, area);
    let ws = &mut app.workspaces[app.current];
    let changed: Vec<(PathBuf, char)> = ws.status.changes.iter().map(|c| (ws.root.join(&c.path), c.kind())).collect();
    frame.render_stateful_widget(List::new(file_items(&ws.tree.rows, &changed)).highlight_style(highlight(focused)), app.areas.files_tree, &mut ws.files_state);
}

fn draw_file(frame: &mut Frame, app: &mut App, area: Rect) {
    let root = &app.ws().root;
    let open = app.editor.as_ref().map(|e| e.path.clone()).or_else(|| match &app.viewer.source {
        Some(crate::viewer::Source::File(path)) => Some(path.clone()),
        _ => None,
    });
    let name = open.map_or("Files".into(), |path| path.strip_prefix(root).unwrap_or(&path).display().to_string());
    let mut rest = Vec::new();
    if app.editor.as_ref().is_some_and(|e| e.dirty) {
        rest.push(Span::from("● unsaved ").fg(theme::yellow()));
    }
    let focused = !in_tree(app);
    let block = Block::bordered().border_type(BorderType::Rounded).title(title(&name, focused, rest)).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let file = block.inner(area);
    frame.render_widget(block, area);
    app.areas.files_file = file;
    if app.editor.is_some() {
        draw_editor(frame, app, file);
    } else if app.viewer.is_open() {
        draw_viewer(frame, app, file);
    } else {
        frame.render_widget(Line::from(" select a file").fg(theme::dim()), file);
    }
}

fn file_items(rows: &[Row], changed: &[(PathBuf, char)]) -> Vec<ListItem<'static>> {
    let mut kinds: HashMap<&Path, char> = HashMap::new();
    for (path, kind) in changed {
        for at in path.ancestors() {
            kinds.entry(at).or_insert(*kind);
        }
    }
    rows.iter().map(|row| tree_item(row, kinds.get(row.path.as_path()).copied(), 0)).collect()
}

fn tree_item(row: &Row, kind: Option<char>, comments: usize) -> ListItem<'static> {
    let icon = match (row.is_dir, row.expanded) {
        (true, true) => "▾ ",
        (true, false) => "▸ ",
        _ => "  ",
    };
    let style = kind.map_or_else(Style::default, |k| Style::default().fg(kind_color(k)));
    let mut spans = vec![Span::from(format!(" {}{icon}", "  ".repeat(row.depth))).fg(theme::dim()), Span::styled(row.name.clone(), style)];
    if comments > 0 {
        spans.push(Span::from(format!(" ✎{comments}")).fg(theme::yellow()));
    }
    ListItem::new(Line::from(spans))
}

fn draw_changes(frame: &mut Frame, app: &mut App, area: Rect) {
    app.areas.panels.push((Focus::Changes, area));
    let focused = app.focus == Focus::Changes && !app.reading;
    let Some(changes) = &mut app.changes else { return };
    let name = if changes.commit.is_some() { "Commit" } else { "Changes" };
    let count = Span::from(format!("{} ", changes.files.len())).fg(theme::blue());
    let block = framed(name, focused, vec![count]);
    let tree = block.inner(area);
    frame.render_widget(block, area);
    app.areas.changes_tree = tree;
    let items: Vec<ListItem> = changes.rows.iter().map(|row| tree_item(row, changes.status_of(&row.path), changes.count(&row.path))).collect();
    if items.is_empty() {
        frame.render_widget(Line::from(" nothing uncommitted").fg(theme::dim()), tree);
    }
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(focused)), tree, &mut changes.state);
}

fn draw_diff(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.reading;
    let Some(changes) = &app.changes else { return };
    let mut rest = Vec::new();
    if let Some(c) = &changes.commit {
        rest.push(Span::from(format!("{} ", c.hash)).fg(theme::muted()));
        rest.push(Span::from(format!("{} ", c.subject)).fg(theme::fg()));
        rest.push(Span::from(format!("· {}, {} ", c.author, c.when)).fg(theme::muted()));
    }
    let name = match &changes.diff {
        Some((path, diff)) => {
            rest.push(Span::from(format!("+{} ", diff.added)).fg(theme::green()));
            rest.push(Span::from(format!("−{} ", diff.removed)).fg(theme::red()));
            path.strip_prefix(&changes.root).unwrap_or(path).display().to_string()
        }
        None if changes.commit.is_some() => "Commit".into(),
        None => "Changes".into(),
    };
    if !changes.comments.is_empty() {
        rest.push(Span::from(format!("· ✎{} ", changes.comments.len())).fg(theme::yellow()));
        let button = Span::from(format!(" send review {} s ", app.keymap.label())).fg(theme::on_accent()).bg(theme::accent()).bold();
        let x = area.x + 1 + (Span::from(name.as_str()).width() + 2 + rest.iter().map(Span::width).sum::<usize>()) as u16;
        let width = (button.width() as u16).min(area.right().saturating_sub(x + 1));
        app.areas.send_review = Rect { x, y: area.y, width, height: 1 };
        rest.push(button);
    }
    let block = Block::bordered().border_type(BorderType::Rounded).title(title(&name, focused, rest)).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let file = block.inner(area);
    frame.render_widget(block, area);
    let Some(changes) = &mut app.changes else { return };
    let [file, ruler] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(1)]).areas(file);
    app.areas.changes_file = file;
    app.areas.changes_ruler = ruler;
    changes.set_split(file.width >= SPLIT_WIDTH);
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
    let [old, divider, new] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(1), Constraint::Fill(1)]).areas(file);
    changes.fit(if changes.split { new.width.min(old.width) } else { file.width } as usize, app.writing.as_ref().map(|w| (w, app.writing_at)));
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
    let side = |comment: Option<&crate::review::Comment>| match comment.and_then(|c| c.lines) {
        _ if !changes.split => file,
        Some((true, _, _)) => old,
        _ => new,
    };
    let note = |text: Vec<Span<'static>>| Line::from([vec![Span::from(" ".repeat(diff.gutter.saturating_sub(2))), Span::from("▎ ").fg(theme::yellow())], text].concat());
    for (y, item) in changes.shown.iter().enumerate() {
        let on = |r: Rect| Rect { y: file.y + y as u16, height: 1, ..r };
        if changes.split
            && let Some(cell) = frame.buffer_mut().cell_mut((divider.x, file.y + y as u16))
        {
            cell.set_symbol("│").set_style(bg.fg(theme::border()));
        }
        match *item {
            Shown::Row(row) | Shown::Wrap { row, .. } => {
                let piece = if let Shown::Wrap { piece, .. } = *item { piece } else { 0 };
                if !changes.split {
                    frame.render_widget(Paragraph::new(shown(row, piece)).style(bg), on(file));
                    continue;
                }
                let (o, n) = diff.pairs[row];
                if let Some(i) = o {
                    let mut line = shown(i, piece);
                    if piece == 0 && diff.kinds[i] == crate::viewer::Kind::Same {
                        line.spans[0] = Span::from(format!("{:>w$}   ", diff.old_numbers[i], w = diff.gutter - 3)).fg(theme::dim());
                    }
                    frame.render_widget(Paragraph::new(mark(line, i)).style(bg), on(old));
                }
                if let Some(i) = n {
                    frame.render_widget(Paragraph::new(shown(i, piece)).style(bg), on(new));
                }
            }
            Shown::Note(i, ref text) => {
                let text = match text.split_once(CURSOR).filter(|_| i.is_none()) {
                    Some((before, after)) => vec![Span::from(before.to_string()), Span::from("█").fg(theme::blue()), Span::from(after.to_string())],
                    None => vec![Span::from(text.clone())],
                };
                let line = note(text);
                let comment = i.map_or(app.writing.as_ref(), |i| changes.comments.get(i));
                frame.render_widget(Paragraph::new(line).style(Style::new().fg(theme::fg()).bg(theme::bg())), on(side(comment)));
            }
            Shown::Hint => {
                let hint = Span::from("⏎ keep, empty deletes · ⌥⏎ or ⌃j new line · esc cancel").fg(theme::muted());
                frame.render_widget(Paragraph::new(note(vec![hint])).style(Style::new().bg(theme::bg())), on(side(app.writing.as_ref())));
            }
        }
    }
}

/// So a 14" laptop, about 155 columns of diff, keeps one view.
const SPLIT_WIDTH: u16 = 170;

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
    let reading = app.focus == Focus::Files && app.reading;
    if reading && app.viewer.mark.is_none() {
        app.viewer.mark = Some(app.viewer.scroll);
    }
    let (scroll, end) = (app.viewer.scroll, (app.viewer.scroll + area.height as usize).min(app.viewer.row_count()));
    let lines: Vec<Line> = (scroll..end).map(|i| app.viewer.decorated(i, reading)).collect();
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
        let project = app.new_in.file_name().unwrap_or_default().to_string_lossy().into_owned();
        match &prompt.ask {
            Ask::Project(at) => {
                let mut spans = vec![
                    Span::from(" new workspace in ").fg(theme::on_accent()).bg(theme::accent()).bold(),
                    Span::from(" › ").fg(theme::blue()).bold(),
                ];
                for (n, i) in app.projects().into_iter().enumerate() {
                    let name = Span::from(format!(" {} ", app.workspaces[i].name));
                    spans.push(if n == *at { name.fg(theme::fg()).bg(theme::hover()).bold() } else { name.fg(theme::muted()) });
                }
                spans.push(Span::from("   ↑↓ project · ⏎ next · esc cancel").fg(theme::muted()));
                Line::from(spans)
            }
            Ask::Workspace(branch) => workspace_input(&project, &prompt.text, branch, false),
            Ask::Branch(name) => workspace_input(&project, name, &prompt.text, true),
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

fn workspace_input(project: &str, name: &str, branch: &str, on_branch: bool) -> Line<'static> {
    let cursor = |here: bool| Span::from(if here { "█" } else { "" }).fg(theme::blue());
    let branch = if branch.is_empty() && !on_branch {
        Span::from("from the name").fg(theme::muted())
    } else {
        Span::from(branch.to_string())
    };
    Line::from(vec![
        Span::from(format!(" new workspace in {project} ")).fg(theme::on_accent()).bg(theme::accent()).bold(),
        Span::from(" › ").fg(theme::blue()).bold(),
        Span::from(name.to_string()),
        cursor(!on_branch),
        Span::from("   branch").fg(theme::blue()).bold(),
        Span::from(" › ").fg(theme::blue()).bold(),
        branch,
        cursor(on_branch),
        Span::from("   tab switches · ⏎ make it · esc cancel").fg(theme::muted()),
    ])
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
    let (name, hint) = match search.mode {
        Mode::Text => (" Search inside files ", " tab: file names · ↑↓ move · ⏎ open · esc close "),
        _ => (" Files ", " tab: search inside files · ↑↓ move · ⏎ open · esc close "),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(name)
        .title_style(Style::new().fg(theme::fg()).bold())
        .title_bottom(Line::from(hint).fg(theme::dim()))
        .border_style(border(true))
        .style(Style::new().bg(theme::bg()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, results] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    (app.areas.popup, app.areas.popup_list) = (popup, results);
    let status = match search.rows.len() {
        _ if search.busy => "searching…".to_string(),
        _ if search.mode == Mode::Text && search.query.chars().count() < 2 => "type at least 2 characters".to_string(),
        _ if search.query.is_empty() => format!("{} files · type to filter", search.file_count()),
        n if n >= 500 => "500+ matches".to_string(),
        n => format!("{n} matches"),
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
    let items: Vec<ListItem> = search
        .rows
        .iter()
        .map(|hit| match hit.line {
            Some(line) => ListItem::new(Line::from(vec![
                Span::from(format!(" {}", hit.rel)).fg(theme::muted()),
                Span::from(format!(":{}", line + 1)).fg(theme::yellow()),
                Span::from(format!("  {}", hit.text)),
            ])),
            None => ListItem::new(format!(" {}", hit.rel)),
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
    let twice = app.keymap.prefix.iter().find(|k| k.mods.contains(ratatui::crossterm::event::KeyModifiers::CONTROL)).map(|k| format!("{k} {k}"));
    let sections: Vec<(&str, Vec<(String, &str)>)> = vec![
        (
            "Panels",
            vec![
                (prefixed("p w t"), "projects, workspaces, the terminal"),
                (prefixed("f c h"), "files, changes, history"),
                (prefixed("F C H"), "files, changes full screen; the selected commit"),
                (prefixed("<"), "hide or show the left side"),
                (prefixed("[ ]"), "previous, next terminal tab"),
                (prefixed("a"), "the next terminal waiting for you"),
                (prefixed("1-9"), "the workspace with that number"),
            ],
        ),
        (
            "Make and close",
            vec![
                (prefixed("n"), "new project, workspace, terminal, file or comment, by panel"),
                (prefixed("N"), "new workspace, in any project"),
                (prefixed("x"), "remove the project, delete the workspace, close the terminal"),
            ]
            .into_iter()
            .chain(twice.map(|keys| (keys, "the key itself, to the program")))
            .collect(),
        ),
        (
            "Files",
            vec![
                (prefixed("/"), "search file names, tab for inside files"),
                (prefixed("n"), "new file (ending with / for a folder)"),
                (prefixed("e r d"), "edit, rename, delete"),
                ("↑↓ ←→ ⏎".into(), "move, fold, open the file"),
            ],
        ),
        (
            "Full screen",
            vec![
                ("esc".into(), "back to the tree, the terminal beside it, then into it"),
                ("⏎ in history".into(), "the commit: its files and their changes"),
                ("⏎ tab".into(), "from the tree into the file, and back"),
                ("pgup pgdn".into(), "scroll the file"),
                ("⇧↑↓ J K".into(), "changes: select lines, also with ⇧click or drag"),
                (prefixed("n e d"), "changes: comment on the lines or the file, edit, delete"),
                (prefixed("s"), "changes: send the review to the workspace's agent"),
                ("⌃s esc".into(), "editing: save, leave (twice to throw changes away)"),
            ],
        ),
        (
            "Mouse",
            vec![
                ("click".into(), "go there; a file, a change or a commit opens full screen"),
                ("drag".into(), "select to copy in a terminal; a border to resize"),
                ("wheel swipe".into(), "scroll, and sideways in files"),
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

fn draw_welcome(frame: &mut Frame, app: &mut App, area: Rect) {
    let width = (area.width * 6 / 10).max(60).min(area.width);
    let height = (area.height / 2).clamp(8, 12);
    let [_, logo, _, title, _, picker, _] = Layout::vertical([
        Constraint::Fill(1),
        Constraint::Length(PULPO.len() as u16),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(height),
        Constraint::Fill(2),
    ])
    .areas(area);
    let lines: Vec<Line> = pulpo(false).into_iter().map(|row| Line::from(row).fg(theme::blue())).collect();
    frame.render_widget(Paragraph::new(lines).centered(), logo);
    frame.render_widget(name().centered(), title);
    draw_repos_in(frame, app, Rect { x: area.x + (area.width - width) / 2, width, ..picker });
}

fn draw_repos(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    draw_repos_in(frame, app, centered(area, (area.width * 6 / 10).max(60), (area.height * 6 / 10).max(10)));
}

fn draw_repos_in(frame: &mut Frame, app: &mut App, popup: Rect) {
    let esc = if app.no_projects { "quit" } else { "close" };
    let Some(search) = &mut app.repos else { return };
    frame.render_widget(Clear, popup);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .title(" New project ")
        .title_style(Style::new().fg(theme::fg()).bold())
        .title_bottom(Line::from(format!(" a new folder's path (~/…) starts one · ⏎ add · esc {esc} ")).fg(theme::dim()))
        .border_style(border(true))
        .style(Style::new().bg(theme::bg()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, results] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);
    (app.areas.popup, app.areas.popup_list) = (popup, results);
    let status = if search.busy { "looking for git repositories in your home folder…".to_string() } else { format!("{} found", search.rows.len()) };
    frame.render_widget(
        Line::from(vec![
            Span::from(" › ").fg(theme::blue()).bold(),
            Span::from(search.query.clone()),
            Span::from("█").fg(theme::blue()),
            Span::from(format!("   {status}")).fg(theme::muted()),
        ]),
        input,
    );
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
