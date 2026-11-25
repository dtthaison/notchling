use super::*;
use std::ffi::CString;
use std::os::fd::{FromRawFd, RawFd};
use std::os::unix::ffi::OsStrExt;

const SLEEP: &str = "/bin/sleep";

struct Forked {
    stream: UnixStream,
    pid: libc::pid_t,
    go: Option<RawFd>,
}

impl Forked {
    fn exec(&mut self) {
        if let Some(go) = self.go.take() {
            // SAFETY: our own pipe end, written once and closed.
            unsafe {
                libc::write(go, [1u8].as_ptr().cast(), 1);
                libc::close(go);
            }
        }
        let sleep = std::fs::canonicalize(SLEEP).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        while exe_of(self.pid).and_then(|p| std::fs::canonicalize(p).ok()) != Some(sleep.clone()) {
            assert!(Instant::now() < deadline, "the child never exec'd");
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

impl Drop for Forked {
    fn drop(&mut self) {
        // SAFETY: our own child.
        unsafe {
            if let Some(go) = self.go.take() {
                libc::close(go);
            }
            libc::kill(self.pid, libc::SIGKILL);
            libc::waitpid(self.pid, std::ptr::null_mut(), 0);
        }
    }
}

fn exe_of(pid: libc::pid_t) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use std::ffi::OsStr;
        let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
        // SAFETY: the buffer is writable for its full length.
        let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
        (n > 0).then(|| PathBuf::from(OsStr::from_bytes(&buf[..n as usize])))
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::fs::read_link(format!("/proc/{pid}/exe")).ok()
    }
}

fn connect_in_child(socket: &Path, hold: bool) -> Forked {
    let program = CString::new(SLEEP).unwrap();
    let secs = CString::new("30").unwrap();
    let argv = [program.as_ptr(), secs.as_ptr(), std::ptr::null()];
    // SAFETY: a zeroed sockaddr_un is valid; the path fits (asserted).
    let mut addr: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    addr.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = socket.as_os_str().as_bytes();
    assert!(bytes.len() < addr.sun_path.len());
    for (slot, b) in addr.sun_path.iter_mut().zip(bytes) {
        *slot = *b as libc::c_char;
    }
    let addr_len = std::mem::size_of::<libc::sockaddr_un>() as libc::socklen_t;
    let mut done = [0 as RawFd; 2];
    let mut go = [0 as RawFd; 2];
    // SAFETY: plain descriptor calls on valid out-arrays.
    let fd = unsafe {
        assert_eq!(libc::pipe(done.as_mut_ptr()), 0);
        assert_eq!(libc::pipe(go.as_mut_ptr()), 0);
        for d in [done[0], done[1], go[0], go[1]] {
            libc::fcntl(d, libc::F_SETFD, libc::FD_CLOEXEC);
        }
        let fd = libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0);
        assert!(fd >= 0);
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        fd
    };
    // SAFETY: the child calls only async-signal-safe functions and ends in execv or _exit.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0, "fork");
    if pid == 0 {
        // SAFETY: as above.
        unsafe {
            let ok = libc::connect(fd, (&addr as *const libc::sockaddr_un).cast(), addr_len) == 0;
            libc::write(done[1], [u8::from(ok)].as_ptr().cast(), 1);
            if hold {
                let mut b = [0u8; 1];
                libc::read(go[0], b.as_mut_ptr().cast(), 1);
            }
            libc::execv(program.as_ptr(), argv.as_ptr());
            libc::_exit(127);
        }
    }
    let mut ok = [0u8; 1];
    // SAFETY: our own pipe ends.
    unsafe {
        libc::close(done[1]);
        libc::close(go[0]);
        assert_eq!(libc::read(done[0], ok.as_mut_ptr().cast(), 1), 1);
        libc::close(done[0]);
    }
    assert_eq!(ok[0], 1, "the child could not connect");
    // SAFETY: `fd` is ours, open, and owned by nothing else in this process.
    let stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    Forked {
        stream,
        pid,
        go: hold.then_some(go[1]),
    }
}

fn hello_tier(stream: &UnixStream) -> String {
    let mut s = stream.try_clone().unwrap();
    send(&mut s, HELLO);
    let mut r = LineReader::new(stream.try_clone().unwrap());
    let v = reply(&mut r);
    v["ok"]["tier"].as_str().unwrap_or("closed").to_string()
}

fn sleep_is_the_app() -> Allowlist {
    Allowlist::new([PathBuf::from(SLEEP)])
}

fn roomy() -> Limits {
    Limits {
        max_clients: 64,
        ..fast()
    }
}

#[test]
fn a_peer_that_execs_between_accept_and_hello_is_not_the_app() {
    let run = start_with(sleep_is_the_app(), roomy());
    let mut forked = connect_in_child(&run.socket, true);
    let deadline = Instant::now() + Duration::from_secs(5);
    while run.handle.connections() < 1 {
        assert!(Instant::now() < deadline, "never accepted");
        std::thread::sleep(Duration::from_millis(5));
    }
    forked.exec();
    assert_eq!(hello_tier(&forked.stream), "agent");
}

#[cfg(target_os = "macos")]
#[test]
fn connect_fork_exec_never_gets_the_app_tier() {
    let run = start_with(sleep_is_the_app(), roomy());
    for round in 0..60u64 {
        let mut forked = connect_in_child(&run.socket, false);
        if round % 2 == 0 {
            forked.exec();
        }
        std::thread::sleep(Duration::from_millis((round % 4) * 15));
        assert_eq!(hello_tier(&forked.stream), "agent", "round {round}");
    }
}

#[cfg(target_os = "macos")]
#[test]
fn an_app_tier_request_from_another_process_is_refused() {
    let run = start_with(Allowlist::new([me()]), fast());
    let (mut s, mut r) = raw(&run.socket);
    send(&mut s, HELLO);
    assert_eq!(reply(&mut r)["ok"]["tier"], "app");
    let line = b"{\"id\":2,\"op\":\"settings.set\",\"args\":{}}\n";
    let fd = std::os::fd::AsRawFd::as_raw_fd(&s);
    // SAFETY: the child only writes and exits.
    let pid = unsafe { libc::fork() };
    assert!(pid >= 0);
    if pid == 0 {
        // SAFETY: as above.
        unsafe {
            libc::write(fd, line.as_ptr().cast(), line.len());
            libc::_exit(0);
        }
    }
    // SAFETY: our own child.
    unsafe { libc::waitpid(pid, std::ptr::null_mut(), 0) };
    let v = reply(&mut r);
    assert_eq!(v["err"]["code"], "forbidden", "{v}");
    send(&mut s, r#"{"id":3,"op":"settings.set","args":{}}"#);
    assert_eq!(reply(&mut r)["ok"], "done");
}
