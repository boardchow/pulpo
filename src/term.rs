//! An embedded terminal as Pulpo shows it: a mirror of one that the server
//! (`server.rs`) runs, fed with its screen and then its output. Also the
//! emulator parts both sides share.

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

/// History kept by the mirror; the server keeps the same.
pub const SCROLLBACK: usize = 5000;

/// Things the emulated program asks of its terminal.
#[derive(Default)]
pub struct Events {
    pub replies: Vec<u8>,
    pub bell: bool,
    pub title: Option<String>,
    /// Sequences to pass on to the terminal Pulpo runs in (clipboard).
    pub host: Vec<u8>,
    /// What the program reports about itself (OSC 7501).
    pub status: Records,
    /// Progress (OSC 9;4) stands in for a status until a real one arrives.
    reports_status: bool,
}

impl Events {
    /// Carrying on with what a terminal's programs reported before.
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
}

/// The mirror only draws: the server answers the program's queries, and
/// the clipboard goes on to the terminal Pulpo runs in.
struct Mirror;

impl vt100::Callbacks for Mirror {
    fn copy_to_clipboard(&mut self, _: &mut vt100::Screen, ty: &[u8], data: &[u8]) {
        to_host(&[&b"\x1b]52;"[..], ty, b";", data, b"\x07"].concat());
    }
}

pub struct Term {
    pub id: usize,
    /// What the server says about it: its program, title, state.
    pub info: Info,
    /// Its agent finished or asked something (hooks), not seen yet.
    pub hook_unread: bool,
    /// The agent state its hooks last reported, to notice when it changes.
    pub reported: State,
    /// Something that wants you now (an agent asks or finished), to notify
    /// about once; Pulpo takes it.
    pub notice: Option<String>,
    pub last_notice: Option<Instant>,
    parser: vt100::Parser<Mirror>,
    link: Link,
    /// The size last asked of the server, so each is asked once.
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
            parser: vt100::Parser::new_with_callbacks(nz(rows), nz(cols), SCROLLBACK, Mirror),
            link,
            asked: (rows, cols),
        }
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
    }

    pub fn set_info(&mut self, info: Info) {
        // It reports that it waits for you, failed or finished.
        let before = self.info.status.as_ref().map(|(status, _)| *status);
        if let Some((now, detail)) = &info.status
            && Some(*now) != before
            && matches!(now, Status::Blocked | Status::Error | Status::Done)
        {
            self.notice = Some(detail.clone());
        }
        self.info = info;
    }

    /// The server changed its size; what follows is drawn at that size.
    pub fn set_size(&mut self, rows: u16, cols: u16) {
        self.parser.set_size(nz(rows), nz(cols));
    }

    /// Ask the server for this size.
    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.asked != (rows, cols) {
            self.asked = (rows, cols);
            self.link.send(&ToServer::Resize { id: self.info.id, rows, cols });
        }
    }

    /// You are looking at it: nothing left to read.
    pub fn seen(&mut self) {
        self.hook_unread = false;
        self.notice = None;
        if self.info.unread {
            self.info.unread = false;
            self.link.send(&ToServer::Seen { id: self.info.id });
        }
    }

    /// It finished something or asked you something since you last looked.
    pub fn unread(&self) -> bool {
        self.info.unread || self.hook_unread
    }

    /// The program is printing on its own: an agent at work, a build, a server.
    pub fn working(&self) -> bool {
        self.info.working
    }

    /// What the program says it is doing (OSC 7501), if it says.
    pub fn status(&self) -> Option<(Status, String)> {
        self.info.status.clone()
    }

    /// What to call this terminal: the program running in it, or the shell.
    pub fn label(&self) -> &str {
        self.info.running.as_deref().unwrap_or(&self.info.name)
    }

    pub fn key(&mut self, key: KeyEvent) {
        if let Some(bytes) = encode_key(key, self.parser.screen().application_cursor()) {
            self.send(&bytes);
        }
    }

    pub fn paste(&mut self, text: &str) {
        if self.parser.screen().bracketed_paste() {
            self.send(format!("\x1b[200~{text}\x1b[201~").as_bytes());
        } else {
            self.send(text.as_bytes());
        }
    }

    fn send(&mut self, bytes: &[u8]) {
        self.parser.screen_mut().set_scrollback(0);
        self.link.input(self.info.id, bytes);
    }

    /// The program asked to receive mouse events (Claude, vim with mouse on…).
    pub fn wants_mouse(&self) -> bool {
        self.parser.screen().mouse_protocol_mode() != vt100::MouseProtocolMode::None
    }

    /// The program draws on its own screen (Claude's full-screen views,
    /// less, vim): there is no history to scroll.
    pub fn alternate_screen(&self) -> bool {
        self.parser.screen().alternate_screen()
    }

    /// Send a mouse event at a 1-based cell, in the encoding the program asked
    /// for. Returns false if it doesn't take this kind of event.
    pub fn mouse(&mut self, kind: MouseEventKind, col: u16, row: u16, modifiers: KeyModifiers) -> bool {
        let screen = self.parser.screen();
        match encode_mouse(screen.mouse_protocol_mode(), screen.mouse_protocol_encoding(), kind, col, row, modifiers) {
            Some(bytes) => {
                self.link.input(self.info.id, &bytes);
                true
            }
            None => false,
        }
    }

    /// Scroll through history; positive goes back in time.
    pub fn scroll(&mut self, rows: isize) {
        let screen = self.parser.screen_mut();
        screen.set_scrollback(screen.scrollback().saturating_add_signed(rows));
    }

    /// Draw the screen into `area`; returns where the cursor should be shown.
    pub fn render(&self, area: Rect, buf: &mut Buffer) -> Option<(u16, u16)> {
        let screen = self.parser.screen();
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
                        .fg(color(cell.fgcolor(), theme::FG))
                        .bg(color(cell.bgcolor(), theme::BG))
                        .add_modifier(modifier),
                );
            }
        }
        let (row, col) = screen.cursor_position();
        (!screen.hide_cursor() && screen.scrollback() == 0 && row < area.height && col < area.width)
            .then(|| (area.x + col, area.y + row))
    }
}

