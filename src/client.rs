//! Pulpo's side of the server: connect to it, or start it, then pass what
//! it says on to the event loop.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::net::UnixStream;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, Result};

use crate::Msg;
use crate::proto::{self, Frame, ToClient, ToServer};
use crate::server;

/// The way to the server, shared by every terminal. Writes that fail are
/// dropped: the reader notices the server is gone and says so.
#[derive(Clone)]
pub struct Link(Arc<Mutex<UnixStream>>);

impl Link {
    pub fn send(&self, message: &ToServer) {
        self.write(&proto::encode(message));
    }

    pub fn input(&self, id: u32, bytes: &[u8]) {
        self.write(&proto::input(id, bytes));
    }

    fn write(&self, frame: &[u8]) {
        if let Ok(mut stream) = self.0.lock() {
            let _ = stream.write_all(frame);
        }
    }
}

/// Connect to the server, starting it if none runs, and say hello. Its
/// messages arrive on `tx`; `Msg::ServerGone` when it goes away.
pub fn connect(tx: Sender<Msg>) -> Result<Link> {
    let path = server::socket();
    let stream = match UnixStream::connect(&path) {
        Ok(stream) => stream,
        Err(_) => {
            start()?;
            let mut tries = 0;
            loop {
                std::thread::sleep(Duration::from_millis(50));
                match UnixStream::connect(&path) {
                    Ok(stream) => break stream,
                    Err(e) if tries == 100 => {
                        let log = server::dir().join("log");
                        return Err(e).with_context(|| format!("the Pulpo server didn't start; see {}", log.display()));
                    }
                    Err(_) => tries += 1,
                }
            }
        }
    };
    let link = Link(Arc::new(Mutex::new(stream.try_clone()?)));
    link.send(&ToServer::Hello { protocol: proto::PROTOCOL });
    let mut reader = stream;
    std::thread::spawn(move || {
        while let Ok(frame) = proto::read_frame(&mut reader) {
            let msg = match frame {
                Frame::Json(json) => match serde_json::from_slice::<ToClient>(&json) {
                    Ok(message) => Msg::Server(message),
                    Err(_) => continue,
                },
                Frame::Output(id, bytes) => Msg::Output(id, bytes),
                Frame::Input(..) => continue,
            };
            if tx.send(msg).is_err() {
                return;
            }
        }
        let _ = tx.send(Msg::ServerGone);
    });
    Ok(link)
}

/// Start `pulpo serve` on its own: its own session, no terminal, output to
/// its log, so it keeps running after this Pulpo quits.
fn start() -> Result<()> {
    let dir = server::dir();
    fs::create_dir_all(&dir)?;
    let log = OpenOptions::new().create(true).append(true).open(dir.join("log"))?;
    let mut cmd = Command::new(std::env::current_exe()?);
    cmd.arg("serve").current_dir("/").stdin(Stdio::null()).stdout(log.try_clone()?).stderr(log);
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    cmd.spawn().context("starting the Pulpo server")?;
    Ok(())
}
