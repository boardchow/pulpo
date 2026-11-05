use std::collections::VecDeque;
use std::io::Write;
use std::num::NonZeroU16;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::agents::State;
use crate::client::Link;
use crate::proto::{Info, ToServer};
use crate::status::{self, Records, Status};
use crate::theme;

pub const SCROLLBACK: usize = 5000;

#[derive(Default)]
pub struct Events {
    pub replies: Vec<u8>,
    pub bell: bool,
    pub title: Option<String>,
    pub host: Vec<u8>,
    pub status: Records,
    /// Lines scrolled off the screen, as text: vt100 keeps 32 bytes a character.
    pub history: VecDeque<Box<str>>,
    /// Progress (OSC 9;4) stands in for a status until a real one arrives.
    reports_status: bool,
}

impl Events {
    pub fn with_status(status: Records) -> Events {
        Events { reports_status: true, status, ..Events::default() }
    }
}

impl vt100::Callbacks for Events {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(String::from_utf8_lossy(title).into_owned());
    }

    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        self.host.extend([&b"\x1b]52;"[..], ty, b";", data, b"\x07"].concat());
    }

    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            [b"7501", b"?"] => self.replies.extend(b"\x1b]7501;?\x1b\\"),
            [b"7501", body] => {
                if let Some(report) = status::parse(body) {
                    self.reports_status = true;
                    self.status.apply(report);
                }
            }
            [b"9", b"4", state, ..] if !self.reports_status => {
                let state = match *state {
                    b"0" => "clear",
                    b"2" => "error",
                    _ => "working",
                };
                if let Some(report) = status::parse(format!("state={state}").as_bytes()) {
                    self.status.apply(report);
                }
            }
            // A new shell prompt: whatever ran is over.
            [b"133", mark, ..] if mark.starts_with(b"A") => self.status.end_runs(),
            _ => {}
        }
    }

    // Answer cursor position and device attribute queries; some programs
    // wait for these before drawing.
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        i1: Option<u8>,
        _: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        match (i1, c) {
            (None, 'n') if params.first() == Some(&&[6][..]) => {
                let (row, col) = screen.cursor_position();
                self.replies.extend(format!("\x1b[{};{}R", row + 1, col + 1).bytes());
            }
            (None, 'c') => self.replies.extend(b"\x1b[?62;22c"),
            _ => {}
        }
    }

    fn on_scroll(&mut self, row: vt100::capture::RowContents<'_>, alternate_screen: bool) {
        if alternate_screen {
            return;
        }
        let mut line = String::new();
        let _ = row.write_formatted_basic(&mut line, &mut vt100::capture::BasicFormattedCaptureState::new());
        if self.history.len() >= SCROLLBACK {
            self.history.pop_front();
        }
        self.history.push_back(line.into());
    }
}

/// A terminal that keeps its history in `Events`.
pub fn parser(rows: u16, cols: u16, events: Events) -> vt100::Parser<Events> {
    vt100::Parser::new_with_callbacks(nz(rows), nz(cols), 0, events)
}

/// vt100 only clears the history it keeps itself.
pub fn feed(parser: &mut vt100::Parser<Events>, mut bytes: &[u8]) {
    const CLEAR: &[u8] = b"\x1b[3J";
    while let Some(at) = bytes.windows(CLEAR.len()).position(|w| w == CLEAR) {
        parser.process(&bytes[..at + CLEAR.len()]);
        parser.callbacks_mut().history.clear();
        bytes = &bytes[at + CLEAR.len()..];
    }
    parser.process(bytes);
}

/// The mirror only draws: the server answers the program's queries.
struct Mirror;

impl vt100::Callbacks for Mirror {
    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        to_host(&[&b"\x1b]52;"[..], ty, b";", data, b"\x07"].concat());
    }
}

pub struct Term {
    pub id: usize,
    pub info: Info,
    pub hook_unread: bool,
    pub reported: State,
    pub notice: Option<String>,
    pub last_notice: Option<Instant>,
    /// Only while it's watched, from its snapshot on.
    parser: Option<vt100::Parser<Mirror>>,
    /// The size of the snapshot that's the next output.
    snapshot: Option<(u16, u16)>,
    link: Link,
    asked: (u16, u16),
}

