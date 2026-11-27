use super::{PtyCommand, PtyError, ReadOutcome, Signal, StartStep};
use std::ffi::{CStr, CString};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Pty {
    master: AtomicI32,
    pid: libc::pid_t,
    status: Mutex<Option<i32>>,
}

#[cfg(target_os = "macos")]
const TIOCSCTTY: libc::c_ulong = 0x2000_7461;
#[cfg(target_os = "linux")]
const TIOCSCTTY: libc::c_ulong = libc::TIOCSCTTY as libc::c_ulong;

#[cfg(not(target_os = "linux"))]
static PTSNAME: Mutex<()> = Mutex::new(());

fn cstring(bytes: &[u8]) -> Result<CString, PtyError> {
    CString::new(bytes).map_err(|_| PtyError::Nul)
}

fn last_error() -> io::Error {
    io::Error::last_os_error()
}

struct Fd(libc::c_int);

impl Fd {
    fn raw(&self) -> libc::c_int {
        self.0
    }

    fn into_raw(self) -> libc::c_int {
        let fd = self.0;
        std::mem::forget(self);
        fd
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        // SAFETY: the descriptor is ours and closed once, here.
        unsafe { libc::close(self.0) };
    }
}

fn set_cloexec(fd: libc::c_int) -> Result<(), PtyError> {
    // SAFETY: plain fcntl calls on a descriptor we own.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFD);
        if flags < 0 {
            return Err(PtyError::Spawn(last_error()));
        }
        if flags & libc::FD_CLOEXEC == 0
            && libc::fcntl(fd, libc::F_SETFD, flags | libc::FD_CLOEXEC) < 0
        {
            return Err(PtyError::Spawn(last_error()));
        }
    }
    Ok(())
}

fn set_nonblocking(fd: libc::c_int) -> Result<(), PtyError> {
    // SAFETY: plain fcntl calls on a descriptor we own.
    unsafe {
        let flags = libc::fcntl(fd, libc::F_GETFL);
        if flags < 0 || libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
            return Err(PtyError::Spawn(last_error()));
        }
    }
    Ok(())
}

fn cloexec_pipe() -> Result<(Fd, Fd), PtyError> {
    let mut ends = [0 as libc::c_int; 2];
    #[cfg(target_os = "linux")]
    // SAFETY: ends holds the two descriptors pipe2 writes.
    let made = unsafe { libc::pipe2(ends.as_mut_ptr(), libc::O_CLOEXEC) };
    #[cfg(not(target_os = "linux"))]
    // SAFETY: ends holds the two descriptors pipe writes.
    let made = unsafe { libc::pipe(ends.as_mut_ptr()) };
    if made != 0 {
        return Err(PtyError::Spawn(last_error()));
    }
    let (read, write) = (Fd(ends[0]), Fd(ends[1]));
    set_cloexec(read.raw())?;
    set_cloexec(write.raw())?;
    Ok((read, write))
}

#[cfg(target_os = "linux")]
fn slave_name(master: libc::c_int) -> Result<CString, PtyError> {
    let mut name = [0 as libc::c_char; 128];
    // SAFETY: name is valid for its length.
    let failed = unsafe { libc::ptsname_r(master, name.as_mut_ptr(), name.len()) };
    if failed != 0 {
        return Err(PtyError::Spawn(io::Error::from_raw_os_error(failed)));
    }
    // SAFETY: ptsname_r wrote a NUL-terminated string.
    Ok(unsafe { CStr::from_ptr(name.as_ptr()) }.to_owned())
}

#[cfg(not(target_os = "linux"))]
fn slave_name(master: libc::c_int) -> Result<CString, PtyError> {
    let _one_at_a_time = PTSNAME.lock().unwrap_or_else(|p| p.into_inner());
    // SAFETY: master is open; the result is copied before the lock drops.
    let name = unsafe { libc::ptsname(master) };
    if name.is_null() {
        return Err(PtyError::Spawn(last_error()));
    }
    // SAFETY: ptsname returned a NUL-terminated string.
    Ok(unsafe { CStr::from_ptr(name) }.to_owned())
}

fn open_pty(size: &libc::winsize) -> Result<(Fd, Fd), PtyError> {
    // SAFETY: no pointers.
    let master = unsafe { libc::posix_openpt(libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC) };
    if master < 0 {
        return Err(PtyError::Spawn(last_error()));
    }
    let master = Fd(master);
    set_cloexec(master.raw())?;
    set_nonblocking(master.raw())?;
    // SAFETY: master is a pty master.
    if unsafe { libc::grantpt(master.raw()) } != 0 || unsafe { libc::unlockpt(master.raw()) } != 0 {
        return Err(PtyError::Spawn(last_error()));
    }
    let name = slave_name(master.raw())?;
    // SAFETY: name is a NUL-terminated path.
    let slave = unsafe {
        libc::open(
            name.as_ptr(),
            libc::O_RDWR | libc::O_NOCTTY | libc::O_CLOEXEC,
        )
    };
    if slave < 0 {
        return Err(PtyError::Spawn(last_error()));
    }
    let mut slave = Fd(slave);
    if slave.raw() <= 2 {
        // SAFETY: duplicates above stdio, close-on-exec.
        let high = unsafe { libc::fcntl(slave.raw(), libc::F_DUPFD_CLOEXEC, 3) };
        if high < 0 {
            return Err(PtyError::Spawn(last_error()));
        }
        slave = Fd(high);
    }
    set_cloexec(slave.raw())?;
    // SAFETY: size is a valid winsize for TIOCSWINSZ.
    if unsafe { libc::ioctl(master.raw(), libc::TIOCSWINSZ, size) } != 0 {
        return Err(PtyError::Spawn(last_error()));
    }
    Ok((master, slave))
}

