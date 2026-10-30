//! An embedded terminal: a process on a PTY, emulated with vt100.

use std::io::{Read, Write};
use std::num::NonZeroU16;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use anyhow::Result;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};

use crate::{Msg, theme};

const SCROLLBACK: usize = 5000;
const BUSY_FOR: Duration = Duration::from_millis(1500);

/// Things the emulated program asks of its terminal.
#[derive(Default)]
struct Events {
    replies: Vec<u8>,
    bell: bool,
    title: Option<String>,
}

impl vt100::Callbacks for Events {
    fn audible_bell(&mut self, _: &mut vt100::Screen) {
        self.bell = true;
    }

    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = Some(String::from_utf8_lossy(title).into_owned());
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

pub struct Term {
    pub id: usize,
    /// Exported as `WS_TAB` so hook events can be matched to this terminal.
    pub tag: String,
    pub name: String,
    /// The program running in the foreground, when it isn't the shell itself
    /// (None: the shell is idle at its prompt).
    pub running: Option<String>,
    /// The workspace directory the program was started in.
    pub cwd: PathBuf,
    pub title: String,
    /// The program rang the bell while this tab was in the background.
    pub bell: bool,
    last_output: Instant,
    parser: vt100::Parser<Events>,
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send + Sync>,
}

impl Term {
    pub fn spawn(id: usize, program: &str, cwd: &Path, (rows, cols): (u16, u16), tx: Sender<Msg>) -> Result<Term> {
        let pair = native_pty_system().openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })?;
        let tag = format!("{}-{id}", std::process::id());
        let mut cmd = CommandBuilder::new(program);
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("WS_TAB", &tag);
        // Don't let programs assume Ghostty features this emulator lacks.
        cmd.env_remove("TERM_PROGRAM");
        cmd.env_remove("TERM_PROGRAM_VERSION");
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = pair.master.take_writer()?;
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            while let Ok(n @ 1..) = reader.read(&mut buf) {
                if tx.send(Msg::Pty(id, buf[..n].to_vec())).is_err() {
                    return;
                }
            }
            let _ = tx.send(Msg::PtyExit(id));
        });

        let name = Path::new(program).file_name().unwrap_or_default().to_string_lossy().into_owned();
        Ok(Term {
            id,
            tag,
            name,
            running: None,
            cwd: cwd.to_path_buf(),
            title: String::new(),
            bell: false,
            last_output: Instant::now(),
            parser: vt100::Parser::new_with_callbacks(nz(rows), nz(cols), SCROLLBACK, Events::default()),
            master: pair.master,
            writer,
            child,
        })
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        self.parser.process(bytes);
        self.last_output = Instant::now();
        let events = self.parser.callbacks_mut();
        let replies = std::mem::take(&mut events.replies);
        if let Some(title) = events.title.take() {
            self.title = title;
        }
        self.bell |= std::mem::take(&mut events.bell);
        if !replies.is_empty() {
            let _ = self.writer.write_all(&replies);
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) {
        if self.parser.screen().size() == (nz(rows), nz(cols)) {
            return;
        }
        let _ = self.master.resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
        self.parser.set_size(nz(rows), nz(cols));
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
        let _ = self.writer.write_all(bytes);
        let _ = self.writer.flush();
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
                let _ = self.writer.write_all(&bytes);
                let _ = self.writer.flush();
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

    /// What to call this terminal: the program running in it, or the shell.
    pub fn label(&self) -> &str {
        self.running.as_deref().unwrap_or(&self.name)
    }

    pub fn refresh_foreground(&mut self) {
        let leader = self.master.process_group_leader();
        self.running = match (leader, self.child.process_id()) {
            (Some(group), Some(shell)) if group as u32 != shell => process_name(group as u32),
            _ => None,
        };
    }

    pub fn busy(&self) -> bool {
        self.last_output.elapsed() < BUSY_FOR
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

impl Drop for Term {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}

fn process_name(pid: u32) -> Option<String> {
    let out = std::process::Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]).output().ok()?;
    let comm = String::from_utf8_lossy(&out.stdout);
    let name = comm.trim().rsplit('/').next()?.trim_start_matches('-');
    (!name.is_empty()).then(|| name.to_string())
}

fn nz(n: u16) -> NonZeroU16 {
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
}
