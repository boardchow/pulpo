//! Pseudo-terminals, with only what Pulpo needs from libc. A terminal is a
//! master file descriptor and a process id, so it can be handed to another
//! program image (the server's upgrade) and rebuilt from those two numbers.

use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::process::{Command, Stdio};

pub struct Pty {
    pub master: File,
    pub pid: libc::pid_t,
    /// Its exit was collected; the pid may now belong to someone else.
    reaped: bool,
}

/// Start `program` on a new pseudo-terminal of `rows` x `cols`, as the
/// leader of its own session with the terminal as its controlling tty.
pub fn spawn(program: &str, args: &[String], cwd: &Path, env: &[(&str, &str)], unset: &[&str], (rows, cols): (u16, u16)) -> io::Result<Pty> {
    let (mut master, mut slave): (RawFd, RawFd) = (-1, -1);
    let mut size = winsize(rows, cols);
    if unsafe { libc::openpty(&mut master, &mut slave, std::ptr::null_mut(), std::ptr::null_mut(), &mut size) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let (master, slave) = unsafe { (OwnedFd::from_raw_fd(master), OwnedFd::from_raw_fd(slave)) };
    // Other terminals' programs must not inherit this one's end.
    set_cloexec(master.as_raw_fd(), true)?;

    let mut cmd = Command::new(program);
    cmd.args(args).current_dir(cwd).envs(env.iter().copied());
    for name in unset {
        cmd.env_remove(name);
    }
    cmd.stdin(Stdio::from(slave.try_clone()?)).stdout(Stdio::from(slave.try_clone()?)).stderr(Stdio::from(slave));
    unsafe {
        cmd.pre_exec(|| {
            // Start clean of whatever signal handling Pulpo set up.
            for signal in [libc::SIGCHLD, libc::SIGHUP, libc::SIGINT, libc::SIGQUIT, libc::SIGTERM, libc::SIGALRM, libc::SIGPIPE] {
                libc::signal(signal, libc::SIG_DFL);
            }
            let empty: libc::sigset_t = std::mem::zeroed();
            libc::sigprocmask(libc::SIG_SETMASK, &empty, std::ptr::null_mut());
            // Its own session, with the terminal as its controlling tty, so
            // it gets SIGWINCH on resize and job control works.
            if libc::setsid() == -1 || libc::ioctl(0, libc::TIOCSCTTY as _, 0) == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }
    // The child is waited for by pid (`try_wait`), not through `Child`.
    let child = cmd.spawn()?;
    Ok(Pty { master: File::from(master), pid: child.id() as libc::pid_t, reaped: false })
}

impl Pty {
    /// A terminal handed over by the server before it upgraded.
    ///
    /// # Safety
    /// `master` must be an open pseudo-terminal master that nothing else
    /// owns, and `pid` its program, a child of this process.
    pub unsafe fn from_raw(master: RawFd, pid: libc::pid_t) -> Pty {
        Pty { master: unsafe { File::from_raw_fd(master) }, pid, reaped: false }
    }

    pub fn resize(&self, rows: u16, cols: u16) {
        let size = winsize(rows, cols);
        unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) };
    }

    /// The process group in the foreground: the shell itself, or the
    /// program it runs.
    pub fn foreground(&self) -> Option<libc::pid_t> {
        match unsafe { libc::tcgetpgrp(self.master.as_raw_fd()) } {
            pgrp if pgrp > 0 => Some(pgrp),
            _ => None,
        }
    }

    /// Its program has exited (and is reaped).
    pub fn exited(&mut self) -> bool {
        self.reaped |= unsafe { libc::waitpid(self.pid, std::ptr::null_mut(), libc::WNOHANG) } == self.pid;
        self.reaped
    }

    /// Tell the program its terminal went away, as closing a window does.
    pub fn hang_up(&self) {
        if !self.reaped {
            unsafe { libc::kill(self.pid, libc::SIGHUP) };
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.hang_up();
        // Reap it if it is already gone, so it doesn't linger as a zombie.
        self.exited();
    }
}

pub fn set_cloexec(fd: RawFd, on: bool) -> io::Result<()> {
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
    let flags = if on { flags | libc::FD_CLOEXEC } else { flags & !libc::FD_CLOEXEC };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn winsize(rows: u16, cols: u16) -> libc::winsize {
    libc::winsize { ws_row: rows.max(1), ws_col: cols.max(1), ws_xpixel: 0, ws_ypixel: 0 }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    #[test]
    fn runs_a_program_on_a_terminal() {
        let pty = spawn("/bin/sh", &["-c".into(), "stty size; tty".into()], Path::new("/"), &[], &[], (12, 34)).unwrap();
        // Reads end (EIO) once the program exits and closes the slave.
        let (mut master, mut out, mut buf) = (pty.master.try_clone().unwrap(), Vec::new(), [0u8; 256]);
        while let Ok(n @ 1..) = master.read(&mut buf) {
            out.extend(&buf[..n]);
        }
        let out = String::from_utf8_lossy(&out);
        assert!(out.starts_with("12 34"), "{out:?}");
        assert!(out.contains("/dev/tty"), "{out:?}");
    }
}
