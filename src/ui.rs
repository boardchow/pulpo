use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::agents::State;
use crate::app::{App, Focus};
use crate::git::Change;
use crate::keymap::{Action, Group, TABS};
use crate::pane::Pane;
use crate::picker::{self, Kind, Show};
use crate::search::Mode;
use crate::tree::Row;
use crate::workspace::GraphItem;
use crate::theme;

pub fn draw(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Block::new().style(Style::new().fg(theme::fg()).bg(theme::bg())), frame.area());
    let [main, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let width = if app.sidebar_hidden { 0 } else { (main.width * 27 / 100).clamp(28, 42).min(main.width) };
    let [sidebar, right] = Layout::horizontal([Constraint::Length(width), Constraint::Fill(1)]).areas(main);
    if width > 0 {
        draw_sidebar(frame, app, sidebar);
    } else {
        let areas = &mut app.areas;
        (areas.workspaces, areas.sessions, areas.changes, areas.graph) = Default::default();
        areas.tabs.clear();
        areas.counts.clear();
    }
    draw_right(frame, app, right);
    draw_footer(frame, app, footer);
    draw_search(frame, app);
    draw_picker(frame, app);
    draw_prefix(frame, app);
    draw_help(frame, app);
}

/// The pulpo itself, drawn with block octants (Unicode 16): 2×4 pixels per
/// cell, so the pixels come out square and small.
const PULPO: [&str; 3] = [
    "𜺠𜷡𜷥██𜷤𜷞𜺣",
    "𜴡𜷤▆██▆𜷥𜴍",
    "𜴁𜴄🮂𜴁𜴄🮂𜴁𜴄",
];

/// The pulpo, its tentacles moving while something works.
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

/// The pulpo, and beside it the version, what the terminals are doing and
/// where the current workspace lives.
fn draw_header(frame: &mut Frame, app: &mut App, area: Rect) {
    let art_width = PULPO[0].chars().count() as u16;
    let [_, art, text] = Layout::horizontal([Constraint::Length(1), Constraint::Length(art_width), Constraint::Fill(1)]).spacing(1).areas(area);
    let states: Vec<State> = app.terms.iter().map(|term| app.term_state(term)).collect();
    let count = |state: State| states.iter().filter(|&&s| s == state).count();
    let (waiting, working) = (count(State::Waiting) + count(State::Done), count(State::Running));
    let lines: Vec<Line> = pulpo(working > 0).into_iter().map(|row| Line::from(row).fg(theme::blue())).collect();
    frame.render_widget(Paragraph::new(lines), art);

    let busy: Vec<String> = [(waiting, "to read"), (working, "working")].into_iter().filter(|&(n, _)| n > 0).map(|(n, label)| format!("{n} {label}")).collect();
    let activity = match states.len() {
        _ if !busy.is_empty() => busy.join(" · "),
        0 => "no terminals yet".to_string(),
        1 => "1 terminal, all quiet".to_string(),
        n => format!("{n} terminals, all quiet"),
    };
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let root = &app.ws().root;
    let path = match root.strip_prefix(&home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => root.display().to_string(),
    };
    // Too long for the sidebar: keep the end, which names the workspace.
    let (width, length) = (text.width as usize, path.chars().count());
    let path = if length > width && width > 0 { format!("…{}", path.chars().skip(length - width + 1).collect::<String>()) } else { path };
    let lines = vec![
        Line::from(vec![Span::from("Pulpo").fg(theme::fg()).bold(), Span::from(concat!(" v", env!("CARGO_PKG_VERSION"))).fg(theme::muted())]),
        // Colored only when a terminal waits for you or works.
        Line::from(activity).fg(match (waiting, working) {
            (0, 0) => theme::muted(),
            (0, _) => theme::yellow(),
            _ => theme::magenta(),
        }),
        Line::from(path).fg(theme::muted()),
    ];
    frame.render_widget(Paragraph::new(lines), text);
    // A click on what they're doing shows them.
    let show = match (waiting, working) {
        (0, 0) => Show::All,
        (0, _) => Show::Working,
        _ => Show::ToRead,
    };
    app.areas.counts = vec![(show, Rect { y: text.y + 1, height: 1, ..text })];
}

/// The session's tabs, numbered as ⌥1-9 reaches them, each with how its
/// terminals are doing; the modes on, by the heading.
fn draw_tabs(frame: &mut Frame, app: &mut App, area: Rect) {
    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    let mut heading = vec![Span::from(" TABS").fg(theme::muted()).bold(), Span::from(format!("  {}", app.keymap.hint(&TABS))).fg(theme::dim())];
    let badge = |text: &str| Span::from(format!(" {text} ")).fg(theme::bg()).bg(theme::yellow()).bold();
    for (on, text) in [(app.zoomed_pane().is_some(), "ZOOM"), (app.board, "BOARD"), (app.scrolling.is_some(), "SCROLL")] {
        if on {
            heading.extend([Span::from(" "), badge(text)]);
        }
    }
    frame.render_widget(Line::from(heading), head);

    let mut rects = Vec::new();
    let mut lines = Vec::new();
    if let Some(list) = app.tabs() {
        for (n, tab) in list.list.iter().enumerate().take(body.height as usize) {
            let on = n == list.active && !app.board;
            let terms: Vec<&crate::term::Term> = tab.terms().into_iter().filter_map(|id| app.terms.iter().find(|t| t.id == id)).collect();
            let state = terms.iter().map(|t| app.term_state(t)).max_by_key(|s| s.urgency()).unwrap_or(State::Idle);
            let (icon, color) = state_icon(state);
            let named = tab.term.and_then(|id| terms.iter().find(|t| t.id == id)).or(terms.first()).map(|t| t.label().to_string());
            let label = tab.name.clone().or(named).unwrap_or_else(|| "file".into());
            let more = tab.root.panes().len().saturating_sub(1);
            let more = if more > 0 { format!(" +{more}") } else { String::new() };
            let name = Span::from(format!(" {label}"));
            let line = Line::from(vec![
                Span::from(format!(" {} ", n + 1)).fg(theme::dim()),
                Span::from(icon).fg(color).bold(),
                if on { name.fg(theme::fg()).bold() } else { name.fg(theme::muted()) },
                Span::from(more).fg(theme::dim()),
            ]);
            lines.push(if on { line.bg(theme::selection()) } else { line });
            rects.push((n, Rect { y: body.y + n as u16, height: 1, ..body }));
        }
    }
    if lines.is_empty() {
        lines.push(Line::from(format!(" none yet: {} opens one", app.keymap.hint(&[Action::NewTab]))).fg(theme::dim()));
    }
    app.areas.tabs = rects;
    frame.render_widget(Paragraph::new(lines), body);
}