const REPORT_LEN: usize = 1 + std::mem::size_of::<i32>();

fn step_code(step: StartStep) -> u8 {
    match step {
        StartStep::Terminal => 1,
        StartStep::Folder => 2,
        StartStep::Program => 3,
    }
}

fn step_from(code: u8) -> StartStep {
    match code {
        1 => StartStep::Terminal,
        2 => StartStep::Folder,
        _ => StartStep::Program,
    }
}

unsafe fn fail_start(report_fd: libc::c_int, step: StartStep) -> ! {
    let errno = io::Error::last_os_error().raw_os_error().unwrap_or(0);
    let mut report = [0u8; REPORT_LEN];
    report[0] = step_code(step);
    report[1..].copy_from_slice(&errno.to_ne_bytes());
    // SAFETY: report is valid for REPORT_LEN bytes. A failed write leaves the parent with an empty
    // report, which reads as success; nothing better can be done here.
    unsafe {
        libc::write(report_fd, report.as_ptr().cast(), report.len());
        libc::_exit(127)
    }
}

fn read_start_report(report: &Fd) -> Option<(StartStep, i32)> {
    let mut buf = [0u8; REPORT_LEN];
    let mut have = 0;
    while have < REPORT_LEN {
        // SAFETY: the slice from `have` is valid for the bytes asked for.
        let n = unsafe {
            libc::read(
                report.raw(),
                buf[have..].as_mut_ptr().cast(),
                REPORT_LEN - have,
            )
        };
        if n > 0 {
            have += n as usize;
        } else if n == 0 || last_error().raw_os_error() != Some(libc::EINTR) {
            break;
        }
    }
    if have < REPORT_LEN {
        return None;
    }
    let mut errno = [0u8; std::mem::size_of::<i32>()];
    errno.copy_from_slice(&buf[1..]);
    Some((step_from(buf[0]), i32::from_ne_bytes(errno)))
}

fn reap(pid: libc::pid_t) {
    loop {
        let mut raw: libc::c_int = 0;
        // SAFETY: raw is a valid out pointer; pid is our own child.
        let r = unsafe { libc::waitpid(pid, &mut raw, 0) };
        if r >= 0 || last_error().raw_os_error() != Some(libc::EINTR) {
            return;
        }
    }
}

