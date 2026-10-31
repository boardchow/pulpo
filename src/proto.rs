//! What Pulpo and its server (`pulpo serve`) say to each other over the
//! server's Unix socket.
//!
//! A frame is `[u32 length][u8 kind][payload]`: kind 0 carries a JSON
//! message, kind 1 a terminal's output and kind 2 input for one, both as
//! `[u32 terminal id][bytes]`. `Hello` and `Upgrade` must never change, so
//! that a Pulpo can always find out the server runs another build, and
//! have it become its own.

use std::io::{self, Read};
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::status::Status;

/// Bumped when a message changes in a way the other side would misread.
pub const PROTOCOL: u32 = 1;
const MAX_FRAME: usize = 64 << 20;

const JSON: u8 = 0;
const OUTPUT: u8 = 1;
const INPUT: u8 = 2;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToServer {
    Hello { protocol: u32 },
    /// Start a terminal; `token` comes back in its `Attach`.
    Spawn {
        token: u64,
        program: String,
        args: Vec<String>,
        cwd: PathBuf,
        rows: u16,
        cols: u16,
        /// Set in its environment, over the server's (an isolated
        /// session's Compose project and ports).
        #[serde(default)]
        env: Vec<(String, String)>,
    },
    Resize { id: u32, rows: u16, cols: u16 },
    /// You looked at it: nothing left to read.
    Seen { id: u32 },
    /// Stop it, and the programs it runs.
    Close { id: u32 },
    /// Replace the server with the program at `exe`, keeping every terminal.
    Upgrade { exe: PathBuf },
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToClient {
    /// `exe` and `built` (its modification time) tell a Pulpo whether the
    /// server runs an older build than its own.
    Hello { protocol: u32, pid: u32, exe: PathBuf, built: u64 },
    /// A terminal to show, `rows` x `cols`; an output frame with its
    /// snapshot follows. `token` answers this client's `Spawn`.
    Attach { info: Info, rows: u16, cols: u16, token: Option<u64> },
    /// Every terminal the server had when this client connected is attached.
    Ready,
    Info { info: Info },
    /// The terminal changed size; output after this is at the new size.
    Resized { id: u32, rows: u16, cols: u16 },
    Exit { id: u32 },
    Error { message: String },
}

/// What a terminal shows besides its screen.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Default)]
pub struct Info {
    pub id: u32,
    /// `PULPO_TAB` in its environment, so hook events find it.
    pub tag: String,
    /// The program it was started with, and the one in the foreground now
    /// when that isn't the shell.
    pub name: String,
    pub running: Option<String>,
    pub cwd: PathBuf,
    pub title: String,
    /// The process it started, root of what runs in it.
    pub pid: i32,
    /// It prints on its own.
    pub working: bool,
    /// It finished something or rang since someone last looked.
    pub unread: bool,
    /// What it reports about itself (OSC 7501).
    pub status: Option<(Status, String)>,
}

pub enum Frame {
    Json(Vec<u8>),
    Output(u32, Vec<u8>),
    Input(u32, Vec<u8>),
}

pub fn encode(message: &impl Serialize) -> Vec<u8> {
    let json = serde_json::to_vec(message).expect("messages always serialize");
    frame(JSON, &[&json])
}

pub fn output(id: u32, bytes: &[u8]) -> Vec<u8> {
    frame(OUTPUT, &[&id.to_be_bytes(), bytes])
}

pub fn input(id: u32, bytes: &[u8]) -> Vec<u8> {
    frame(INPUT, &[&id.to_be_bytes(), bytes])
}

fn frame(kind: u8, parts: &[&[u8]]) -> Vec<u8> {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    let mut out = Vec::with_capacity(5 + len);
    out.extend((len as u32).to_be_bytes());
    out.push(kind);
    for part in parts {
        out.extend(*part);
    }
    out
}

pub fn read_frame(r: &mut impl Read) -> io::Result<Frame> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head)?;
    let len = u32::from_be_bytes(head[..4].try_into().expect("4 bytes")) as usize;
    if len > MAX_FRAME {
        return Err(io::Error::new(io::ErrorKind::InvalidData, format!("frame of {len} bytes")));
    }
    let mut payload = vec![0u8; len];
    r.read_exact(&mut payload)?;
    let id_and_bytes = |mut payload: Vec<u8>| -> io::Result<(u32, Vec<u8>)> {
        if payload.len() < 4 {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "short frame"));
        }
        let bytes = payload.split_off(4);
        Ok((u32::from_be_bytes(payload.try_into().expect("4 bytes")), bytes))
    };
    match head[4] {
        JSON => Ok(Frame::Json(payload)),
        OUTPUT => id_and_bytes(payload).map(|(id, b)| Frame::Output(id, b)),
        INPUT => id_and_bytes(payload).map(|(id, b)| Frame::Input(id, b)),
        kind => Err(io::Error::new(io::ErrorKind::InvalidData, format!("frame kind {kind}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let hello = ToServer::Hello { protocol: PROTOCOL };
        let mut wire = encode(&hello);
        wire.extend(output(7, b"\x1b[31mred"));
        wire.extend(input(3, b""));
        let mut r = &wire[..];
        let Frame::Json(json) = read_frame(&mut r).unwrap() else { panic!() };
        assert_eq!(serde_json::from_slice::<ToServer>(&json).unwrap(), hello);
        assert!(matches!(read_frame(&mut r).unwrap(), Frame::Output(7, b) if b == b"\x1b[31mred"));
        assert!(matches!(read_frame(&mut r).unwrap(), Frame::Input(3, b) if b.is_empty()));
        // The stream ended cleanly between frames.
        assert_eq!(read_frame(&mut r).err().unwrap().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn refuses_bad_frames() {
        let huge = [&(MAX_FRAME as u32 + 1).to_be_bytes()[..], &[JSON]].concat();
        assert_eq!(read_frame(&mut &huge[..]).err().unwrap().kind(), io::ErrorKind::InvalidData);
        assert!(read_frame(&mut &frame(9, &[b"x"])[..]).is_err());
        assert!(read_frame(&mut &frame(OUTPUT, &[b"ab"])[..]).is_err());
    }

    #[test]
    fn hello_keeps_its_shape() {
        // Older and newer builds rely on this exact form.
        let json = serde_json::to_string(&ToServer::Hello { protocol: 1 }).unwrap();
        assert_eq!(json, r#"{"type":"hello","protocol":1}"#);
        let json = serde_json::to_string(&ToServer::Upgrade { exe: "/bin/pulpo".into() }).unwrap();
        assert_eq!(json, r#"{"type":"upgrade","exe":"/bin/pulpo"}"#);
    }
}
