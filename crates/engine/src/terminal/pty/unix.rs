//! `forkpty` on macOS and Linux (Linux's comes from libc's util part, which glibc 2.34 folded in).

use super::{PtyCommand, PtyError, ReadOutcome, Signal};
use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A child on a pty. Reading, writing and resizing take `&self`, so one thread can read while another
/// types. Dropping it ends the child (SIGTERM, SIGKILL after two seconds) and closes the pty.
#[derive(Debug)]
pub struct Pty {
    master: libc::c_int,
    pid: libc::pid_t,
    /// The exit status once reaped, so the pid is never signalled after it may have been reused.
    status: Mutex<Option<i32>>,
}

fn cstring(bytes: &[u8]) -> Result<CString, PtyError> {
    CString::new(bytes).map_err(|_| PtyError::Nul)
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

impl Pty {
    pub fn spawn(command: &PtyCommand) -> Result<Pty, PtyError> {
        // Everything the child needs is made here: after fork only async-signal-safe calls are allowed.
        let program = cstring(command.program.as_os_str().as_bytes())?;
        let mut argv = vec![program.clone()];
        for arg in &command.args {
            argv.push(cstring(arg.as_bytes())?);
        }
        let mut envp = Vec::new();
        for (key, value) in command.child_env() {
            let mut pair = key.as_bytes().to_vec();
            pair.push(b'=');
            pair.extend_from_slice(value.as_bytes());
            envp.push(cstring(&pair)?);
        }
        let cwd = match &command.cwd {
            Some(dir) => Some(cstring(dir.as_os_str().as_bytes())?),
            None => None,
        };
        let mut argv_ptrs: Vec<*const libc::c_char> = argv.iter().map(|a| a.as_ptr()).collect();
        argv_ptrs.push(std::ptr::null());
        let mut envp_ptrs: Vec<*const libc::c_char> = envp.iter().map(|e| e.as_ptr()).collect();
        envp_ptrs.push(std::ptr::null());
        // SAFETY: sysconf has no preconditions.
        let open_max = unsafe { libc::sysconf(libc::_SC_OPEN_MAX) };
        let max_fd = if open_max <= 0 {
            4096
        } else {
            open_max.min(4096) as libc::c_int
        };
        // SAFETY: an all-zero sigset_t is a valid value to hand sigemptyset.
        let mut no_signals: libc::sigset_t = unsafe { std::mem::zeroed() };
        // SAFETY: no_signals is a valid sigset_t.
        unsafe { libc::sigemptyset(&mut no_signals) };
        let mut size = libc::winsize {
            ws_row: command.rows,
            ws_col: command.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let mut master: libc::c_int = -1;
        // SAFETY: the pointers are valid for the call; termp may be null.
        let pid = unsafe {
            libc::forkpty(
                &mut master,
                std::ptr::null_mut(),
                std::ptr::null_mut::<libc::termios>(),
                &mut size,
            )
        };
        if pid == 0 {
            // The child: reset the signal mask and SIGPIPE (Rust ignores it, and an ignored signal
            // survives exec), close what it inherited, then exec. Nothing here allocates.
            // SAFETY: each call is async-signal-safe and the pointers were made before the fork.
            unsafe {
                libc::sigprocmask(libc::SIG_SETMASK, &no_signals, std::ptr::null_mut());
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);
                let mut fd = 3;
                while fd < max_fd {
                    libc::close(fd);
                    fd += 1;
                }
                if let Some(dir) = &cwd {
                    libc::chdir(dir.as_ptr());
                }
                libc::execve(program.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
                libc::_exit(127);
            }
        }
        if pid < 0 {
            return Err(PtyError::Spawn(last_error()));
        }
        // SAFETY: master is the fd forkpty just opened.
        unsafe { libc::fcntl(master, libc::F_SETFD, libc::FD_CLOEXEC) };
        // Keep the pointer vectors alive past the fork (the child used them).
        drop((argv_ptrs, envp_ptrs));
        Ok(Pty {
            master,
            pid,
            status: Mutex::new(None),
        })
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// Blocks until there is output or the child's side closes.
    pub fn read(&self, buf: &mut [u8]) -> Result<ReadOutcome, PtyError> {
        loop {
            // SAFETY: buf is valid for buf.len() bytes and master stays open while self lives.
            let n = unsafe { libc::read(self.master, buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                return Ok(ReadOutcome::Data(n as usize));
            }
            if n == 0 {
                return Ok(ReadOutcome::Closed);
            }
            let error = last_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => continue,
                // Linux reports the closed side as EIO.
                Some(libc::EIO) => return Ok(ReadOutcome::Closed),
                _ => return Err(PtyError::Io(error)),
            }
        }
    }

    /// Waits up to `timeout` for output.
    pub fn read_timeout(&self, buf: &mut [u8], timeout: Duration) -> Result<ReadOutcome, PtyError> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let ms = left.as_millis().min(i32::MAX as u128) as libc::c_int;
            let mut pfd = libc::pollfd {
                fd: self.master,
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: one valid pollfd.
            let ready = unsafe { libc::poll(&mut pfd, 1, ms) };
            if ready > 0 {
                return self.read(buf);
            }
            if ready < 0 {
                let error = last_error();
                if error.raw_os_error() != Some(libc::EINTR) {
                    return Err(PtyError::Io(error));
                }
            }
            if Instant::now() >= deadline {
                return Ok(ReadOutcome::Timeout);
            }
        }
    }

    pub fn write_all(&self, bytes: &[u8]) -> Result<(), PtyError> {
        let mut offset = 0;
        while offset < bytes.len() {
            let rest = &bytes[offset..];
            // SAFETY: rest is valid for rest.len() bytes.
            let n = unsafe { libc::write(self.master, rest.as_ptr().cast(), rest.len()) };
            if n > 0 {
                offset += n as usize;
                continue;
            }
            let error = last_error();
            if n < 0 && error.raw_os_error() == Some(libc::EINTR) {
                continue;
            }
            return Err(PtyError::Io(error));
        }
        Ok(())
    }

    /// Sets the window size; the child gets SIGWINCH.
    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let size = libc::winsize {
            ws_row: rows.max(1),
            ws_col: cols.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: size is a valid winsize for TIOCSWINSZ.
        if unsafe { libc::ioctl(self.master, libc::TIOCSWINSZ, &size) } != 0 {
            return Err(PtyError::Io(last_error()));
        }
        Ok(())
    }

    /// The exit status if the child has ended: its code, or 128 plus the signal that ended it.
    pub fn try_wait(&self) -> Result<Option<i32>, PtyError> {
        let mut status = self
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(code) = *status {
            return Ok(Some(code));
        }
        loop {
            let mut raw: libc::c_int = 0;
            // SAFETY: raw is a valid out pointer; pid is our own child.
            let r = unsafe { libc::waitpid(self.pid, &mut raw, libc::WNOHANG) };
            if r == self.pid {
                let code = if libc::WIFEXITED(raw) {
                    libc::WEXITSTATUS(raw)
                } else if libc::WIFSIGNALED(raw) {
                    128 + libc::WTERMSIG(raw)
                } else {
                    return Ok(None); // stopped, not ended
                };
                *status = Some(code);
                return Ok(Some(code));
            }
            if r == 0 {
                return Ok(None);
            }
            let error = last_error();
            if error.raw_os_error() != Some(libc::EINTR) {
                return Err(PtyError::Io(error));
            }
        }
    }

    /// Blocks until the child ends.
    pub fn wait(&self) -> Result<i32, PtyError> {
        loop {
            if let Some(code) = self.try_wait()? {
                return Ok(code);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Signals the child, unless it has already been reaped.
    pub fn signal(&self, signal: Signal) -> Result<(), PtyError> {
        let status = self
            .status
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if status.is_some() {
            return Ok(());
        }
        let number = match signal {
            Signal::Term => libc::SIGTERM,
            Signal::Kill => libc::SIGKILL,
            Signal::Int => libc::SIGINT,
            Signal::Hup => libc::SIGHUP,
        };
        // SAFETY: pid is our unreaped child, so it can't have been reused.
        if unsafe { libc::kill(self.pid, number) } != 0 {
            return Err(PtyError::Io(last_error()));
        }
        Ok(())
    }

    /// SIGTERM, then SIGKILL if it is still there after `grace`; returns the exit status.
    pub fn terminate(&self, grace: Duration) -> Option<i32> {
        if let Ok(Some(code)) = self.try_wait() {
            return Some(code);
        }
        let _ = self.signal(Signal::Term);
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            match self.try_wait() {
                Ok(Some(code)) => return Some(code),
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(_) => return None,
            }
        }
        let _ = self.signal(Signal::Kill);
        self.wait().ok()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.terminate(Duration::from_secs(2));
        // SAFETY: master is ours and closed once, here.
        unsafe { libc::close(self.master) };
    }
}
