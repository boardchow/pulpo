#![allow(dead_code)]

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

pub const PROTOCOL: u32 = 2;

pub fn frame(kind: u8, payload: &[u8]) -> Vec<u8> {
    let mut out = (payload.len() as u32).to_be_bytes().to_vec();
    out.push(kind);
    out.extend(payload);
    out
}

pub fn read_frame(r: &mut impl Read) -> Option<(u8, Vec<u8>)> {
    let mut head = [0u8; 5];
    r.read_exact(&mut head).ok()?;
    let mut payload = vec![0u8; u32::from_be_bytes(head[..4].try_into().unwrap()) as usize];
    r.read_exact(&mut payload).ok()?;
    Some((head[4], payload))
}

pub fn connect(sock: &Path) -> UnixStream {
    let started = Instant::now();
    loop {
        match UnixStream::connect(sock) {
            Ok(stream) => return stream,
            Err(_) if started.elapsed() < Duration::from_secs(10) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => panic!("no server at {}: {e}", sock.display()),
        }
    }
}

pub struct Conn(pub UnixStream);

impl Conn {
    pub fn open(sock: &Path) -> Conn {
        let stream = connect(sock);
        stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut conn = Conn(stream);
        conn.send(json!({ "type": "hello", "protocol": PROTOCOL }));
        conn
    }

    pub fn send(&mut self, message: Value) {
        self.0.write_all(&frame(0, &serde_json::to_vec(&message).unwrap())).unwrap();
    }

    /// Its output is sent from now on, after a snapshot.
    pub fn watch(&mut self, id: u64) {
        self.send(json!({ "type": "watch", "id": id }));
    }

    pub fn input(&mut self, id: u64, text: &str) {
        self.0.write_all(&frame(2, &[&(id as u32).to_be_bytes()[..], text.as_bytes()].concat())).unwrap();
    }

    pub fn read(&mut self) -> Option<Result<Value, String>> {
        let (kind, payload) = read_frame(&mut self.0)?;
        Some(match kind {
            0 => Ok(serde_json::from_slice(&payload).unwrap()),
            _ => Err(String::from_utf8_lossy(&payload[4..]).into_owned()),
        })
    }

    pub fn until(&mut self, kind: &str) -> Value {
        loop {
            match self.read().expect("the server hung up") {
                Ok(message) if message["type"] == kind => return message,
                _ => {}
            }
        }
    }

    pub fn until_output(&mut self, text: &str) {
        let mut seen = String::new();
        while !seen.contains(text) {
            if let Err(output) = self.read().expect("the server hung up") {
                seen.push_str(&output);
            }
        }
    }
}

pub fn wait_for(what: &str, mut done: impl FnMut() -> bool) {
    let started = Instant::now();
    while !done() {
        assert!(started.elapsed() < Duration::from_secs(20), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(100));
    }
}

pub fn running(pattern: &str) -> bool {
    Command::new("pgrep").args(["-f", pattern]).output().unwrap().status.success()
}