/// A list in the sidebar: its heading (and the key that goes there), then
/// its rows.
fn section(frame: &mut Frame, area: Rect, title: &str, key: &str, focused: bool, list: List, state: &mut ListState) {
    let [head, body] = Layout::vertical([Constraint::Length(1), Constraint::Fill(1)]).areas(area);
    let heading = Line::from(vec![
        Span::from(format!(" {title}")).fg(if focused { theme::blue() } else { theme::muted() }).bold(),
        Span::from(format!("  {key}")).fg(theme::dim()),
    ]);
    frame.render_widget(heading, head);
    frame.render_stateful_widget(list.highlight_style(highlight(focused)), body, state);
}

fn draw_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let in_sidebar = matches!(app.focus, Focus::Workspaces | Focus::Sessions | Focus::Changes | Focus::Graph);
    let outer = Block::bordered().border_type(BorderType::Rounded).border_style(border(in_sidebar)).style(Style::new().bg(theme::bg()));
    let inner = outer.inner(area);
    frame.render_widget(outer, area);
    // The pulpo on top, a blank line under it.
    let [header, _, inner] = Layout::vertical([Constraint::Length(PULPO.len() as u16), Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    draw_header(frame, app, header);
    let workspaces_height = app.repos().len().clamp(1, 8) as u16 + 1;
    let sessions_height = app.sessions().len().clamp(1, 6) as u16 + 1;
    // The last few commits; the rest scrolls inside.
    let graph_height = app.ws().graph_len().clamp(1, 10) as u16 + 1;
    // Changes take whatever height is left. The files are behind f.
    let tabs_height = app.tabs().map_or(0, |t| t.list.len()).clamp(1, 9) as u16 + 1;
    let [workspaces, _, sessions, _, tabs, _, changes, _, graph] = Layout::vertical([
        Constraint::Length(workspaces_height),
        Constraint::Length(1),
        Constraint::Length(sessions_height),
        Constraint::Length(1),
        Constraint::Length(tabs_height),
        Constraint::Length(1),
        Constraint::Min(2),
        Constraint::Length(1),
        Constraint::Length(graph_height),
    ])
    .areas(inner);
    app.areas.workspaces = workspaces;
    app.areas.sessions = sessions;
    app.areas.changes = changes;
    app.areas.graph = graph;

    // Workspaces: the repositories, each with how its sessions are doing.
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
            // Something works or waits in one of its sessions.
            let states = app.workspaces.iter().enumerate().filter(|(_, s)| s.repo == w.repo).filter_map(|(s, _)| app.session_state(s));
            if let Some(state @ (State::Running | State::Waiting | State::Done)) = states.max_by_key(|state| state.urgency()) {
                let (icon, color) = state_icon(state);
                spans.push(Span::from(format!(" {icon}")).fg(color).bold());
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let focused = app.focus == Focus::Workspaces;
    section(frame, workspaces, "WORKSPACES", "w", focused, List::new(items), &mut app.workspaces_state);

    // Sessions of this repository: the checkout itself, then its worktrees.
    // The current one holds the terminals, files and changes on screen.
    let focused = app.focus == Focus::Sessions;
    let items: Vec<ListItem> = app
        .sessions()
        .into_iter()
        .map(|i| {
            let w = &app.workspaces[i];
            let current = i == app.current;
            let (icon, color) = app.session_state(i).map_or((" ", theme::dim()), state_icon);
            let name = Span::from(app.session_name(i).to_string());
            let name = match app.session_state(i) {
                Some(State::Waiting | State::Done) => name.fg(color).bold(),
                _ if current => name.fg(theme::fg()).bold(),
                _ => name.fg(theme::muted()),
            };
            let mut spans = vec![Span::from(format!(" {icon} ")).fg(color).bold(), name];
            if let Some(branch) = &w.status.branch {
                spans.push(Span::from(format!("  {branch}")).fg(theme::muted()));
            }
            if w.status.ahead > 0 {
                spans.push(Span::from(format!(" ↑{}", w.status.ahead)).fg(theme::green()));
            }
            if w.status.behind > 0 {
                spans.push(Span::from(format!(" ↓{}", w.status.behind)).fg(theme::red()));
            }
            if !w.status.changes.is_empty() {
                spans.push(Span::from(format!(" ±{}", w.status.changes.len())).fg(theme::blue()));
            }
            // An isolated session: where its app answers.
            if let Some(session) = app.isolated.iter().find(|s| s.root == w.root) {
                let web = session.ports.iter().filter(|(var, _)| ["APP", "WEB", "HTTP"].iter().any(|k| var.contains(k)));
                for (_, port) in web {
                    spans.push(Span::from(format!(" :{port}")).fg(theme::sky()));
                }
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    section(frame, sessions, "SESSIONS", "s", focused, List::new(items), &mut app.sessions_state);
    draw_tabs(frame, app, tabs);

    let current = app.current;
    let ws = &mut app.workspaces[current];

    // Changes in the current workspace.
    let items: Vec<ListItem> = ws
        .status
        .changes
        .iter()
        .map(|c| {
            let code: String = c.code.iter().map(|&ch| if ch == '.' { ' ' } else { ch }).collect();
            let mut spans = vec![Span::from(format!(" {code} ")).fg(kind_color(c.kind())).bold()];
            spans.extend(change_spans(c));
            ListItem::new(Line::from(spans))
        })
        .collect();
    let focused = app.focus == Focus::Changes;
    let title = format!("CHANGES {}", ws.status.changes.len());
    section(frame, changes, &title, "c", focused, List::new(items), &mut ws.changes_state);

    // History of the local branches, each lane of the graph in its own color,
    // and the files of the commit unfolded.
    let lanes = |graph: &str, file: bool| -> Vec<Span<'static>> {
        let mut spans = vec![Span::from(" ")];
        spans.extend(graph.chars().enumerate().map(|(i, ch)| {
            // Under a commit, its lane goes on down past the files.
            let ch = match ch {
                '*' if file => '|',
                '*' => '●',
                _ if file => ' ',
                ch => ch,
            };
            Span::from(ch.to_string()).fg(lanes()[i / 2 % 5])
        }));
        spans
    };
    let items: Vec<ListItem> = (0..ws.graph_len())
        .filter_map(|i| ws.graph_item(i))
        .map(|item| match item {
            GraphItem::Row(row) => {
                let mut spans = lanes(&row.graph, false);
                if let Some(commit) = &row.commit {
                    let fold = if ws.is_commit_open(&commit.hash) { " ▾" } else { "" };
                    spans.push(Span::from(fold).fg(theme::dim()));
                    if !commit.refs.is_empty() {
                        spans.push(Span::from(format!(" {}", commit.refs)).fg(theme::sky()).bold());
                    }
                    spans.push(Span::from(format!(" {}", commit.subject)));
                }
                ListItem::new(Line::from(spans))
            }
            GraphItem::File(row, change) => {
                let mut spans = lanes(&row.graph, true);
                spans.push(Span::from(format!(" {} ", change.kind())).fg(kind_color(change.kind())).bold());
                spans.extend(change_spans(change));
                ListItem::new(Line::from(spans))
            }
        })
        .collect();
    let focused = app.focus == Focus::Graph;
    section(frame, graph, "GRAPH", "g", focused, List::new(items), &mut ws.graph_state);
}

/// Rows of a file tree, colored by git status like VS Code. Among matches,
/// the folders are only the way to them, so they stay quiet.
fn file_items(rows: &[Row], changed: &[(PathBuf, char)], quiet_folders: bool) -> Vec<ListItem<'static>> {
    rows.iter()
        .map(|row| {
            let icon = match (row.is_dir, row.expanded) {
                (true, true) => "▾ ",
                (true, false) => "▸ ",
                _ => "  ",
            };
            let kind = changed
                .iter()
                .find(|(p, _)| if row.is_dir { p.starts_with(&row.path) } else { *p == row.path })
                .map(|(_, k)| *k);
            let style = match kind {
                Some(k) => Style::default().fg(kind_color(k)),
                None if row.is_dir && quiet_folders => Style::default().fg(theme::muted()),
                None => Style::default(),
            };
            ListItem::new(Line::from(vec![
                Span::from(format!(" {}{icon}", "  ".repeat(row.depth))).fg(theme::dim()),
                Span::styled(row.name.clone(), style),
            ]))
        })
        .collect()
}

/// A changed file: its name in the color of the change, lines added and
/// removed, then its folder.
fn change_spans(c: &Change) -> Vec<Span<'static>> {
    let path = Path::new(&c.path);
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let dir = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let mut spans = vec![Span::from(name).fg(kind_color(c.kind()))];
    if c.added > 0 {
        spans.push(Span::from(format!(" +{}", c.added)).fg(theme::green()));
    }
    if c.removed > 0 {
        spans.push(Span::from(format!(" −{}", c.removed)).fg(theme::red()));
    }
    spans.push(Span::from(format!(" {dir}")).fg(theme::muted()));
    spans
}

/// How a terminal shows its state: a spinner while it works, a steady
/// colored mark until you read what it finished or asks, a dim one after.
fn state_icon(state: State) -> (&'static str, Color) {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    match state {
        State::Running => (SPINNER[(now.as_millis() / 100) as usize % SPINNER.len()], theme::yellow()),
        State::Waiting | State::Done => ("◆", theme::magenta()),
        State::Idle => ("○", theme::dim()),
    }
}

/// Colors of the graph's lanes, left to right, like `git log --graph --color`.
fn lanes() -> [Color; 5] {
    [theme::blue(), theme::magenta(), theme::green(), theme::yellow(), theme::sky()]
}

/// The tab on screen: its panes as split, or the zoomed one alone.
fn draw_right(frame: &mut Frame, app: &mut App, area: Rect) {
    app.areas.editor = Rect::default();
    app.areas.viewer = Rect::default();
    app.areas.ruler = Rect::default();
    app.areas.panes.clear();
    app.areas.dividers.clear();
    app.set_term_size(area.height.saturating_sub(2), area.width.saturating_sub(2));
    if app.board {
        draw_board(frame, app, area);
        return;
    }
    let Some(tab) = app.tab() else {
        draw_empty(frame, app, area);
        return;
    };
    let (panes, dividers) = match app.zoomed_pane() {
        Some(pane) => (vec![(pane, area)], Vec::new()),
        None => (tab.root.layout(area), tab.root.dividers(area)),
    };
    app.areas.dividers = dividers;
    for &(pane, rect) in &panes {
        match pane {
            Pane::Term(id) => draw_term_pane(frame, app, id, rect, None),
            Pane::File if app.editor.is_some() => draw_editor(frame, app, rect),
            Pane::File => draw_viewer(frame, app, rect),
        }
    }
    app.areas.panes = panes;
}

/// Every terminal of the repository, tiled, each titled with its session;
/// or the one you zoomed.
fn draw_board(frame: &mut Frame, app: &mut App, area: Rect) {
    let terms = app.repo_terms();
    let zoomed = app.active.filter(|id| app.zoomed && app.focus == Focus::Terminal && terms.iter().any(|(t, _)| t == id));
    let tiles = match zoomed {
        Some(id) => vec![(Pane::Term(id), area)],
        None => {
            let panes: Vec<Pane> = terms.iter().map(|&(id, _)| Pane::Term(id)).collect();
            crate::pane::arrange(&panes, 3).map_or_else(Vec::new, |grid| grid.layout(area))
        }
    };
    for &(pane, rect) in &tiles {
        if let Pane::Term(id) = pane {
            let session = terms.iter().find(|(t, _)| *t == id).map(|&(_, s)| app.session_name(s).to_string());
            draw_term_pane(frame, app, id, rect, session);
        }
    }
    app.areas.panes = tiles;
}

/// A terminal in its pane, sized to it, its state (and on the board, its
/// session) in the title. Scrolling back, what you look for stands out.
fn draw_term_pane(frame: &mut Frame, app: &mut App, id: usize, area: Rect, session: Option<String>) {
    let focused = app.focus == Focus::Terminal && app.active == Some(id);
    let Some(term) = app.terms.iter().find(|t| t.id == id) else { return };
    let state = app.term_state(term);
    let (icon, color) = state_icon(state);
    let label = Span::from(term.label().to_string());
    let mut title = vec![Span::from(" ")];
    if let Some(session) = session {
        title.push(Span::from(format!("{session} · ")).fg(theme::dim()));
    }
    title.extend([Span::from(format!("{icon} ")).fg(color).bold(), if focused { label.fg(theme::fg()).bold() } else { label.fg(theme::muted()) }]);
    if matches!(state, State::Waiting | State::Done) {
        title.push(Span::from(" to read").fg(theme::magenta()));
    }
    title.push(Span::from(" "));
    let block = Block::bordered().border_type(BorderType::Rounded).title(Line::from(title)).border_style(border(focused)).style(Style::new().bg(theme::bg()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let Some(term) = app.terms.iter_mut().find(|t| t.id == id) else { return };
    term.resize(inner.height.max(1), inner.width.max(1));
    if let Some(cursor) = term.render(inner, frame.buffer_mut())
        && focused
    {
        frame.set_cursor_position(cursor);
    }
    underline_links(frame.buffer_mut(), inner);
    let Some(scroll) = app.scrolling.as_ref().filter(|s| s.id == id) else { return };
    if !scroll.typing && !scroll.query.is_empty() {
        mark(frame.buffer_mut(), inner, &scroll.query);
    }
    // The line you're on, and the lines selected.
    let top = app.terms.iter().find(|t| t.id == id).map_or(0, |t| t.top_line());
    let selected = scroll.anchor.map(|a| (a.min(scroll.cursor), a.max(scroll.cursor)));
    for row in 0..inner.height {
        let line = top + row as usize;
        let bg = if line == scroll.cursor {
            theme::selection()
        } else if selected.is_some_and(|(from, to)| (from..=to).contains(&line)) {
            theme::text_selection()
        } else {
            continue;
        };
        for x in inner.left()..inner.right() {
            if let Some(cell) = frame.buffer_mut().cell_mut((x, inner.y + row)) {
                cell.set_bg(bg);
            }
        }
    }
}

/// Links in a terminal, underlined: ctrl-click opens them.
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

/// Where `needle` starts in `chars`, from `from` on.
fn row_find(chars: &[char], needle: &str, from: usize) -> Option<usize> {
    let needle: Vec<char> = needle.chars().collect();
    (from..chars.len().saturating_sub(needle.len() - 1)).find(|&i| chars[i..i + needle.len()] == needle[..])
}

/// Every place `query` shows in `area` (any case), on the find color.
fn mark(buf: &mut ratatui::buffer::Buffer, area: Rect, query: &str) {
    let needle: Vec<char> = query.to_lowercase().chars().collect();
    for y in area.top()..area.bottom() {
        let row: Vec<char> = (area.left()..area.right())
            .map(|x| buf.cell((x, y)).and_then(|c| c.symbol().chars().next()).unwrap_or(' ').to_lowercase().next().unwrap_or(' '))
            .collect();
        for start in 0..row.len().saturating_sub(needle.len() - 1) {
            if row[start..start + needle.len()] == needle[..] {
                for x in start..start + needle.len() {
                    if let Some(cell) = buf.cell_mut((area.x + x as u16, y)) {
                        cell.set_bg(theme::find_bg());
                    }
                }
            }
        }
    }
}

/// No tab in this session: the pulpo, and how to start.
fn draw_empty(frame: &mut Frame, app: &App, area: Rect) {
    let block = Block::bordered().border_type(BorderType::Rounded).border_style(border(false)).style(Style::new().bg(theme::bg()));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let working = app.terms.iter().any(|t| app.term_state(t) == State::Running);
    let keymap = &app.keymap;
    let key = |k: String, text: &str| Line::from(vec![Span::from(format!("{k:<10}")).fg(theme::blue()).bold(), Span::from(text.to_string()).fg(theme::muted())]);
    let mut lines: Vec<Line> = pulpo(working).into_iter().map(|row| Line::from(row).fg(theme::blue())).collect();
    lines.extend([
        Line::default(),
        Line::from("No terminals in this session").bold(),
        Line::default(),
        key("t".into(), "a shell here, then type claude"),
        key(keymap.hint(&[Action::NewTab]), "the same, from anywhere"),
        key(keymap.hint(&[Action::Help]), "every key"),
    ]);
    let height = lines.len() as u16;
    let width = 44.min(inner.width);
    let spot = Rect { x: inner.x + inner.width.saturating_sub(width) / 2, y: inner.y + inner.height.saturating_sub(height) / 3, width, height: height.min(inner.height) };
    // The pulpo and the heading centered, the keys under them.
    for (i, line) in lines.into_iter().enumerate() {
        let row = Rect { y: spot.y + i as u16, height: 1, ..spot };
        if row.y >= inner.bottom() {
            break;
        }
        let line = if i < 5 { line.centered() } else { line };
        frame.render_widget(line, row);
    }
}

/// The file being edited, drawn as the viewer draws files.
fn draw_editor(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Editor;
    let root = app.ws().root.clone();
    let Some(editor) = app.editor.as_mut() else { return };
    let name = editor.path.strip_prefix(&root).unwrap_or(&editor.path).display().to_string();
    let title = if editor.dirty { format!("{name} ● unsaved") } else { name };
    let block = block(&title, focused).style(Style::new().bg(theme::editor_bg()));
    let inner = block.inner(area);
    editor.view = (inner.height as usize, inner.width as usize);
    let rows = editor.rows(inner.height as usize);
    let cursor = editor.cursor_cell();
    app.areas.editor = inner;
    frame.render_widget(Paragraph::new(rows).block(block), area);
    if focused
        && let Some((row, col)) = cursor
        && row < inner.height as usize
        && col < inner.width as usize
    {
        frame.set_cursor_position((inner.x + col as u16, inner.y + row as u16));
    }
}

fn draw_viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = block(&app.viewer.heading(), app.focus == Focus::Viewer).style(Style::new().bg(theme::editor_bg()));
    let inner = block.inner(area);
    // The overview ruler takes the last column.
    let [text, ruler] = Layout::horizontal([Constraint::Fill(1), Constraint::Length(1)]).areas(inner);
    app.viewer.height = inner.height as usize;
    app.viewer.set_width(text.width);
    if let Some(line) = app.viewer.goto.take() {
        app.viewer.scroll = line.saturating_sub(inner.height as usize / 3);
    }
    app.viewer.scroll_by(0);
    app.areas.viewer = area;
    let (scroll, end) = (app.viewer.scroll, (app.viewer.scroll + inner.height as usize).min(app.viewer.row_count()));
    let lines: Vec<Line> = (scroll..end).map(|i| app.viewer.decorated(i, text.width)).collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);

    // Changes and matches as colored marks, over a lighter band for the part in view.
    app.areas.ruler = ruler;
    for (i, (color, in_view)) in app.viewer.ruler(ruler.height as usize).into_iter().enumerate() {
        if let Some(cell) = frame.buffer_mut().cell_mut((ruler.x, ruler.y + i as u16)) {
            cell.set_symbol(if color.is_some() { "▐" } else { " " });
            cell.set_style(Style::new().fg(color.unwrap_or(theme::editor_bg())).bg(if in_view { theme::border() } else { theme::editor_bg() }));
        }
    }
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let keymap = &app.keymap;
    if app.prefix.is_some() {
        let again = format!("{} again", keymap.prefix_label());
        let help = keymap.keys_label(&[Action::Help]);
        let mut keys = vec![("esc", "cancel"), (help.as_str(), "every key")];
        if app.focus == Focus::Terminal {
            keys.push((again.as_str(), "sends it to the program"));
        }
        frame.render_widget(Line::from(footer_spans("PREFIX", theme::yellow(), &keys, &[])), area);
        return;
    }
    if app.new_menu {
        let keys = [("w", "workspace"), ("s", "session"), ("t", "terminal"), ("f", "file or folder"), ("esc", "cancel")];
        frame.render_widget(Line::from(footer_spans("NEW", theme::accent(), &keys, &[])), area);
        return;
    }
    if let Some(create) = app.creating.as_ref().filter(|c| c.session) {
        let mut spans = footer_spans("NEW SESSION", theme::accent(), &[("⏎", "create"), ("esc", "cancel")], &[]);
        let input = vec![
            Span::from("› ").fg(theme::blue()).bold(),
            Span::from(create.name.clone()),
            Span::from("█").fg(theme::blue()),
            Span::from("   its own copy, Docker, ports and database   ").fg(theme::muted()),
        ];
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    if let Some(create) = &app.creating {
        // Where it goes, then the name being typed, like the find line.
        let root = &app.ws().root;
        let dir = create.dir.strip_prefix(root).unwrap_or(&create.dir).display().to_string();
        let dir = if dir.is_empty() { String::new() } else { format!("{dir}/") };
        let mut spans = footer_spans("NEW FILE", theme::accent(), &[("⏎", "create"), ("esc", "cancel")], &[]);
        let input = vec![
            Span::from("› ").fg(theme::blue()).bold(),
            Span::from(dir).fg(theme::muted()),
            Span::from(create.name.clone()),
            Span::from("█").fg(theme::blue()),
            Span::from("   end with / for a folder   ").fg(theme::muted()),
        ];
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    if let Some(text) = &app.swarm {
        let mut spans = footer_spans("SWARM", theme::accent(), &[("⏎", "start"), ("esc", "cancel")], &[]);
        let input = vec![
            Span::from("› ").fg(theme::blue()).bold(),
            Span::from(text.clone()),
            Span::from("█").fg(theme::blue()),
            Span::from("   how many (3 if you don't say), then their task: 4 fix the flaky tests   ").fg(theme::muted()),
        ];
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    if let Some(scroll) = app.scrolling.as_ref().filter(|_| app.focus == Focus::Terminal) {
        let back = app.terms.iter().find(|t| t.id == scroll.id).map_or(0, |t| t.scrolled());
        let found = match scroll.found {
            Some(false) => "   no more matches",
            _ => "",
        };
        let keys: &[(&str, &str)] = if scroll.typing {
            &[("⏎", "find, going back"), ("esc", "cancel")]
        } else {
            &[("j/k u/d", "move"), ("g/G", "oldest/now"), ("/", "find"), ("n/N", "older/newer"), ("v", "select"), ("y", "copy"), ("q", "back to now")]
        };
        let mut spans = footer_spans("SCROLL", theme::yellow(), keys, &[]);
        let mut input = Vec::new();
        if scroll.typing || !scroll.query.is_empty() {
            input.push(Span::from("› ").fg(theme::blue()).bold());
            input.push(Span::from(scroll.query.clone()));
            if scroll.typing {
                input.push(Span::from("█").fg(theme::blue()));
            }
        }
        input.push(Span::from(format!("   {back} lines back{found}   ")).fg(theme::muted()));
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    if let Some(name) = &app.renaming {
        let mut spans = footer_spans("RENAME TAB", theme::accent(), &[("⏎", "rename"), ("esc", "cancel")], &[]);
        let input = vec![
            Span::from("› ").fg(theme::blue()).bold(),
            Span::from(name.clone()),
            Span::from("█").fg(theme::blue()),
            Span::from("   empty: named after its terminal   ").fg(theme::muted()),
        ];
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    if let Some(message) = &app.message {
        frame.render_widget(Line::from(format!(" {message}")).fg(theme::yellow()).bold(), area);
        return;
    }
    if app.focus == Focus::Viewer
        && let Some(find) = &app.viewer.find
    {
        let count = match find.matches.len() {
            0 if find.query.is_empty() => String::new(),
            0 => "no match".to_string(),
            n => format!("{}/{n}", find.current + 1),
        };
        let keys: &[(&str, &str)] =
            if find.typing { &[("⏎", "done"), ("esc", "cancel")] } else { &[("n/p", "next/previous"), ("/", "new search"), ("esc", "clear")] };
        // The same input line as the search popup, after the badge.
        let mut input = vec![Span::from("› ").fg(theme::blue()).bold(), Span::from(find.query.clone())];
        if find.typing {
            input.push(Span::from("█").fg(theme::blue()));
        }
        input.push(Span::from(format!("   {count}   ")).fg(theme::muted()));
        let mut spans = footer_spans("FIND IN FILE", theme::accent(), keys, &[]);
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    // Typing what to find in the editor, like finding in the viewer.
    if let Some(editor) = app.editor.as_ref().filter(|e| e.find_typing && app.focus == Focus::Editor) {
        let input = vec![
            Span::from("› ").fg(theme::blue()).bold(),
            Span::from(editor.find.clone().unwrap_or_default()),
            Span::from("█").fg(theme::blue()),
            Span::from("   ").fg(theme::muted()),
        ];
        let mut spans = footer_spans("FIND IN FILE", theme::accent(), &[("⏎", "next"), ("shift-⏎", "previous"), ("esc", "done")], &[]);
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    // In a terminal, the keys come from the keymap.
    let focus = [Action::FocusLeft, Action::FocusUp, Action::FocusRight, Action::FocusDown];
    let terminal_keys: Vec<(String, &str)> = [
        (keymap.prefix_label(), "prefix"),
        (keymap.hint(&focus), "panes"),
        (keymap.hint(&TABS), "tabs"),
        (keymap.hint(&[Action::SplitBeside, Action::SplitBelow]), "split"),
        (keymap.hint(&[Action::NewTab]), "new tab"),
        (keymap.hint(&[Action::ClosePane]), "close"),
        (keymap.hint(&[Action::Zoom]), if app.zoomed { "unzoom" } else { "zoom" }),
        ("shift-⏎".to_string(), "new line"),
        (keymap.hint(&[Action::Help]), "keys"),
    ]
    .into_iter()
    .filter(|(key, _)| !key.trim().is_empty())
    .collect();
    let terminal_keys: Vec<(&str, &str)> = terminal_keys.iter().map(|(key, label)| (key.as_str(), *label)).collect();
    let prefix = keymap.prefix_label();
    // Where you are, then the keys that work there, then the global ones.
    let (badge, keys): (&str, &[(&str, &str)]) = match app.focus {
        Focus::Editor => (
            "EDIT",
            &[
                ("ctrl-s", "save"),
                ("esc", "close"),
                ("ctrl-z/y", "undo/redo"),
                ("shift-arrows", "select"),
                ("ctrl-c/x/v", "copy/cut/paste"),
                ("ctrl-f", "find"),
                ("ctrl-g", "next"),
            ],
        ),
        // Every key goes to the editor; micro's, or how to come back.
        Focus::Terminal if app.in_editor() => (
            "EDITOR",
            match app.term().map(|t| t.info.name.as_str()) {
                Some("micro") => &[
                    ("ctrl-s", "save and close"),
                    ("ctrl-q", "close (asks to save)"),
                    ("ctrl-z", "undo"),
                    ("ctrl-y", "redo"),
                    ("ctrl-f", "find"),
                    ("ctrl-c ctrl-x ctrl-v", "copy, cut, paste"),
                    ("shift-arrows", "select"),
                ],
                _ => &[("quit the editor", "to come back to Pulpo")],
            },
        ),
        Focus::Terminal => ("TERMINAL", &terminal_keys),
        Focus::Workspaces => ("WORKSPACES", &[("j/k", "switch"), ("x", "remove")]),
        Focus::Sessions if app.isolated_current().is_some() => {
            ("SESSIONS", &[("j/k", "switch"), ("⏎", "its terminal"), ("n t", "new terminal in it"), ("x x", "delete")])
        }
        Focus::Sessions => ("SESSIONS", &[("j/k", "switch"), ("⏎", "its terminal"), ("n t", "new terminal in it")]),
        Focus::Changes => ("CHANGES", &[("j/k", "move"), ("⏎", "read diff"), ("e", "edit"), ("y", "copy path")]),
        Focus::Graph => ("GRAPH", &[("j/k", "move"), ("⏎", "a commit's files / read a diff"), ("h/l", "fold")]),
        Focus::Viewer => match (app.viewer.is_diff(), app.viewer.expanded) {
            (true, true) => ("CHANGE", &[("n/p", "next/prev change"), ("j/k h/l", "scroll"), ("/", "find"), ("⏎", "split view"), ("esc", "back")]),
            (true, false) => ("CHANGE", &[("n/p", "next/prev change"), ("j/k h/l", "scroll"), ("/", "find"), ("⏎", "full height"), ("esc", "back")]),
            (false, true) => ("VIEWER", &[("j/k h/l", "scroll"), ("/", "find in file"), ("e", "edit"), ("⏎", "split view"), ("esc", "back")]),
            (false, false) => ("VIEWER", &[("j/k h/l", "scroll"), ("/", "find in file"), ("e", "edit"), ("⏎", "full height"), ("esc", "back")]),
        },
    };
    let global: &[(&str, &str)] = match app.focus {
        Focus::Terminal | Focus::Editor => &[],
        // In the viewer, / finds in the file instead.
        Focus::Viewer => &[("w s c f t", "go to panel"), (&prefix, "prefix"), ("?", "all keys"), ("q", "quit")],
        _ => &[("w s c g f t", "go to panel"), ("n", "new"), (&prefix, "prefix"), ("?", "all keys"), ("q", "quit")],
    };
    let color = if app.focus == Focus::Terminal { theme::yellow() } else { theme::accent() };
    frame.render_widget(Line::from(footer_spans(badge, color, keys, global)), area);
}

fn footer_spans(badge: &str, color: Color, keys: &[(&str, &str)], global: &[(&str, &str)]) -> Vec<Span<'static>> {
    // Text on the indigo accent is white, on lighter badges dark; keys use the
    // lighter blue where the accent would be too dark to read.
    let (badge_fg, key) = if color == theme::accent() { (theme::on_accent(), theme::blue()) } else { (theme::bg(), color) };
    let mut spans = vec![Span::styled(format!(" {badge} "), Style::default().fg(badge_fg).bg(color).bold()), Span::from(" ")];
    let key_color = key;
    for (i, (key, label)) in keys.iter().chain(global).enumerate() {
        if i == keys.len() && !global.is_empty() {
            spans.push(Span::from("│ ").fg(theme::dim()));
        }
        spans.push(Span::from(key.to_string()).fg(key_color).bold());
        spans.push(Span::from(format!(" {label}  ")).fg(theme::muted()));
    }
    spans
}

/// Each panel's own keys. The prefix layer and the chords come from the
/// keymap (`help_sections`).
const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Go to a panel",
        &[
            ("w s c g f v", "Workspaces, Sessions, Changes, Graph, Files, Viewer (g: not from the viewer)"),
            ("t", "into the current terminal (opens a shell if there is none)"),
            ("/", "search inside files (in the viewer: find in this file); tab switches to Files and back"),
            ("z / ctrl-f", "into the current shell, full height (hides the viewer)"),
            ("tab / shift-tab", "next / previous panel"),
        ],
    ),
    (
        "In a panel",
        &[
            ("n w  n t  n f", "new workspace / terminal in this session / file or folder, by the file on screen (end the name with /)"),
            ("n s", "new session, with its own copy, Docker, ports and database"),
            ("e", "edit the file here, looking just as in the viewer ($PULPO_EDITOR: your own editor instead)"),
            ("y", "copy the file's path, relative to the workspace"),
            ("x x", "remove a workspace from the list (files kept) / delete a task session / close the terminal"),
            ("j k  ↑ ↓  ⏎", "move, open (a session's terminal, a file, a diff, a commit's files)"),
            ("h l", "fold / unfold a commit's files in Graph"),
            ("1-9  [ ]", "enter the session's terminal N / previous, next terminal"),
            ("r", "refresh"),
            ("q q", "quit Pulpo; terminals keep running, and come back next time"),
            ("Q Q", "quit Pulpo and stop every terminal (ctrl-c, then hang up)"),
        ],
    ),
    (
        "Files (f)",
        &[
            ("↑ ↓  ← →  ⏎", "move / fold, unfold a folder / open a file, fold a folder"),
            ("type", "filter the tree by name; ⏎ on a folder shows it in the tree"),
            ("esc", "in a file opened from here: back to how things were (zoomed terminal, previewed file)"),
            ("tab", "search inside files instead (and back)"),
            ("ctrl-e  ctrl-y", "edit the file / copy its path, relative to the workspace"),
            ("ctrl-x ctrl-x", "delete the file or folder"),
        ],
    ),
    (
        "In the viewer",
        &[
            ("⏎ / esc", "full height or split view / back to where you were"),
            ("/  n p", "find in this file / next, previous match (or change, in a diff)"),
            ("d u  g G", "page down, up / top, bottom"),
            ("h l  ← →  0", "scroll sideways / back to the start (also shift + wheel, trackpad)"),
            ("click", "focus a panel, pick an item; shift-drag selects text"),
        ],
    ),
    (
        "In the editor",
        &[
            ("ctrl-s  esc", "save / close (asks again with unsaved changes)"),
            ("shift + moves", "select; alt or ctrl + ← → moves by word, ctrl + home end to the file's ends"),
            ("ctrl-c x v  a", "copy, cut, paste (a whole line without a selection) / select all"),
            ("ctrl-z  ctrl-y", "undo / redo"),
            ("tab  shift-tab", "indent (the selected lines) / unindent"),
            ("ctrl-f  ctrl-g", "find / next match (shift: previous)"),
            ("click  drag", "place the cursor / select"),
        ],
    ),
];

/// Every key, by where it works: the keymap's first, then the panels'.
fn help_sections(app: &App) -> Vec<(String, Vec<(String, String)>)> {
    let keymap = &app.keymap;
    let prefix = keymap.prefix.iter().map(ToString::to_string).collect::<Vec<_>>().join(" or ");
    let mut sections: Vec<(String, Vec<(String, String)>)> = Group::ALL
        .iter()
        .map(|&group| {
            let rows = keymap.rows(group).into_iter().map(|(keys, label)| (keys, label.to_string())).collect();
            (format!("{prefix}, then: {}", group.title().to_lowercase()), rows)
        })
        .collect();
    sections.push(("Anywhere, without the prefix".into(), keymap.chord_rows()));
    let mut terminal = vec![
        ("shift-⏎".to_string(), "new line in a Claude prompt".to_string()),
        (format!("{0} {0}", keymap.prefix_label()), "the prefix itself, for the program".to_string()),
        ("claude".to_string(), "type it in a shell to start an agent; the terminal is named after it".to_string()),
        ("ctrl-click".to_string(), "open the link under the mouse, or show the file (path:line) in the viewer; right-click offers it too".to_string()),
        ("drag a border".to_string(), "resize the panes on each side of it".to_string()),
    ];
    if keymap.legacy {
        terminal.push((
            "ctrl-s f n q [ ] 1-9".to_string(),
            "the old keys (sidebar, viewer, shell, close, switch); they go back to the programs in the next release".to_string(),
        ));
    }
    sections.push(("Inside a terminal".into(), terminal));
    sections.extend(HELP.iter().map(|(title, keys)| (title.to_string(), keys.iter().map(|(k, t)| (k.to_string(), t.to_string())).collect())));
    sections
}

fn draw_help(frame: &mut Frame, app: &App) {
    let Some(filter) = &app.help else { return };
    // What you typed, in a key, its text or its section's title.
    let needle = filter.to_lowercase();
    let sections: Vec<(String, Vec<(String, String)>)> = help_sections(app)
        .into_iter()
        .filter_map(|(title, rows)| {
            if title.to_lowercase().contains(&needle) {
                return Some((title, rows));
            }
            let rows: Vec<_> = rows.into_iter().filter(|(k, t)| k.to_lowercase().contains(&needle) || t.to_lowercase().contains(&needle)).collect();
            (!rows.is_empty()).then_some((title, rows))
        })
        .collect();
    let to_lines = |sections: &[(String, Vec<(String, String)>)]| -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        for (title, keys) in sections {
            lines.push(Line::from(format!(" {title}")).bold());
            for (key, text) in keys {
                lines.push(Line::from(vec![Span::from(format!("   {key:<16}")).fg(theme::blue()).bold(), Span::from(text.clone())]));
            }
            lines.push(Line::default());
        }
        lines
    };
    // Two columns when there's room, so it all fits on a laptop, split
    // where the lines are about even; long lines wrap.
    let area = frame.area();
    let total: usize = sections.iter().map(|(_, rows)| rows.len() + 2).sum();
    let mut split = sections.len();
    if area.width >= 160 {
        let mut count = 0;
        let half = sections.iter().position(|(_, rows)| {
            count += rows.len() + 2;
            count * 2 >= total
        });
        split = half.map_or(sections.len(), |i| i + 1);
    }
    let texts: Vec<Vec<Line>> = if split < sections.len() { vec![to_lines(&sections[..split]), to_lines(&sections[split..])] } else { vec![to_lines(&sections)] };
    let width = area.width.min(100 * texts.len() as u16);
    let column = (width.saturating_sub(2) / texts.len() as u16).max(1) as usize;
    // Rows once wrapped, give or take a word per row.
    let rows = |lines: &Vec<Line>| lines.iter().map(|l| l.width().max(1).div_ceil(column.saturating_sub(8).max(1))).sum::<usize>();
    let height = texts.iter().map(rows).max().unwrap_or(0).max(1) as u16 + 4;
    let popup = centered(area, width, height);
    frame.render_widget(Clear, popup);
    frame.render_widget(Block::new().style(Style::new().fg(theme::fg()).bg(theme::bg())), popup);
    let block = block("Keyboard shortcuts", true).title_bottom(Line::from(" type to filter · esc closes ").fg(theme::dim()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, body] = Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Fill(1)]).areas(inner);
    let status = if sections.is_empty() { "   nothing matches".to_string() } else { String::new() };
    frame.render_widget(
        Line::from(vec![Span::from(" › ").fg(theme::blue()).bold(), Span::from(filter.clone()), Span::from("█").fg(theme::blue()), Span::from(status).fg(theme::muted())]),
        input,
    );
    let areas = Layout::horizontal(vec![Constraint::Fill(1); texts.len()]).split(body);
    for (lines, area) in texts.into_iter().zip(areas.iter()) {
        frame.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), *area);
    }
}

/// What can follow the prefix, shown a moment after it: the keymap's
/// groups side by side, and the chords that need no prefix under them.
fn draw_prefix(frame: &mut Frame, app: &App) {
    if !app.prefix_panel() || app.help.is_some() {
        return;
    }
    let keymap = &app.keymap;
    let groups: Vec<(&str, Vec<(String, &str)>)> = Group::ALL.iter().map(|&g| (g.title(), keymap.rows(g))).collect();
    let tall = groups.iter().map(|(_, rows)| rows.len()).max().unwrap_or(0) as u16;
    let area = frame.area();
    // Over the bottom of the screen, just above the footer.
    let height = (tall + 5).min(area.height.saturating_sub(1));
    let popup = Rect { x: area.x, y: area.bottom().saturating_sub(height + 1), width: area.width, height };
    frame.render_widget(Clear, popup);
    let title = Line::from(vec![Span::from(format!(" {} ", keymap.prefix_label())).fg(theme::yellow()).bold(), Span::from("then ").fg(theme::muted())]);
    let block = Block::bordered()
        .title(title)
        .title_bottom(Line::from(" esc cancels · another key does what it does on its own ").fg(theme::dim()))
        .border_style(border(true))
        .style(Style::new().bg(theme::bg()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [columns, _, chords] = Layout::vertical([Constraint::Fill(1), Constraint::Length(1), Constraint::Length(1)]).areas(inner);
    let areas = Layout::horizontal(vec![Constraint::Fill(1); groups.len()]).split(columns);
    for ((title, rows), area) in groups.iter().zip(areas.iter()) {
        let mut lines = vec![Line::from(format!(" {title}")).fg(theme::dim()).bold()];
        lines.extend(rows.iter().map(|(keys, label)| Line::from(vec![Span::from(format!(" {keys:<6} ")).fg(theme::blue()).bold(), Span::from(*label)])));
        frame.render_widget(Paragraph::new(lines), *area);
    }
    let mut spans = vec![Span::from(" WITHOUT IT ").fg(theme::dim()).bold()];
    for (keys, label) in keymap.chord_rows() {
        spans.push(Span::from(format!(" {keys} ")).fg(theme::blue()).bold());
        spans.push(Span::from(format!("{label}  ")).fg(theme::muted()));
    }
    frame.render_widget(Line::from(spans), chords);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let (width, height) = (width.min(area.width), height.min(area.height));
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 3, width, height }
}

/// Go to, or the menu, over everything else.
fn draw_picker(frame: &mut Frame, app: &mut App) {
    let Some(picker) = &mut app.picker else { return };
    let area = frame.area();
    let popup = match picker.at {
        // Under the mouse, kept on screen.
        Some((x, y)) => {
            let (width, height) = (64.min(area.width), (picker.rows.len() as u16 + 5).min(area.height));
            Rect { x: x.min(area.right().saturating_sub(width)), y: y.min(area.bottom().saturating_sub(height)), width, height }
        }
        None => centered(area, (area.width * 7 / 10).max(60), (area.height * 7 / 10).max(12)),
    };
    frame.render_widget(Clear, popup);
    let goto = picker.kind == Kind::Goto;
    let (title, hint) = match picker.kind {
        Kind::Goto => ("Go to", " type to filter · ⇥ which terminals · ↑↓ move · ⏎ go there · esc close "),
        Kind::Menu => ("Menu", " type to filter · ↑↓ move · ⏎ do it · esc close "),
        Kind::Theme => ("Theme", " type to filter · ↑↓ try it · ⏎ keep it · esc put the old one back "),
    };
    let block = block(title, true).title_bottom(Line::from(hint).fg(theme::dim()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, list, _, detail] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Fill(1),
        Constraint::Length(u16::from(goto)),
        Constraint::Length(u16::from(goto)),
    ])
    .areas(inner);

    let mut spans = vec![Span::from(" › ").fg(theme::blue()).bold(), Span::from(picker.query.clone()), Span::from("█").fg(theme::blue()), Span::from("   ")];
    if goto {
        for show in Show::ALL {
            let chip = Span::from(format!(" {} ", show.label()));
            spans.push(if show == picker.show { chip.fg(theme::bg()).bg(theme::blue()).bold() } else { chip.fg(theme::muted()) });
            spans.push(Span::from(" "));
        }
    }
    spans.push(Span::from(format!("  {} of {}", picker.shown(), picker.entries.len())).fg(theme::dim()));
    frame.render_widget(Line::from(spans), input);

    let items: Vec<ListItem> = picker
        .rows
        .iter()
        .map(|row| match row {
            // The repository's name, then where it is.
            picker::Row::Heading(group) => {
                let (name, rest) = group.split_once("  ").unwrap_or((group, ""));
                ListItem::new(Line::from(vec![Span::from(format!(" {}", name.to_uppercase())).fg(theme::muted()).bold(), Span::from(format!("  {rest}")).fg(theme::dim())]))
            }
            picker::Row::Entry(i) => {
                let entry = &picker.entries[*i];
                let line = if goto {
                    let state = entry.state.unwrap_or(State::Idle);
                    let (icon, color) = state_icon(state);
                    let said = if matches!(state, State::Waiting | State::Done) { theme::magenta() } else { theme::muted() };
                    Line::from(vec![
                        Span::from(format!("   {icon} ")).fg(color).bold(),
                        Span::from(format!("{:<14} ", entry.text)).fg(theme::fg()),
                        Span::from(format!("{:<22} ", entry.place)).fg(theme::muted()),
                        Span::from(format!("{:>4}  ", entry.age)).fg(theme::dim()),
                        Span::from(entry.detail.replace('\n', " ")).fg(said),
                    ])
                } else {
                    Line::from(vec![Span::from(format!("   {:<52} ", entry.text)), Span::from(entry.place.clone()).fg(theme::blue()).bold()])
                };
                ListItem::new(line)
            }
        })
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), list, &mut picker.state);
    if goto && let Some(entry) = picker.selected() {
        frame.render_widget(Line::from(format!(" {}", entry.path)).fg(theme::muted()), detail);
    }
}

