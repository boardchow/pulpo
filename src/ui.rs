use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};

use crate::agents::State;
use crate::app::{App, Focus};
use crate::git::Change;
use crate::search::Mode;
use crate::workspace::GraphItem;
use crate::theme::{ACCENT, BG, BLUE, BORDER, DIM, EDITOR_BG, FG, GREEN, HOVER, MAGENTA, MUTED, RED, SELECTION, SKY, WHITE, YELLOW};

pub fn draw(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Block::new().style(Style::new().fg(FG).bg(BG)), frame.area());
    let [main, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let width = (main.width * 27 / 100).clamp(28, 42).min(main.width);
    let [sidebar, right] = Layout::horizontal([Constraint::Length(width), Constraint::Fill(1)]).areas(main);
    draw_sidebar(frame, app, sidebar);
    draw_right(frame, app, right);
    draw_footer(frame, app, footer);
    draw_search(frame, app);
    draw_help(frame, app);
}

/// The pulpo itself, drawn with block octants (Unicode 16): 2×4 pixels per
/// cell, so the pixels come out square and small.
const PULPO: [&str; 3] = [
    "𜺠𜷡𜷥██𜷤𜷞𜺣",
    "𜴡𜷤▆██▆𜷥𜴍",
    "𜴁𜴄🮂𜴁𜴄🮂𜴁𜴄",
];

/// The pulpo, and beside it the version, what the terminals are doing and
/// where the current workspace lives.
fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let art_width = PULPO[0].chars().count() as u16;
    let [art, text] = Layout::horizontal([Constraint::Length(art_width), Constraint::Fill(1)]).spacing(2).areas(area);
    let lines: Vec<Line> = PULPO.iter().map(|row| Line::from(*row)).collect();
    frame.render_widget(Paragraph::new(lines).fg(BLUE), art);

    let states: Vec<State> = app.terms.iter().map(|term| app.term_state(term)).collect();
    let count = |state: State| states.iter().filter(|&&s| s == state).count();
    let busy: Vec<String> = [(count(State::Waiting), "to read"), (count(State::Running), "working")]
        .into_iter()
        .filter(|&(n, _)| n > 0)
        .map(|(n, label)| format!("{n} {label}"))
        .collect();
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
    let width = text.width as usize;
    let length = path.chars().count();
    let path = if length > width && width > 0 { format!("…{}", path.chars().skip(length - width + 1).collect::<String>()) } else { path };

    let lines = vec![
        Line::from(vec![Span::from("Pulpo").fg(FG).bold(), Span::from(concat!(" v", env!("CARGO_PKG_VERSION"))).fg(MUTED)]),
        // Colored only when a terminal waits for you or works.
        Line::from(activity).fg(match (count(State::Waiting), count(State::Running)) {
            (0, 0) => MUTED,
            (0, _) => YELLOW,
            _ => MAGENTA,
        }),
        Line::from(path).fg(MUTED),
    ];
    frame.render_widget(Paragraph::new(lines), text);
}