impl Pty {
    pub fn spawn(command: &PtyCommand) -> Result<Pty, PtyError> {
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
        let size = libc::winsize {
            ws_row: command.rows,
            ws_col: command.cols,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        let (master, slave) = open_pty(&size)?;
        let (report_read, report_write) = cloexec_pipe()?;
        let (slave_fd, report_fd) = (slave.raw(), report_write.raw());

        // SAFETY: the child below only makes async-signal-safe calls on data made before the fork.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            // SAFETY: each call is async-signal-safe and the pointers were made before the fork.
            unsafe {
                libc::sigprocmask(libc::SIG_SETMASK, &no_signals, std::ptr::null_mut());
                libc::signal(libc::SIGPIPE, libc::SIG_DFL);
                libc::setsid();
                if libc::ioctl(slave_fd, TIOCSCTTY as _, 0) != 0 {
                    fail_start(report_fd, StartStep::Terminal);
                }
                for target in 0..3 {
                    if libc::dup2(slave_fd, target) < 0 {
                        fail_start(report_fd, StartStep::Terminal);
                    }
                }
                let mut fd = 3;
                while fd < max_fd {
                    if fd != report_fd {
                        libc::close(fd);
                    }
                    fd += 1;
                }
                if let Some(dir) = &cwd {
                    if libc::chdir(dir.as_ptr()) != 0 {
                        fail_start(report_fd, StartStep::Folder);
                    }
                }
                libc::execve(program.as_ptr(), argv_ptrs.as_ptr(), envp_ptrs.as_ptr());
                fail_start(report_fd, StartStep::Program)
            }
        }
        if pid < 0 {
            return Err(PtyError::Spawn(last_error()));
        }
        drop(slave);
        drop(report_write);
        drop((argv_ptrs, envp_ptrs));
        if let Some((step, errno)) = read_start_report(&report_read) {
            reap(pid);
            return Err(PtyError::Start {
                step,
                error: io::Error::from_raw_os_error(errno),
            });
        }
        Ok(Pty {
            master: AtomicI32::new(master.into_raw()),
            pid,
            status: Mutex::new(None),
        })
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    fn try_read(&self, buf: &mut [u8]) -> Result<Option<ReadOutcome>, PtyError> {
        loop {
            // SAFETY: buf is valid for buf.len() bytes and master stays open while self lives.
            let n = unsafe { libc::read(self.fd(), buf.as_mut_ptr().cast(), buf.len()) };
            if n > 0 {
                return Ok(Some(ReadOutcome::Data(n as usize)));
            }
            if n == 0 {
                return Ok(Some(ReadOutcome::Closed));
            }
            let error = last_error();
            match error.raw_os_error() {
                Some(libc::EINTR) => continue,
                Some(libc::EAGAIN) => return Ok(None),
                Some(libc::EIO) => return Ok(Some(ReadOutcome::Closed)),
                _ => return Err(PtyError::Io(error)),
            }
        }
    }

    fn wait_ready(&self, events: libc::c_short, ms: libc::c_int) -> Result<bool, PtyError> {
        loop {
            let mut pfd = libc::pollfd {
                fd: self.fd(),
                events,
                revents: 0,
            };
            // SAFETY: one valid pollfd.
            let ready = unsafe { libc::poll(&mut pfd, 1, ms) };
            if ready >= 0 {
                return Ok(ready > 0);
            }
            let error = last_error();
            if error.raw_os_error() != Some(libc::EINTR) {
                return Err(PtyError::Io(error));
            }
        }
    }

    pub fn read(&self, buf: &mut [u8]) -> Result<ReadOutcome, PtyError> {
        loop {
            if let Some(outcome) = self.try_read(buf)? {
                return Ok(outcome);
            }
            self.wait_ready(libc::POLLIN, -1)?;
        }
    }

    pub fn read_timeout(&self, buf: &mut [u8], timeout: Duration) -> Result<ReadOutcome, PtyError> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let ms = left.as_millis().min(i32::MAX as u128) as libc::c_int;
            if self.wait_ready(libc::POLLIN, ms)? {
                if let Some(outcome) = self.try_read(buf)? {
                    return Ok(outcome);
                }
            }
            if Instant::now() >= deadline {
                return Ok(ReadOutcome::Timeout);
            }
        }
    }

    pub fn write_all(&self, bytes: &[u8]) -> Result<(), PtyError> {
        self.write_until(bytes, None)
    }

    pub fn write_all_timeout(&self, bytes: &[u8], timeout: Duration) -> Result<(), PtyError> {
        self.write_until(bytes, Some(Instant::now() + timeout))
    }

    fn write_until(&self, bytes: &[u8], deadline: Option<Instant>) -> Result<(), PtyError> {
        let mut offset = 0;
        while offset < bytes.len() {
            let rest = &bytes[offset..];
            // SAFETY: rest is valid for rest.len() bytes.
            let n = unsafe { libc::write(self.fd(), rest.as_ptr().cast(), rest.len()) };
            if n > 0 {
                offset += n as usize;
                continue;
            }
            let error = last_error();
            match error.raw_os_error() {
                Some(libc::EINTR) if n < 0 => {}
                Some(libc::EAGAIN) if n < 0 => {
                    let ms = match deadline {
                        None => -1,
                        Some(deadline) => {
                            let left = deadline.saturating_duration_since(Instant::now());
                            if left.is_zero() {
                                return Err(PtyError::Timeout);
                            }
                            left.as_millis().clamp(1, i32::MAX as u128) as libc::c_int
                        }
                    };
                    self.wait_ready(libc::POLLOUT, ms)?;
                }
                _ => return Err(PtyError::Io(error)),
            }
        }
        Ok(())
    }

    pub fn resize(&self, rows: u16, cols: u16) -> Result<(), PtyError> {
        let size = libc::winsize {
            ws_row: rows.max(1),
            ws_col: cols.max(1),
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        // SAFETY: size is a valid winsize for TIOCSWINSZ.
        if unsafe { libc::ioctl(self.fd(), libc::TIOCSWINSZ, &size) } != 0 {
            return Err(PtyError::Io(last_error()));
        }
        Ok(())
    }

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
                    return Ok(None);
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

    pub fn wait(&self) -> Result<i32, PtyError> {
        loop {
            if let Some(code) = self.try_wait()? {
                return Ok(code);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }

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

    fn fd(&self) -> libc::c_int {
        self.master.load(Ordering::Acquire)
    }

    fn hang_up(&self) {
        let fd = self.master.swap(-1, Ordering::AcqRel);
        if fd >= 0 {
            // SAFETY: fd was ours, and the swap means it is closed exactly once.
            unsafe { libc::close(fd) };
        }
    }

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
        self.hang_up();
        let _ = self.signal(Signal::Kill);
        self.wait().ok()
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        self.hang_up();
        self.terminate(Duration::from_secs(2));
    }
}
