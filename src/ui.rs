use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, Paragraph};

use crate::agents::State;
use crate::app::{App, Focus};
use crate::search::Mode;
use crate::theme::{ACCENT, BG, BLUE, BORDER, DIM, EDITOR_BG, FG, GREEN, HOVER, MAGENTA, MUTED, RED, SELECTION, SKY, WHITE, YELLOW};

pub fn draw(frame: &mut Frame, app: &mut App) {
    frame.render_widget(Block::new().style(Style::new().fg(FG).bg(BG)), frame.area());
    let [main, footer] = Layout::vertical([Constraint::Min(1), Constraint::Length(1)]).areas(frame.area());
    let width = (main.width * 24 / 100).clamp(26, 38).min(main.width);
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

fn draw_sidebar(frame: &mut Frame, app: &mut App, area: Rect) {
    let workspaces_height = app.workspaces.len().clamp(1, 8) as u16 + 2;
    let sessions_height = app.terms.len().clamp(1, 6) as u16 + 2;
    let changes_height = app.ws().status.changes.len().clamp(1, 8) as u16 + 2;
    let [mascot, workspaces, sessions, changes, files] = Layout::vertical([
        // A blank line above and below the pulpo.
        Constraint::Length(PULPO.len() as u16 + 2),
        Constraint::Length(workspaces_height),
        Constraint::Length(sessions_height),
        Constraint::Length(changes_height),
        Constraint::Min(3),
    ])
    .areas(area);
    app.areas.workspaces = workspaces;
    app.areas.sessions = sessions;
    app.areas.changes = changes;
    app.areas.files = files;

    let art: Vec<Line> = PULPO.iter().map(|row| Line::from(*row)).collect();
    frame.render_widget(Paragraph::new(art).fg(BLUE), mascot.inner(Margin::new(1, 1)));

    // Workspaces: each repository followed by its worktrees.
    let items: Vec<ListItem> = app
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, w)| {
            let current = i == app.current;
            let mut spans = vec![Span::from(if current { " ● " } else { "   " }).fg(ACCENT)];
            if w.is_worktree() {
                spans.push(Span::from("└ ").fg(DIM));
            }
            let name = Span::from(w.name.clone());
            spans.push(if current { name.bold() } else { name });
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
            ListItem::new(Line::from(spans))
        })
        .collect();
    let focused = app.focus == Focus::Workspaces;
    let list = List::new(items).block(block("Workspaces", focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, workspaces, &mut app.workspaces_state);

    // Agents and terminals, across all workspaces.
    let focused = app.focus == Focus::Sessions;
    let sessions_block = block("Terminals", focused);
    if app.terms.is_empty() {
        let hint = Line::from(" n: new shell").fg(MUTED);
        frame.render_widget(Paragraph::new(hint).block(sessions_block), sessions);
    } else {
        let items: Vec<ListItem> = app
            .terms
            .iter()
            .enumerate()
            .map(|(i, term)| {
                let (state, detail) = app.term_state(term);
                let (icon, color) = match state {
                    State::Running => ("●", YELLOW),
                    State::Waiting => ("?", MAGENTA),
                    State::Done => ("✓", GREEN),
                    State::Idle => ("○", DIM),
                };
                let mut spans = vec![
                    Span::from(format!(" {} ", i + 1)).fg(MUTED),
                    Span::from(format!("{icon} ")).fg(color).bold(),
                    Span::from(term.label().to_string()),
                ];
                if let Some(ws) = app.workspace_name(&term.cwd) {
                    spans.push(Span::from(format!(" {ws}")).fg(MUTED));
                }
                if term.bell {
                    spans.push(Span::from(" !").fg(MAGENTA).bold());
                }
                if !detail.is_empty() {
                    spans.push(Span::from(format!("  {detail}")).fg(color));
                }
                ListItem::new(Line::from(spans))
            })
            .collect();
        let list = List::new(items).block(sessions_block).highlight_style(highlight(focused));
        frame.render_stateful_widget(list, sessions, &mut app.sessions_state);
    }

    let current = app.current;
    let ws = &mut app.workspaces[current];

    // Changes in the current workspace.
    let items: Vec<ListItem> = ws
        .status
        .changes
        .iter()
        .map(|c| {
            let path = Path::new(&c.path);
            let name = path.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let dir = path.parent().map(|p| p.display().to_string()).unwrap_or_default();
            let code: String = c.code.iter().map(|&ch| if ch == '.' { ' ' } else { ch }).collect();
            let mut spans = vec![
                Span::from(format!(" {code} ")).fg(kind_color(c.kind())).bold(),
                Span::from(name).fg(kind_color(c.kind())),
            ];
            if c.added > 0 {
                spans.push(Span::from(format!(" +{}", c.added)).fg(GREEN));
            }
            if c.removed > 0 {
                spans.push(Span::from(format!(" −{}", c.removed)).fg(RED));
            }
            spans.push(Span::from(format!(" {dir}")).fg(MUTED));
            ListItem::new(Line::from(spans))
        })
        .collect();
    let focused = app.focus == Focus::Changes;
    let title = format!("Changes {}", ws.status.changes.len());
    let list = List::new(items).block(block(&title, focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, changes, &mut ws.changes_state);

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
    let title = format!("Files · {}", ws.name);
    let list = List::new(items).block(block(&title, focused)).highlight_style(highlight(focused));
    frame.render_stateful_widget(list, files, &mut ws.files_state);
}

/// Terminal tabs on top, the file viewer below when something is open.
fn draw_right(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.viewer.expanded && app.viewer.is_open() {
        // Terminals keep running, unseen and at their current size.
        app.areas.terminal = Rect::default();
        draw_viewer(frame, app, area);
        return;
    }
    // Zoomed: the terminal alone, the viewer stays open but hidden.
    let viewer_shown = app.viewer.is_open() && !(app.zoomed && !app.terms.is_empty());
    let [top, bottom] = Layout::vertical([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);
    let term_area = if viewer_shown { top } else { area };
    let inner = Block::bordered().inner(term_area);
    app.resize_terms(inner.height, inner.width);

    app.areas.terminal = Rect::default();
    app.areas.viewer = Rect::default();
    match (app.terms.is_empty(), viewer_shown) {
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
    let mut tabs = vec![Span::from(" ")];
    let mut x = area.x + 2;
    app.tab_hits.clear();
    for (i, term) in app.terms.iter().enumerate() {
        let label = Span::from(format!(" {} {} ", i + 1, term.label()));
        let width = label.width() as u16;
        app.tab_hits.push((x, x + width.saturating_sub(1), i));
        x += width + 1;
        tabs.push(if i == app.active { label.fg(WHITE).bg(SELECTION).bold() } else { label.fg(MUTED) });
        tabs.push(Span::from(" "));
    }
    if app.terms.is_empty() {
        tabs.push(Span::from("Terminal "));
    }
    let block = Block::bordered().title(Line::from(tabs)).border_style(border(focused)).style(Style::new().bg(BG));
    let inner = block.inner(area);
    app.areas.terminal = area;
    frame.render_widget(block, area);

    match app.terms.get(app.active) {
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
                Line::from("  No terminals open.").bold(),
                Line::default(),
                key("n", "open a shell in this workspace (then type claude for an agent)"),
                key("ctrl-s", "switch between the sidebar and the terminal"),
            ];
            frame.render_widget(Paragraph::new(hint), inner);
        }
    }
}

fn draw_viewer(frame: &mut Frame, app: &mut App, area: Rect) {
    let block = block(&app.viewer.heading(), app.focus == Focus::Viewer).style(Style::new().bg(EDITOR_BG));
    let inner = block.inner(area);
    app.viewer.height = inner.height as usize;
    app.viewer.set_width(inner.width);
    if let Some(line) = app.viewer.goto.take() {
        app.viewer.scroll = line.saturating_sub(inner.height as usize / 3);
    }
    app.viewer.scroll_by(0);
    app.areas.viewer = area;
    let (scroll, end) = (app.viewer.scroll, (app.viewer.scroll + inner.height as usize).min(app.viewer.row_count()));
    let lines: Vec<Line> = (scroll..end).map(|i| app.viewer.decorated(i, inner.width)).collect();
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
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
        Focus::Terminal => (
            "TERMINAL",
            // ctrl-f only matters when there is a viewer to hide or show.
            match (app.viewer.is_open(), app.zoomed) {
                (true, true) => &[("ctrl-s", "sidebar"), ("ctrl-n", "new shell"), ("ctrl-q", "close"), ("ctrl-1-9", "switch"), ("ctrl-f", "show viewer"), ("shift-⏎", "new line")],
                (true, false) => &[("ctrl-s", "sidebar"), ("ctrl-n", "new shell"), ("ctrl-q", "close"), ("ctrl-1-9", "switch"), ("ctrl-f", "hide viewer"), ("shift-⏎", "new line")],
                _ => &[("ctrl-s", "sidebar"), ("ctrl-n", "new shell"), ("ctrl-q", "close"), ("ctrl-1-9", "switch"), ("shift-⏎", "new line")],
            },
        ),
        Focus::Workspaces => ("WORKSPACES", &[("j/k", "switch"), ("n", "new workspace"), ("x", "remove")]),
        Focus::Sessions => ("TERMINALS", &[("j/k", "pick"), ("⏎", "enter"), ("n", "new shell"), ("x", "close")]),
        Focus::Changes => ("CHANGES", &[("j/k", "move"), ("⏎", "read diff"), ("n", "new shell")]),
        Focus::Files => ("FILES", &[("j/k", "move"), ("⏎", "open"), ("h/l", "fold"), ("n", "new shell")]),
        Focus::Viewer => match (app.viewer.is_diff(), app.viewer.expanded) {
            (true, true) => ("CHANGE", &[("n/p", "next/prev change"), ("j/k h/l", "scroll"), ("/", "find"), ("⏎", "split view"), ("esc", "back")]),
            (true, false) => ("CHANGE", &[("n/p", "next/prev change"), ("j/k h/l", "scroll"), ("/", "find"), ("⏎", "full height"), ("esc", "back")]),
            (false, true) => ("VIEWER", &[("j/k h/l", "scroll"), ("/", "find in file"), ("⏎", "split view"), ("esc", "back")]),
            (false, false) => ("VIEWER", &[("j/k h/l", "scroll"), ("/", "find in file"), ("⏎", "full height"), ("esc", "back")]),
        },
    };
    let global: &[(&str, &str)] = match app.focus {
        Focus::Terminal => &[],
        // In the viewer, / finds in the file instead.
        Focus::Viewer => &[("w t c f s", "go to panel"), ("?", "all keys"), ("q", "quit")],
        _ => &[("w t c f s", "go to panel"), ("/", "search"), ("?", "all keys"), ("q", "quit")],
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
            ("w t c f v", "Workspaces, Terminals, Changes, Files, Viewer"),
            ("s", "into the current shell (opens one if there is none)"),
            ("z / ctrl-f", "into the current shell, full height (hides the viewer)"),
            ("tab / shift-tab", "next / previous panel"),
        ],
    ),
    (
        "In a panel",
        &[
            ("n", "new: a workspace in Workspaces, a shell anywhere else"),
            ("x x", "remove a workspace from the list (files kept) / close the terminal"),
            ("j k  ↑ ↓  ⏎", "move, open (a terminal, a file, a diff)"),
            ("h l", "fold / unfold a folder"),
            ("1-9  [ ]", "enter terminal N / previous, next terminal"),
            ("/  tab", "search files by name / tab: search inside files"),
            ("r", "refresh"),
            ("q q", "quit pulpo (closes all terminals)"),
        ],
    ),
    (
        "Inside a terminal",
        &[
            ("ctrl-s", "back to the sidebar (ctrl-s or s to come back)"),
            ("ctrl-n", "new shell"),
            ("ctrl-q", "close this shell (asks again if something is running in it)"),
            ("ctrl-1-9", "switch to terminal 1-9"),
            ("ctrl-f", "hide / show the viewer under the terminal"),
            ("shift-⏎", "new line in a Claude prompt"),
            ("claude", "type it in a shell to start an agent; the tab is named after it"),
        ],
    ),
    (
        "In the viewer",
        &[
            ("⏎ / esc", "full height or split view / back to where you were"),
            ("/  n p", "find in this file / next, previous match (or change, in a diff)"),
            ("d u  g G", "page down, up / top, bottom"),
            ("h l  ← →  0", "scroll sideways / back to the start (also shift + wheel, trackpad)"),
            ("click", "focus a panel, pick an item, switch tab; shift-drag selects text"),
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
