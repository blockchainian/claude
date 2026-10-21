//! Programs run on a pseudo-terminal whose master side this process reads and writes.

use std::cell::Cell;
use std::fs::File;
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::process::CommandExt;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

pub struct Pty {
    child: Child,
    master: File,
    output_claimed: Cell<bool>,
}

impl Pty {
    /// Starts `command` as the session leader of a new terminal of `rows` by `cols` cells.
    pub fn spawn(mut command: Command, rows: u16, cols: u16) -> io::Result<Self> {
        let mut master = -1;
        let mut slave = -1;
        let mut size = winsize(rows, cols);
        // openpty returns independent owned descriptors on success.
        if unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut size,
            )
        } != 0
        {
            return Err(io::Error::last_os_error());
        }
        let master = unsafe { File::from_raw_fd(master) };
        let slave = unsafe { File::from_raw_fd(slave) };
        // Only the standard descriptors belong in the executed process.
        for fd in [master.as_raw_fd(), slave.as_raw_fd()] {
            if unsafe { libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
                return Err(io::Error::last_os_error());
            }
        }
        command
            .env("TERM", "xterm-256color")
            .stdin(Stdio::from(slave.try_clone()?))
            .stdout(Stdio::from(slave.try_clone()?))
            .stderr(Stdio::from(slave));
        unsafe {
            command.pre_exec(move || {
                if libc::setsid() == -1 {
                    return Err(io::Error::last_os_error());
                }
                if libc::ioctl(libc::STDIN_FILENO, libc::TIOCSCTTY as _, 0) == -1 {
                    return Err(io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        Ok(Self {
            child,
            master,
            output_claimed: Cell::new(false),
        })
    }

    #[cfg(test)]
    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn exited(&mut self) -> io::Result<bool> {
        Ok(self.child.try_wait()?.is_some())
    }

    /// Gives output to a reader thread; shutdown leaves draining to that reader.
    /// A second handle on the terminal's input, for writing from another thread.
    pub fn writer(&self) -> io::Result<File> {
        self.master.try_clone()
    }

    pub fn reader(&self) -> io::Result<File> {
        let reader = self.master.try_clone()?;
        self.output_claimed.set(true);
        Ok(reader)
    }

    /// Waits up to `timeout` for output and appends it to `output`; returns false once the
    /// terminal is closed.
    pub fn read(&mut self, timeout: Duration, output: &mut Vec<u8>) -> io::Result<bool> {
        let mut descriptor = libc::pollfd {
            fd: self.master.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        let result = unsafe {
            libc::poll(
                &mut descriptor,
                1,
                timeout.as_millis().min(i32::MAX as u128) as i32,
            )
        };
        if result < 0 {
            let error = io::Error::last_os_error();
            return if error.kind() == io::ErrorKind::Interrupted {
                Ok(true)
            } else {
                Err(error)
            };
        }
        if result == 0 {
            return Ok(true);
        }
        read_available(&mut self.master, output)
    }

    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.master.write_all(bytes)?;
        self.master.flush()
    }

    /// Resizes the terminal; the kernel tells the program with SIGWINCH.
    pub fn resize(&self, rows: u16, cols: u16) -> io::Result<()> {
        let size = winsize(rows, cols);
        if unsafe { libc::ioctl(self.master.as_raw_fd(), libc::TIOCSWINSZ, &size) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// Writes `detach` and waits up to `timeout` for the program to exit, killing it after
    /// that. Without `detach` the program is killed at once.
    pub fn close(&mut self, detach: Option<&[u8]>, timeout: Duration) -> io::Result<()> {
        let write = match detach {
            Some(bytes) => self.write(bytes),
            None => Ok(()),
        };
        let deadline = Instant::now()
            + if detach.is_some() {
                timeout
            } else {
                Duration::ZERO
            };
        loop {
            if self.child.try_wait()?.is_some() {
                return write;
            }
            if Instant::now() >= deadline {
                self.child.kill()?;
                self.child.wait()?;
                return write;
            }
            if self.output_claimed.get() {
                // A second blocking read can race the terminal reader and miss the deadline.
                std::thread::sleep(Duration::from_millis(10));
            } else {
                // Drain output while waiting: a full PTY buffer must not block the child's exit.
                let _ = self.read(Duration::from_millis(50), &mut Vec::new());
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        match self.child.try_wait() {
            Ok(Some(_)) => {}
            _ => {
                let _ = self.child.kill();
                let _ = self.child.wait();
            }
        }
    }
}

/// Reads what the master side holds now; returns false once the terminal is closed.
pub fn read_available(master: &mut File, output: &mut Vec<u8>) -> io::Result<bool> {
    let mut bytes = [0; 65536];
    match master.read(&mut bytes) {
        Ok(0) => Ok(false),
        Ok(count) => {
            output.extend_from_slice(&bytes[..count]);
            Ok(true)
        }
        Err(error) if error.kind() == io::ErrorKind::Interrupted => Ok(true),
        // macOS reports a master whose last slave closed as EIO rather than end of file.
        Err(error) if error.raw_os_error() == Some(libc::EIO) => Ok(false),
        Err(error) => Err(error),
    }
}

fn winsize(rows: u16, cols: u16) -> libc::winsize {
    libc::winsize {
        ws_row: rows,
        ws_col: cols,
        ws_xpixel: 0,
        ws_ypixel: 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_and_exit_reach_the_master() {
        let mut command = Command::new("sh");
        command.args(["-c", "stty size; printf done"]);
        let mut pty = Pty::spawn(command, 12, 34).unwrap();
        let mut output = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while pty.read(Duration::from_millis(100), &mut output).unwrap() {
            assert!(Instant::now() < deadline, "terminal never closed");
        }
        let text = String::from_utf8_lossy(&output);
        assert!(text.contains("12 34"), "{text}");
        assert!(text.ends_with("done"), "{text}");
    }

    #[test]
    fn resize_reaches_the_program() {
        let mut command = Command::new("sh");
        command.args(["-c", "read line; stty size"]);
        let mut pty = Pty::spawn(command, 12, 34).unwrap();
        pty.resize(20, 60).unwrap();
        pty.write(b"\n").unwrap();
        let mut output = Vec::new();
        while pty.read(Duration::from_millis(100), &mut output).unwrap() {}
        assert!(String::from_utf8_lossy(&output).contains("20 60"));
    }

    #[test]
    fn close_leaves_output_to_the_terminal_reader() {
        let mut command = Command::new("sh");
        command.args(["-c", "stty raw -echo; printf READY; read line; dd if=/dev/zero bs=65536 count=16 2>/dev/null"]);
        let mut pty = Pty::spawn(command, 5, 20).unwrap();
        let mut ready = Vec::new();
        let deadline = Instant::now() + Duration::from_secs(5);
        while !ready.ends_with(b"READY") {
            assert!(Instant::now() < deadline);
            pty.read(Duration::from_millis(50), &mut ready).unwrap();
        }
        let mut reader = pty.reader().unwrap();
        let thread = std::thread::spawn(move || {
            // Let shutdown start while the terminal reader is between frames.
            std::thread::sleep(Duration::from_millis(50));
            let mut output = Vec::new();
            while read_available(&mut reader, &mut output).unwrap() {}
            output
        });
        pty.close(Some(b"\n"), Duration::from_secs(3)).unwrap();
        assert!(pty.exited().unwrap());
        assert_eq!(thread.join().unwrap().len(), 1048576);
    }

    #[test]
    fn close_without_detach_kills_the_program() {
        let mut command = Command::new("sleep");
        command.arg("30");
        let mut pty = Pty::spawn(command, 5, 5).unwrap();
        pty.close(None, Duration::from_secs(1)).unwrap();
        assert!(pty.exited().unwrap());
    }
}