fn draw_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let workspaces_height = app.repos().len().clamp(1, 8) as u16 + 2;
    let sessions_height = app.sessions().len().clamp(1, 6) as u16 + 2;
    let changes_height = app.ws().status.changes.len().clamp(1, 8) as u16 + 2;
    // Taller while you browse it, to see a commit's files.
    let graph_max = if app.focus == Focus::Graph { 20 } else { 10 };
    let graph_height = app.ws().graph_len().clamp(1, graph_max) as u16 + 2;
    // Files last: it is the one panel that takes whatever height is left.
    let [mascot, workspaces, sessions, changes, graph, files] = Layout::vertical([
        // A blank line above and below the pulpo.
        Constraint::Length(PULPO.len() as u16 + 2),
        Constraint::Length(workspaces_height),
        Constraint::Length(sessions_height),
        Constraint::Length(changes_height),
        Constraint::Length(graph_height),
        Constraint::Min(3),
    ])
    .areas(area);
    app.areas.workspaces = workspaces;
    app.areas.sessions = sessions;
    app.areas.changes = changes;
    app.areas.graph = graph;
    app.areas.files = files;

    draw_header(frame, app, mascot.inner(Margin::new(1, 1)));

    // Workspaces: the repositories, each with how its sessions are doing.
    let current_repo = app.ws().repo.clone();
    let items: Vec<ListItem> = app
        .repos()
        .into_iter()
        .map(|i| {
            let w = &app.workspaces[i];
            let current = w.repo == current_repo;
            let mut spans = vec![Span::from(if current { " ● " } else { "   " }).fg(ACCENT)];
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
    let list = List::new(items).block(block("Workspaces", focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, workspaces, &mut app.workspaces_state);

    // Sessions of this repository: the checkout itself, then its worktrees.
    // The current one holds the terminals, files and changes on screen.
    let focused = app.focus == Focus::Sessions;
    let items: Vec<ListItem> = app
        .sessions()
        .into_iter()
        .map(|i| {
            let w = &app.workspaces[i];
            let current = i == app.current;
            let (icon, color) = app.session_state(i).map_or((" ", DIM), state_icon);
            let name = Span::from(app.session_name(i).to_string());
            let name = match app.session_state(i) {
                Some(State::Waiting | State::Done) => name.fg(color).bold(),
                _ if current => name.fg(FG).bold(),
                _ => name.fg(MUTED),
            };
            let mut spans = vec![Span::from(format!(" {icon} ")).fg(color).bold(), name];
            if let Some(branch) = &w.status.branch {
                spans.push(Span::from(format!("  {branch}")).fg(MUTED));
            }
            if w.status.ahead > 0 {
                spans.push(Span::from(format!(" ↑{}", w.status.ahead)).fg(GREEN));
            }
            if w.status.behind > 0 {
                spans.push(Span::from(format!(" ↓{}", w.status.behind)).fg(RED));
            }
            if !w.status.changes.is_empty() {
                spans.push(Span::from(format!(" ±{}", w.status.changes.len())).fg(BLUE));
            }
            // An isolated session: where its app answers.
            if let Some(session) = app.isolated.iter().find(|s| s.root == w.root) {
                let web = session.ports.iter().filter(|(var, _)| ["APP", "WEB", "HTTP"].iter().any(|k| var.contains(k)));
                for (_, port) in web {
                    spans.push(Span::from(format!(" :{port}")).fg(SKY));
                }
            }
            ListItem::new(Line::from(spans))
        })
        .collect();
    let list = List::new(items).block(block("Sessions", focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, sessions, &mut app.sessions_state);

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
    let title = format!("Changes {}", ws.status.changes.len());
    let list = List::new(items).block(block(&title, focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, changes, &mut ws.changes_state);

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
            Span::from(ch.to_string()).fg(LANES[i / 2 % LANES.len()])
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
                    spans.push(Span::from(fold).fg(DIM));
                    if !commit.refs.is_empty() {
                        spans.push(Span::from(format!(" {}", commit.refs)).fg(SKY).bold());
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
    let list = List::new(items).block(block("Graph", focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, graph, &mut ws.graph_state);

    // Files, colored by git status like VS Code.
    let changed: Vec<(PathBuf, char)> = ws.status.changes.iter().map(|c| (ws.root.join(&c.path), c.kind())).collect();
    let items: Vec<ListItem> = ws
        .tree
        .rows
        .iter()
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
            let style = kind.map_or(Style::default(), |k| Style::default().fg(kind_color(k)));
            ListItem::new(Line::from(vec![
                Span::from(format!(" {}{icon}", "  ".repeat(row.depth))).fg(DIM),
                Span::styled(row.name.clone(), style),
            ]))
        })
        .collect();
    let focused = app.focus == Focus::Files;
    let list = List::new(items).block(block("Files", focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, files, &mut ws.files_state);
}

/// A changed file: its name in the color of the change, lines added and
/// removed, then its folder.
fn change_spans(c: &Change) -> Vec<Span<'static>> {
    let path = Path::new(&c.path);
    let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
    let dir = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
    let mut spans = vec![Span::from(name).fg(kind_color(c.kind()))];
    if c.added > 0 {
        spans.push(Span::from(format!(" +{}", c.added)).fg(GREEN));
    }
    if c.removed > 0 {
        spans.push(Span::from(format!(" −{}", c.removed)).fg(RED));
    }
    spans.push(Span::from(format!(" {dir}")).fg(MUTED));
    spans
}

/// How a terminal shows its state: a spinner while it works, a steady
/// colored mark until you read what it finished or asks, a dim one after.
fn state_icon(state: State) -> (&'static str, Color) {
    const SPINNER: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    match state {
        State::Running => (SPINNER[(now.as_millis() / 100) as usize % SPINNER.len()], YELLOW),
        State::Waiting | State::Done => ("◆", MAGENTA),
        State::Idle => ("○", DIM),
    }
}

/// Colors of the graph's lanes, left to right, like `git log --graph --color`.
const LANES: [Color; 5] = [BLUE, MAGENTA, GREEN, YELLOW, SKY];

/// The current terminal on top, the file viewer below when something is open.
fn draw_right(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.viewer.expanded && app.viewer.is_open() {
        // Terminals keep running, unseen and at their current size.
        app.areas.terminal = Rect::default();
        draw_viewer(frame, app, area);
        return;
    }
    // Zoomed: the terminal alone, the viewer stays open but hidden.
    let viewer_shown = app.viewer.is_open() && !(app.zoomed && app.active.is_some());
    let [top, bottom] = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
    let term_area = if viewer_shown { top } else { area };
    let inner = Block::bordered().inner(term_area);
    app.resize_terms(inner.height, inner.width);

    app.areas.terminal = Rect::default();
    app.areas.viewer = Rect::default();
    app.areas.ruler = Rect::default();
    match (app.active.is_none(), viewer_shown) {
        (true, true) => draw_viewer(frame, app, area),
        (_, true) => {
            draw_terminal(frame, app, top);
            draw_viewer(frame, app, bottom);
        }
        (_, false) => draw_terminal(frame, app, area),
    }
}

fn draw_terminal(frame: &mut Frame, app: &mut App, area: Rect) {
    let focused = app.focus == Focus::Terminal;
    // The session's terminals as tabs: the one on screen stands out.
    let mut spans = Vec::new();
    for (n, i) in app.session_terms().into_iter().enumerate() {
        let term = &app.terms[i];
        let (icon, color) = state_icon(app.term_state(term));
        let shown = app.active == Some(term.id);
        if n > 0 {
            spans.push(Span::from(" ·").fg(DIM));
        }
        spans.push(Span::from(format!(" {} ", n + 1)).fg(MUTED));
        spans.push(Span::from(format!("{icon} ")).fg(color).bold());
        let label = Span::from(term.label().to_string());
        spans.push(if shown { label.fg(FG).bold() } else { label.fg(MUTED) });
    }
    let title = if spans.is_empty() {
        Line::from(" Terminal ").fg(MUTED)
    } else {
        spans.push(Span::from(format!(" · {} ", app.session_name(app.current))).fg(DIM));
        Line::from(spans)
    };
    let block = Block::bordered().title(title).border_style(border(focused)).style(Style::new().bg(BG));
    let inner = block.inner(area);
    app.areas.terminal = area;
    frame.render_widget(block, area);

    match app.term() {
        Some(term) => {
            if let Some(cursor) = term.render(inner, frame.buffer_mut())
                && focused
            {
                frame.set_cursor_position(cursor);
            }
        }
        None => {
            let key = |k: &str, text: &str| Line::from(vec![Span::from(format!("  {k:<12}")).fg(BLUE).bold(), Span::from(text.to_string())]);
            let hint = vec![
                Line::default(),
                Line::from("  No terminals in this session.").bold(),
                Line::default(),
                key("s", "open a shell in it (then type claude for an agent)"),
            ];
            frame.render_widget(Paragraph::new(hint), inner);
        }
    }
}

fn draw_viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = block(&app.viewer.heading(), app.focus == Focus::Viewer).style(Style::new().bg(EDITOR_BG));
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
            cell.set_style(Style::new().fg(color.unwrap_or(EDITOR_BG)).bg(if in_view { BORDER } else { EDITOR_BG }));
        }
    }
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    if app.new_menu {
        let keys = [("w", "workspace"), ("s", "session"), ("t", "terminal"), ("f", "file or folder"), ("esc", "cancel")];
        frame.render_widget(Line::from(footer_spans("NEW", ACCENT, &keys, &[])), area);
        return;
    }
    if let Some(create) = app.creating.as_ref().filter(|c| c.session) {
        let mut spans = footer_spans("NEW SESSION", ACCENT, &[("⏎", "create"), ("esc", "cancel")], &[]);
        let input = vec![
            Span::from("› ").fg(BLUE).bold(),
            Span::from(create.name.clone()),
            Span::from("█").fg(BLUE),
            Span::from("   its own copy, Docker, ports and database   ").fg(MUTED),
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
        let mut spans = footer_spans("NEW FILE", ACCENT, &[("⏎", "create"), ("esc", "cancel")], &[]);
        let input = vec![
            Span::from("› ").fg(BLUE).bold(),
            Span::from(dir).fg(MUTED),
            Span::from(create.name.clone()),
            Span::from("█").fg(BLUE),
            Span::from("   end with / for a folder   ").fg(MUTED),
        ];
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    if let Some(message) = &app.message {
        frame.render_widget(Line::from(format!(" {message}")).fg(YELLOW).bold(), area);
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
        let mut input = vec![Span::from("› ").fg(BLUE).bold(), Span::from(find.query.clone())];
        if find.typing {
            input.push(Span::from("█").fg(BLUE));
        }
        input.push(Span::from(format!("   {count}   ")).fg(MUTED));
        let mut spans = footer_spans("FIND IN FILE", ACCENT, keys, &[]);
        spans.splice(2..2, input);
        frame.render_widget(Line::from(spans), area);
        return;
    }
    // Where you are, then the keys that work there, then the global ones.
    let (badge, keys): (&str, &[(&str, &str)]) = match app.focus {
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
        Focus::Terminal => (
            "TERMINAL",
            // ctrl-f only matters when there is a viewer to hide or show.
            match (app.viewer.is_open(), app.zoomed) {
                (true, true) => &[("ctrl-s", "sidebar"), ("ctrl-n", "new shell"), ("ctrl-q", "close"), ("ctrl-[ ]", "switch"), ("ctrl-f", "show viewer"), ("shift-⏎", "new line")],
                (true, false) => &[("ctrl-s", "sidebar"), ("ctrl-n", "new shell"), ("ctrl-q", "close"), ("ctrl-[ ]", "switch"), ("ctrl-f", "hide viewer"), ("shift-⏎", "new line")],
                _ => &[("ctrl-s", "sidebar"), ("ctrl-n", "new shell"), ("ctrl-q", "close"), ("ctrl-[ ]", "switch"), ("shift-⏎", "new line")],
            },
        ),
        Focus::Workspaces => ("WORKSPACES", &[("j/k", "switch"), ("x", "remove")]),
        Focus::Sessions => ("SESSIONS", &[("j/k", "switch"), ("⏎", "its terminal"), ("n t", "new terminal in it")]),
        Focus::Changes => ("CHANGES", &[("j/k", "move"), ("⏎", "read diff"), ("e", "edit"), ("y", "copy path")]),
        Focus::Graph => ("GRAPH", &[("j/k", "move"), ("⏎", "a commit's files / read a diff"), ("h/l", "fold")]),
        Focus::Files => ("FILES", &[("j/k", "move"), ("⏎", "open"), ("h/l", "fold"), ("e", "edit"), ("y", "copy path"), ("x", "delete")]),
        Focus::Viewer => match (app.viewer.is_diff(), app.viewer.expanded) {
            (true, true) => ("CHANGE", &[("n/p", "next/prev change"), ("j/k h/l", "scroll"), ("/", "find"), ("⏎", "split view"), ("esc", "back")]),
            (true, false) => ("CHANGE", &[("n/p", "next/prev change"), ("j/k h/l", "scroll"), ("/", "find"), ("⏎", "full height"), ("esc", "back")]),
            (false, true) => ("VIEWER", &[("j/k h/l", "scroll"), ("/", "find in file"), ("e", "edit"), ("⏎", "split view"), ("esc", "back")]),
            (false, false) => ("VIEWER", &[("j/k h/l", "scroll"), ("/", "find in file"), ("e", "edit"), ("⏎", "full height"), ("esc", "back")]),
        },
    };
    let global: &[(&str, &str)] = match app.focus {
        Focus::Terminal => &[],
        // In the viewer, / finds in the file instead.
        Focus::Viewer => &[("w t c f s", "go to panel"), ("?", "all keys"), ("q", "quit")],
        _ => &[("w t c g f s", "go to panel"), ("n", "new"), ("/", "search"), ("?", "all keys"), ("q", "quit")],
    };
    let color = if app.focus == Focus::Terminal { YELLOW } else { ACCENT };
    frame.render_widget(Line::from(footer_spans(badge, color, keys, global)), area);
}

fn footer_spans(badge: &str, color: Color, keys: &[(&str, &str)], global: &[(&str, &str)]) -> Vec<Span<'static>> {
    // Text on the indigo accent is white, on lighter badges dark; keys use the
    // lighter blue where the accent would be too dark to read.
    let (badge_fg, key) = if color == ACCENT { (WHITE, BLUE) } else { (BG, color) };
    let mut spans = vec![Span::styled(format!(" {badge} "), Style::default().fg(badge_fg).bg(color).bold()), Span::from(" ")];
    let key_color = key;
    for (i, (key, label)) in keys.iter().chain(global).enumerate() {
        if i == keys.len() && !global.is_empty() {
            spans.push(Span::from("│ ").fg(DIM));
        }
        spans.push(Span::from(key.to_string()).fg(key_color).bold());
        spans.push(Span::from(format!(" {label}  ")).fg(MUTED));
    }
    spans
}

const HELP: &[(&str, &[(&str, &str)])] = &[
    (
        "Go to a panel",
        &[
            ("w t c g f v", "Workspaces, Sessions, Changes, Graph, Files, Viewer (g: not from the viewer)"),
            ("s", "into the current shell (opens one if there is none)"),
            ("z / ctrl-f", "into the current shell, full height (hides the viewer)"),
            ("tab / shift-tab", "next / previous panel"),
        ],
    ),
    (
        "In a panel",
        &[
            ("n w  n t  n f", "new workspace / terminal in this session / file or folder (end the name with /)"),
            ("n s", "new session, with its own copy, Docker, ports and database"),
            ("e", "edit the file in micro (offers to install it), or $PULPO_EDITOR"),
            ("y", "copy the file's path, relative to the workspace"),
            ("x x", "remove a workspace from the list (files kept) / delete a file or a task session / close the terminal"),
            ("j k  ↑ ↓  ⏎", "move, open (a session's terminal, a file, a diff, a commit's files)"),
            ("h l", "fold / unfold a folder, or a commit's files in Graph"),
            ("1-9  [ ]", "enter the session's terminal N / previous, next terminal"),
            ("/  tab", "search files by name / tab: search inside files"),
            ("r", "refresh"),
            ("q q", "quit Pulpo; terminals keep running, and come back next time"),
            ("Q Q", "quit Pulpo and stop every terminal (ctrl-c, then hang up)"),
        ],
    ),
    (
        "Inside a terminal",
        &[
            ("ctrl-s", "back to the sidebar (ctrl-s or s to come back)"),
            ("ctrl-n", "new shell in this session"),
            ("ctrl-q", "close this shell (asks again if something is running in it)"),
            ("ctrl-[ ]  ctrl-1-9", "previous, next terminal of the session / terminal 1-9"),
            ("ctrl-f", "hide / show the viewer under the terminal"),
            ("shift-⏎", "new line in a Claude prompt"),
            ("claude", "type it in a shell to start an agent; the terminal is named after it"),
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
];

fn draw_help(frame: &mut Frame, app: &App) {
    if !app.help {
        return;
    }
    let mut lines = Vec::new();
    for (section, keys) in HELP {
        lines.push(Line::from(format!(" {section}")).bold());
        for (key, text) in *keys {
            lines.push(Line::from(vec![Span::from(format!("   {key:<16}")).fg(BLUE).bold(), Span::from(*text)]));
        }
        lines.push(Line::default());
    }
    let popup = centered(frame.area(), 100, lines.len() as u16 + 2);
    frame.render_widget(Clear, popup);
    frame.render_widget(Block::new().style(Style::new().fg(FG).bg(BG)), popup);
    let block = block("Keyboard shortcuts", true).title_bottom(Line::from(" any key to close ").fg(DIM));
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let (width, height) = (width.min(area.width), height.min(area.height));
    Rect { x: area.x + (area.width - width) / 2, y: area.y + (area.height - height) / 3, width, height }
}

/// The search popup over everything else.
fn draw_search(frame: &mut Frame, app: &mut App) {
    let Some(search) = &mut app.search else { return };
    let area = frame.area();
    let popup = centered(area, (area.width * 7 / 10).max(40), (area.height * 7 / 10).max(10));
    frame.render_widget(Clear, popup);
    frame.render_widget(Block::new().style(Style::new().fg(FG).bg(BG)), popup);
    let (title, hint) = match search.mode {
        Mode::Files => (" Search files ", " tab: search inside files · ↑↓ move · ⏎ open · esc close "),
        Mode::Text => (" Search inside files ", " tab: search file names · ↑↓ move · ⏎ open · esc close "),
        Mode::Repos => (" Add a workspace ", " git repositories in your home folder · type to filter, or a path (~/…) · ⏎ add · esc close "),
    };
    let block = block(title.trim(), true).title_bottom(Line::from(hint).fg(DIM));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    let [input, _, results] =
        Layout::vertical([Constraint::Length(1), Constraint::Length(1), Constraint::Min(1)]).areas(inner);

    let status = match search.mode {
        Mode::Repos if search.busy => "looking for git repositories in your home folder…".to_string(),
        _ if search.busy => "searching…".to_string(),
        Mode::Repos => format!("{} found", search.hits.len()),
        Mode::Text if search.query.chars().count() < 2 => "type at least 2 characters".to_string(),
        Mode::Files => format!("{} files", search.hits.len()),
        Mode::Text if search.hits.len() >= 500 => "500+ matches".to_string(),
        Mode::Text => format!("{} matches", search.hits.len()),
    };
    frame.render_widget(
        Line::from(vec![
            Span::from(" › ").fg(BLUE).bold(),
            Span::from(search.query.clone()),
            Span::from("█").fg(BLUE),
            Span::from(format!("   {status}")).fg(MUTED),
        ]),
        input,
    );

    let items: Vec<ListItem> = search
        .hits
        .iter()
        .map(|hit| match hit.line {
            Some(line) => ListItem::new(Line::from(vec![
                Span::from(format!(" {}", hit.rel)).fg(MUTED),
                Span::from(format!(":{}", line + 1)).fg(YELLOW),
                Span::from(format!("  {}", hit.text)),
            ])),
            None => {
                let path = Path::new(&hit.rel);
                let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                let dir = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
                ListItem::new(Line::from(vec![Span::from(format!(" {name}")).bold(), Span::from(format!("  {dir}")).fg(MUTED)]))
            }
        })
        .collect();
    frame.render_stateful_widget(List::new(items).highlight_style(highlight(true)), results, &mut search.state);
}

fn block(title: &str, focused: bool) -> Block<'static> {
    Block::bordered()
        .title(format!(" {title} "))
        .title_style(Style::new().fg(if focused { FG } else { MUTED }).bold())
        .border_style(border(focused))
        .style(Style::new().bg(BG))
}

fn border(focused: bool) -> Style {
    Style::default().fg(if focused { ACCENT } else { BORDER })
}

fn highlight(focused: bool) -> Style {
    if focused {
        Style::default().fg(FG).bg(SELECTION).add_modifier(Modifier::BOLD)
    } else {
        Style::default().bg(HOVER)
    }
}

/// gitDecoration colors.
fn kind_color(kind: char) -> Color {
    match kind {
        'A' | '?' => GREEN,
        'D' => RED,
        'R' | 'C' => SKY,
        'U' => YELLOW,
        _ => BLUE,
    }
}