impl Term {
    pub fn new(info: Info, (rows, cols): (u16, u16), link: Link) -> Term {
        Term {
            id: info.id as usize,
            info,
            hook_unread: false,
            reported: State::Idle,
            notice: None,
            last_notice: None,
            parser: None,
            snapshot: None,
            link,
            asked: (rows, cols),
        }
    }

    /// The output frame that follows is its snapshot.
    pub fn mirror(&mut self, rows: u16, cols: u16) {
        self.parser = None;
        self.snapshot = Some((rows, cols));
    }

    pub fn unmirror(&mut self) {
        self.parser = None;
        self.snapshot = None;
    }

    pub fn mirrored(&self) -> bool {
        self.parser.is_some()
    }

    fn screen(&self) -> Option<&vt100::Screen> {
        self.parser.as_ref().map(vt100::Parser::screen)
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        if let Some((rows, cols)) = self.snapshot.take() {
            self.parser = Some(vt100::Parser::new_with_callbacks(nz(rows), nz(cols), SCROLLBACK, Mirror));
        }
        if let Some(parser) = &mut self.parser {
            parser.process(bytes);
        }
    }

    pub fn set_info(&mut self, info: Info) {
        let before = self.info.status.as_ref().map(|(status, _)| *status);
        if let Some((now, detail)) = &info.status
            && Some(*now) != before
            && matches!(now, Status::Blocked | Status::Error | Status::Done)
        {
            self.notice = Some(detail.clone());
        }
        self.info = info;
    }