/// Bytes that make a new parser of the same size show what `screen` shows:
/// its history, then the screen itself with its modes, and the title.
/// vt100 can't read the screen behind an alternate one (a full-screen
/// program's); that part and its history are left out.
pub fn snapshot(screen: &vt100::Screen, title: &str) -> Vec<u8> {
    let mut screen = screen.clone();
    let (rows, cols) = (screen.size().0.get(), screen.size().1.get());
    let mut out = String::new();
    if screen.alternate_screen() {
        out.push_str("\x1b[?1049h");
    } else {
        screen.set_scrollback(usize::MAX);
        let mut back = screen.scrollback();
        if back > 0 {
            while back > 0 {
                screen.set_scrollback(back);
                let n = back.min(usize::from(rows));
                for row in screen.rows_formatted(0, cols).take(n) {
                    out.push_str("\x1b[m");
                    out.push_str(&row);
                    out.push_str("\x1b[m\r\n");
                }
                back -= n;
            }
            // Scroll those lines off the screen into the history, so drawing
            // the screen over them doesn't lose them.
            out.push_str(&"\n".repeat(usize::from(rows) - 1));
        }
        screen.set_scrollback(0);
    }
    out.push_str(&screen.state_formatted());
    if !title.is_empty() {
        out.push_str(&format!("\x1b]2;{title}\x07"));
    }
    out.into_bytes()
}

/// Write to the terminal Pulpo itself runs in (Ghostty), between frames.
pub fn to_host(bytes: &[u8]) {
    if !bytes.is_empty() {
        let mut out = std::io::stdout();
        let _ = out.write_all(bytes);
        let _ = out.flush();
    }
}