/// The search popup over everything else.
fn draw_search(frame: &mut Frame, app: &mut App) {
    let Some(search) = &mut app.search else { return };
    let area = frame.area();
    let popup = centered(area, (area.width * 7 / 10).max(40), (area.height * 7 / 10).max(10));
    frame.render_widget(Clear, popup);
    frame.render_widget(Block::new().style(Style::new().fg(theme::fg()).bg(theme::bg())), popup);
    let (title, hint) = match search.mode {
        Mode::Files => (
            " Files ",
            " tab: search inside files · ←→ fold · ⏎ open · ctrl-e edit · ctrl-y copy path · ctrl-x delete · esc close ",
        ),
        Mode::Text => (" Search inside files ", " tab: search file names · ↑↓ move · ⏎ open · esc close "),
        Mode::Repos => (" Add a workspace ", " git repositories in your home folder · type to filter, or a path (~/…) · ⏎ add · esc close "),
    };
    let block = block(title.trim(), true).title_bottom(Line::from(hint).fg(theme::dim()));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, results] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);

    let status = match search.mode {
        Mode::Repos if search.busy => "looking for git repositories in your home folder…".to_string(),
        _ if search.busy => "searching…".to_string(),
        Mode::Repos => format!("{} found", search.hits.len()),
        Mode::Text if search.query.chars().count() < 2 => "type at least 2 characters".to_string(),
        Mode::Files if search.query.is_empty() => format!("{} files · type to filter", search.file_count()),
        Mode::Files if search.hits.len() >= 500 => "500+ matches".to_string(),
        Mode::Files => format!("{} matches", search.hits.len()),
        Mode::Text if search.hits.len() >= 500 => "500+ matches".to_string(),
        Mode::Text => format!("{} matches", search.hits.len()),
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

    // The files: the workspace's tree, or the matches laid out as one.
    if search.mode == Mode::Files {
        let ws = &mut app.workspaces[app.current];
        let changed: Vec<(PathBuf, char)> = ws.status.changes.iter().map(|c| (ws.root.join(&c.path), c.kind())).collect();
        let list = |items| List::new(items).highlight_style(highlight(true));
        if search.query.is_empty() {
            frame.render_stateful_widget(list(file_items(&ws.tree.rows, &changed, false)), results, &mut ws.files_state);
        } else {
            frame.render_stateful_widget(list(file_items(&search.tree, &changed, true)), results, &mut search.state);
        }
        return;
    }

    let items: Vec<ListItem> = search
        .hits
        .iter()
        .map(|hit| match hit.line {
            Some(line) => ListItem::new(Line::from(vec![
                Span::from(format!(" {}", hit.rel)).fg(theme::muted()),
                Span::from(format!(":{}", line + 1)).fg(theme::yellow()),
                Span::from(format!("  {}", hit.text)),
            ])),
            None => {
                let path = Path::new(&hit.rel);
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let dir = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                ListItem::new(Line::from(vec![Span::from(format!(" {name}")).bold(), Span::from(format!("  {dir}")).fg(theme::muted())]))
            }
        })
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), results, &mut search.state);
}

fn block(title: &str, focused: bool) -> Block<'static> {
    Block::bordered()
        .border_type(BorderType::Rounded)
        .title(format!(" {title} "))
        .title_style(Style::new().fg(if focused { theme::fg() } else { theme::muted() }).bold())
        .border_style(border(focused))
        .style(Style::new().bg(theme::bg()))
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

/// gitDecoration colors.
fn kind_color(kind: char) -> Color {
    match kind {
        'A' | '?' => theme::green(),
        'D' => theme::red(),
        'R' | 'C' => theme::sky(),
        'U' => theme::yellow(),
        _ => theme::blue(),
    }
}