    pub fn set_size(&mut self, rows: u16, cols: u16) {
        if let Some(parser) = &mut self.parser {
            parser.set_size(nz(rows), nz(cols));
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.asked != (rows, cols) {
            self.asked = (rows, cols);
            self.link.send(&ToServer::Resize { id: self.info.id, rows, cols });
        }
    }

    pub fn seen(&mut self) {
        self.hook_unread = false;
        self.notice = None;
        if self.info.unread {
            self.info.unread = false;
            self.link.send(&ToServer::Seen { id: self.info.id });
        }
    }

    pub fn unread(&self) -> bool {
        self.info.unread || self.hook_unread
    }

    pub fn working(&self) -> bool {
        self.info.working
    }

    pub fn status(&self) -> Option<(Status, String)> {
        self.info.status.clone()
    }

    pub fn label(&self) -> &str {
        self.info.running.as_deref().unwrap_or(&self.info.name)
    }

    pub fn key(&mut self, key: KeyEvent) {
        if let Some(bytes) = encode_key(key, self.screen().is_some_and(vt100::Screen::application_cursor)) {
            self.send(&bytes);
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.screen().is_some_and(vt100::Screen::bracketed_paste) {
            self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        } else {
            self.send(text.as_bytes());
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        if let Some(parser) = &mut self.parser {
            parser.screen_mut().set_scrollback(0);
        }
        self.link.input(self.info.id, bytes);
    }

    pub fn wants_mouse(&self) -> bool {
        self.screen().is_some_and(|screen| screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None)
    }

    pub fn alternate_screen(&self) -> bool {
        self.screen().is_some_and(vt100::Screen::alternate_screen)
    }

    pub fn mouse(&mut self, kind: MouseEventKind, col: u16, row: u16, modifiers: KeyModifiers) -> bool {
        let Some(screen) = self.screen() else { return false };
        match encode_mouse(screen.mouse_protocol_mode(), screen.mouse_protocol_encoding(), kind, col, row, modifiers) {
            Some(bytes) => {
                self.link.input(self.info.id, &bytes);
                true
            }
            None => false,
        }
    }

    pub fn word_at(&self, row: u16, col: u16) -> String {
        let Some(screen) = self.screen() else { return String::new() };
        let line: Vec<char> = screen.rows(0, screen.size().1.get()).nth(row as usize).unwrap_or_default().chars().collect();
        let col = col as usize;
        if col >= line.len() || line[col].is_whitespace() {
            return String::new();
        }
        let stop = |c: &char| c.is_whitespace() || "\"'`<>()[]{}│".contains(*c);
        let start = line[..col].iter().rposition(stop).map_or(0, |i| i + 1);
        let end = line[col..].iter().position(stop).map_or(line.len(), |i| col + i);
        line[start..end].iter().collect::<String>().trim_end_matches(['.', ',', ';', ':']).to_string()
    }

    pub fn selection(&self, a: (u16, u16), b: (u16, u16)) -> String {
        let (start, end) = if a <= b { (a, b) } else { (b, a) };
        let Some(screen) = self.screen() else { return String::new() };
        let text = screen.contents_between(start.0, start.1, end.0, end.1 + 1);
        text.lines().map(str::trim_end).collect::<Vec<_>>().join("\n")
    }

    pub fn scroll(&mut self, rows: isize) {
        if let Some(parser) = &mut self.parser {
            let screen = parser.screen_mut();
            screen.set_scrollback(screen.scrollback().saturating_add_signed(rows));
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
        let screen = self.screen()?;
        for row in 0..area.height {
            for col in 0..area.width {
                let Some(cell) = screen.cell(row, col) else { continue };
                if cell.is_wide_continuation() {
                    continue;
                }
                let Some(target) = buf.cell_mut((area.x + col, area.y + row)) else { continue };
                target.set_symbol(if cell.has_contents() { cell.contents() } else { " " });
                let mut modifier = Modifier::empty();
                for (on, m) in [
                    (cell.bold(), Modifier::BOLD),
                    (cell.dim(), Modifier::DIM),
                    (cell.italic(), Modifier::ITALIC),
                    (cell.underline(), Modifier::UNDERLINED),
                    (cell.inverse(), Modifier::REVERSED),
                ] {
                    if on {
                        modifier |= m;
                    }
                }
                target.set_style(
                    Style::default()
                        .fg(color(cell.fgcolor(), theme::fg()))
                        .bg(color(cell.bgcolor(), theme::bg()))
                        .add_modifier(modifier),
                );
            }
        }
        let (row, col) = screen.cursor_position();
        (!screen.hide_cursor() && screen.scrollback() == 0 && row < area.height && col < area.width)
            .then(|| (area.x + col, area.y + row))
    }
}

/// vt100 can't read the screen behind an alternate one; that part is left out.
pub fn snapshot(parser: &vt100::Parser<Events>, title: &str) -> Vec<u8> {
    let screen = parser.screen();
    let history = &parser.callbacks().history;
    let mut out = String::with_capacity(history.iter().map(|line| line.len() + 8).sum::<usize>() + 4096);
    if !history.is_empty() {
        for line in history {
            out.push_str("\x1b[m");
            out.push_str(line);
            out.push_str("\x1b[m\r\n");
        }
        // Scroll those lines off the screen into the history, so drawing
        // the screen over them doesn't lose them.
        out.push_str(&"\n".repeat(usize::from(screen.size().0.get()) - 1));
    }
    if screen.alternate_screen() {
        out.push_str("\x1b[?1049h");
    }
    out.push_str(&screen.state_formatted());
    if !title.is_empty() {
        out.push_str(&format!("\x1b]2;{title}\x07"));
    }
    out.into_bytes()
}

pub fn to_host(bytes: &[u8]) {
    if !bytes.is_empty() {
        let mut out = std::io::stdout();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }
}

/// Its `argv[0]`, as `ps -o comm=` shows it, without starting `ps`: the server asks every second.
/// Not the executable: Claude's runs from a file named after its version.
pub fn process_name(pid: u32) -> Option<String> {
    let pid = i32::try_from(pid).ok()?;
    // A program running as root, like sudo, doesn't show its arguments: its executable, then.
    let name = argv0(pid).or_else(|| executable(pid))?;
    let name = name.rsplit('/').next()?.trim_start_matches('-');
    (!name.is_empty()).then(|| name.to_string())
}

fn argv0(pid: i32) -> Option<String> {
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size = 0;
    let mut sysctl = |buf: *mut libc::c_void, size: &mut usize| unsafe { libc::sysctl(mib.as_mut_ptr(), 3, buf, size, std::ptr::null_mut(), 0) };
    if sysctl(std::ptr::null_mut(), &mut size) != 0 {
        return None;
    }
    let mut buf = vec![0u8; size];
    if sysctl(buf.as_mut_ptr().cast(), &mut size) != 0 {
        return None;
    }
    // argc, the executable's path, padding, then the arguments.
    let rest = buf.get(size_of::<i32>()..size)?;
    let path_end = rest.iter().position(|&b| b == 0)?;
    let argv0 = rest[path_end..].split(|&b| b == 0).find(|arg| !arg.is_empty())?;
    Some(String::from_utf8_lossy(argv0).into_owned())
}

fn executable(pid: i32) -> Option<String> {
    let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    (n > 0).then(|| String::from_utf8_lossy(&buf[..n as usize]).into_owned())
}

pub fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

fn color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) if i < 16 => theme::ansi()[i as usize],
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

fn encode_mouse(
    mode: vt100::MouseProtocolMode,
    encoding: vt100::MouseProtocolEncoding,
    kind: MouseEventKind,
    col: u16,
    row: u16,
    modifiers: KeyModifiers,
) -> Option<Vec<u8>> {
    use vt100::{MouseProtocolEncoding as Encoding, MouseProtocolMode as Mode};
    let button = |b: MouseButton| match b {
        MouseButton::Left => 0,
        MouseButton::Middle => 1,
        MouseButton::Right => 2,
    };
    let (code, release, motion) = match kind {
        MouseEventKind::Down(b) => (button(b), false, false),
        MouseEventKind::Up(b) => (button(b), true, false),
        MouseEventKind::Drag(b) => (button(b) + 32, false, true),
        MouseEventKind::ScrollUp => (64, false, false),
        MouseEventKind::ScrollDown => (65, false, false),
        _ => return None,
    };
    let wanted = match mode {
        Mode::None => false,
        Mode::Press => !release && !motion,
        Mode::PressRelease => !motion,
        Mode::ButtonMotion | Mode::AnyMotion => true,
    };
    if !wanted {
        return None;
    }
    let mut code = code;
    for (modifier, bit) in [(KeyModifiers::SHIFT, 4), (KeyModifiers::ALT, 8), (KeyModifiers::CONTROL, 16)] {
        if modifiers.contains(modifier) {
            code += bit;
        }
    }
    match encoding {
        Encoding::Sgr => Some(format!("\x1b[<{code};{col};{row}{}", if release { 'm' } else { 'M' }).into_bytes()),
        // The older encodings send a release as button 3 and each value + 32.
        Encoding::Default | Encoding::Utf8 => {
            let code = if release { 3 + (code & !3) } else { code };
            let mut out = b"\x1b[M".to_vec();
            for value in [code, col, row] {
                let value = u32::from(value) + 32;
                match encoding {
                    Encoding::Utf8 => out.extend(char::from_u32(value)?.to_string().bytes()),
                    _ => out.push(u8::try_from(value).ok()?),
                }
            }
            Some(out)
        }
    }
}

fn encode_key(key: KeyEvent, app_cursor: bool) -> Option<Vec<u8>> {
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    // xterm modifier parameter for special keys: 1 + shift + 2*alt + 4*ctrl.
    let m = 1 + shift as u8 + 2 * alt as u8 + 4 * ctrl as u8;
    let esc = if alt { "\x1b" } else { "" };
    let cursor = |c: char| {
        if m > 1 {
            format!("\x1b[1;{m}{c}")
        } else if app_cursor {
            format!("\x1bO{c}")
        } else {
            format!("\x1b[{c}")
        }
    };
    let tilde = |n: u8| if m > 1 { format!("\x1b[{n};{m}~") } else { format!("\x1b[{n}~") };

    let seq = match key.code {
        KeyCode::Char(c) if ctrl => {
            let byte = match c {
                'a'..='z' => c as u8 - b'a' + 1,
                'A'..='Z' => c as u8 - b'A' + 1,
                ' ' | '@' | '2' => 0,
                '[' | '3' => 0x1b,
                '\\' | '4' => 0x1c,
                ']' | '5' => 0x1d,
                '^' | '6' => 0x1e,
                '_' | '-' | '7' => 0x1f,
                '?' | '8' => 0x7f,
                _ => return None,
            };
            return Some([esc.as_bytes(), &[byte]].concat());
        }
        KeyCode::Char(c) => format!("{esc}{c}"),
        // Shift+Enter is sent the way Claude Code reads a new line.
        KeyCode::Enter if shift => "\x1b\r".into(),
        KeyCode::Enter => format!("{esc}\r"),
        KeyCode::Tab if shift => "\x1b[Z".into(),
        KeyCode::Tab => "\t".into(),
        KeyCode::BackTab => "\x1b[Z".into(),
        KeyCode::Backspace => format!("{esc}\x7f"),
        KeyCode::Esc => "\x1b".into(),
        KeyCode::Up => cursor('A'),
        KeyCode::Down => cursor('B'),
        KeyCode::Right => cursor('C'),
        KeyCode::Left => cursor('D'),
        KeyCode::Home => cursor('H'),
        KeyCode::End => cursor('F'),
        KeyCode::Insert => tilde(2),
        KeyCode::Delete => tilde(3),
        KeyCode::PageUp => tilde(5),
        KeyCode::PageDown => tilde(6),
        KeyCode::F(n @ 1..=4) => format!("\x1bO{}", (b'P' + n - 1) as char),
        KeyCode::F(n @ 5..=12) => tilde([15, 17, 18, 19, 20, 21, 23, 24][n as usize - 5]),
        _ => return None,
    };
    Some(seq.into_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> Vec<u8> {
        encode_key(KeyEvent::new(code, modifiers), false).unwrap()
    }

    #[test]
    fn encodes_keys_like_xterm() {
        assert_eq!(key(KeyCode::Char('a'), KeyModifiers::NONE), b"a");
        assert_eq!(key(KeyCode::Char('c'), KeyModifiers::CONTROL), b"\x03");
        assert_eq!(key(KeyCode::Backspace, KeyModifiers::ALT), b"\x1b\x7f");
        assert_eq!(key(KeyCode::Enter, KeyModifiers::SHIFT), b"\x1b\r");
        assert_eq!(key(KeyCode::Up, KeyModifiers::NONE), b"\x1b[A");
        assert_eq!(key(KeyCode::Left, KeyModifiers::ALT), b"\x1b[1;3D");
        assert_eq!(key(KeyCode::Delete, KeyModifiers::NONE), b"\x1b[3~");
        assert_eq!(key(KeyCode::F(5), KeyModifiers::NONE), b"\x1b[15~");
    }

    #[test]
    fn encodes_mouse_events_as_requested() {
        use vt100::{MouseProtocolEncoding as E, MouseProtocolMode as M};
        let none = KeyModifiers::NONE;
        let wheel = encode_mouse(M::PressRelease, E::Sgr, MouseEventKind::ScrollUp, 10, 5, none);
        assert_eq!(wheel.unwrap(), b"\x1b[<64;10;5M");
        let release = encode_mouse(M::PressRelease, E::Sgr, MouseEventKind::Up(MouseButton::Left), 1, 2, none);
        assert_eq!(release.unwrap(), b"\x1b[<0;1;2m");
        let legacy = encode_mouse(M::Press, E::Default, MouseEventKind::Down(MouseButton::Left), 1, 1, none);
        assert_eq!(legacy.unwrap(), b"\x1b[M !!");
        assert!(encode_mouse(M::None, E::Sgr, MouseEventKind::ScrollUp, 1, 1, none).is_none());
        assert!(encode_mouse(M::PressRelease, E::Sgr, MouseEventKind::Drag(MouseButton::Left), 1, 1, none).is_none());
    }

    #[test]
    fn answers_cursor_position_queries() {
        let mut parser = vt100::Parser::new_with_callbacks(nz(10), nz(20), 0, Events::default());
        parser.process(b"ab\x1b[6n");
        assert_eq!(parser.callbacks().replies, b"\x1b[1;3R");
    }

    #[test]
    fn snapshots_rebuild_the_same_terminal() {
        let mut parser = parser(10, 30, Events::default());
        for i in 0..45 {
            feed(&mut parser, format!("\x1b[3{}mline {i}\x1b[m\r\n", i % 8).as_bytes());
        }
        feed(&mut parser, b"\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1h$ half typed\x1b[4D");
        let screen = parser.screen();
        assert_eq!(parser.callbacks().history.len(), 36);

        let mut copy = vt100::Parser::new(nz(10), nz(30), 100);
        copy.process(&snapshot(&parser, "work"));
        let other = copy.screen();
        assert_eq!(other.contents(), screen.contents());
        assert_eq!(other.contents_formatted(), screen.contents_formatted());
        assert_eq!(other.cursor_position(), screen.cursor_position());
        assert!(other.bracketed_paste() && other.application_cursor());
        assert_eq!(other.mouse_protocol_mode(), screen.mouse_protocol_mode());
        assert_eq!(other.mouse_protocol_encoding(), screen.mouse_protocol_encoding());
        let mut back = other.clone();
        back.set_scrollback(usize::MAX);
        assert_eq!(back.scrollback(), 36);
        assert_eq!(back.rows_formatted(0, 30).next().unwrap(), "\x1b[30mline 0");

        // An upgraded server reads its history back from the snapshot.
        let mut resumed = super::parser(10, 30, Events::default());
        feed(&mut resumed, &snapshot(&parser, "work"));
        assert_eq!(resumed.callbacks().history, parser.callbacks().history);
        assert_eq!(resumed.screen().contents_formatted(), screen.contents_formatted());

        parser.process(b"\x1b[?1049h\x1b[2J\x1b[Hvim here");
        let mut copy = vt100::Parser::new(nz(10), nz(30), 100);
        copy.process(&snapshot(&parser, ""));
        assert!(copy.screen().alternate_screen());
        assert_eq!(copy.screen().contents(), parser.screen().contents());
        copy.process(b"\x1b[?1049l");
        copy.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(copy.screen().scrollback(), 36, "the history outlives vim");

        feed(&mut parser, b"\x1b[?1049l\x1b[H\x1b[2J\x1b[3Jafter");
        assert!(parser.callbacks().history.is_empty());
    }

    #[test]
    fn names_a_process_by_its_argv0() {
        use std::os::unix::process::CommandExt;
        let mut child = std::process::Command::new("/bin/sleep").arg0("-/opt/claude").arg("5").spawn().unwrap();
        let name = process_name(child.id());
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(name.as_deref(), Some("claude"));
        assert_eq!(process_name(1).as_deref(), Some("launchd"));
        assert_eq!(process_name(0x7fff_fff0), None);
    }

    #[test]
    fn keeps_what_programs_report_about_themselves() {
        let mut parser = vt100::Parser::new_with_callbacks(nz(10), nz(20), 0, Events::default());
        parser.process(b"\x1b]7501;?\x1b\\");
        assert_eq!(parser.callbacks().replies, b"\x1b]7501;?\x1b\\");
        parser.process(b"\x1b]9;4;3\x07");
        assert_eq!(parser.callbacks().status.summary().unwrap().0, Status::Working);
        parser.process(b"\x1b]7501;state=blocked:kind=question:msg=TmFtZT8\x1b\\");
        parser.process(b"\x1b]9;4;0\x07");
        assert_eq!(parser.callbacks().status.summary(), Some((Status::Blocked, "Name?".into())));
        parser.process(b"\x1b]133;A\x07");
        assert_eq!(parser.callbacks().status.summary(), None);
        parser.process(b"\x1b]52;c;aGk=\x07");
        assert_eq!(parser.callbacks().host, b"\x1b]52;c;aGk=\x07");
    }
}