pub fn process_name(pid: u32) -> Option<String> {
    let out = std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output().ok()?;
    let comm = String::from_utf8_lossy(&out.stdout);
    let name = comm.trim().rsplit('/').next()?.trim_start_matches('-');
    (!name.is_empty()).then(|| name.to_string())
}

pub fn nz(n: u16) -> NonZeroU16 {
    NonZeroU16::new(n).unwrap_or(NonZeroU16::MIN)
}

/// The 16 basic colors and the defaults come from the theme, so terminals
/// look like VS Code's.
fn color(c: vt100::Color, default: Color) -> Color {
    match c {
        vt100::Color::Default => default,
        vt100::Color::Idx(i) if i < 16 => theme::ANSI[i as usize],
        vt100::Color::Idx(i) => Color::Indexed(i),
        vt100::Color::Rgb(r, g, b) => Color::Rgb(r, g, b),
    }
}

/// Translate a mouse event into the bytes an xterm would send.
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

/// Translate a key into the bytes an xterm would send.
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
        let mut parser = vt100::Parser::new_with_callbacks(nz(10), nz(30), 100, Events::default());
        for i in 0..45 {
            parser.process(format!("\x1b[3{}mline {i}\x1b[m\r\n", i % 8).as_bytes());
        }
        parser.process(b"\x1b[?2004h\x1b[?1000h\x1b[?1006h\x1b[?1h$ half typed\x1b[4D");
        let screen = parser.screen();

        let mut copy = vt100::Parser::new(nz(10), nz(30), 100);
        copy.process(&snapshot(screen, "work"));
        let other = copy.screen();
        assert_eq!(other.contents(), screen.contents());
        assert_eq!(other.contents_formatted(), screen.contents_formatted());
        assert_eq!(other.cursor_position(), screen.cursor_position());
        assert!(other.bracketed_paste() && other.application_cursor());
        assert_eq!(other.mouse_protocol_mode(), screen.mouse_protocol_mode());
        assert_eq!(other.mouse_protocol_encoding(), screen.mouse_protocol_encoding());
        // The same history, colors included.
        let history = |s: &vt100::Screen| {
            let mut s = s.clone();
            s.set_scrollback(usize::MAX);
            (s.scrollback(), s.contents_formatted())
        };
        assert_eq!(history(other), history(screen));
        assert_eq!(history(other).0, 36);

        // A full-screen program: its screen, without history.
        parser.process(b"\x1b[?1049h\x1b[2J\x1b[Hvim here");
        let mut copy = vt100::Parser::new(nz(10), nz(30), 100);
        copy.process(&snapshot(parser.screen(), ""));
        assert!(copy.screen().alternate_screen());
        assert_eq!(copy.screen().contents(), parser.screen().contents());
    }

    #[test]
    fn keeps_what_programs_report_about_themselves() {
        let mut parser = vt100::Parser::new_with_callbacks(nz(10), nz(20), 0, Events::default());
        parser.process(b"\x1b]7501;?\x1b\\");
        assert_eq!(parser.callbacks().replies, b"\x1b]7501;?\x1b\\");
        // Progress counts as working, until the program reports a real status.
        parser.process(b"\x1b]9;4;3\x07");
        assert_eq!(parser.callbacks().status.summary().unwrap().0, Status::Working);
        parser.process(b"\x1b]7501;state=blocked:kind=question:msg=TmFtZT8\x1b\\");
        parser.process(b"\x1b]9;4;0\x07");
        assert_eq!(parser.callbacks().status.summary(), Some((Status::Blocked, "Name?".into())));
        // The next prompt ends it.
        parser.process(b"\x1b]133;A\x07");
        assert_eq!(parser.callbacks().status.summary(), None);
        parser.process(b"\x1b]52;c;aGk=\x07");
        assert_eq!(parser.callbacks().host, b"\x1b]52;c;aGk=\x07");
    }
}
