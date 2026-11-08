//! Who is on the other end of a socket: its user, process and executable (decision 0007).
//!
//! macOS: `getpeereid` and `LOCAL_PEERPID`, then `proc_pidpath`. Linux: `SO_PEERCRED`, then
//! `/proc/<pid>/exe`. Windows (`GetNamedPipeClientProcessId`) arrives with the named pipe.

#[cfg(unix)]
use std::io;
use std::path::PathBuf;

/// The peer of a connection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
    pub uid: u32,
    pub pid: u32,
    /// The file the process runs, as the kernel reports it (symlinks already resolved); `None` when
    /// the kernel would not say (the process may have exited).
    pub exe: Option<PathBuf>,
}

/// Whether the peer runs as the daemon's user. Anyone else is closed on at once. A uid mismatch
/// can't be produced in a test without a second account, so this comparison is the tested part.
pub fn same_user(peer_uid: u32, my_uid: u32) -> bool {
    peer_uid == my_uid
}

/// The user this process runs as.
#[cfg(unix)]
pub fn current_uid() -> u32 {
    // SAFETY: getuid takes nothing and can't fail.
    unsafe { libc::getuid() }
}

/// The user this process runs as. Windows has no uid; the named pipe's ACL does that job there.
#[cfg(not(unix))]
pub fn current_uid() -> u32 {
    0
}

/// The peer of the connected Unix socket `fd`.
#[cfg(target_os = "macos")]
pub fn peer_of_fd(fd: std::os::fd::RawFd) -> io::Result<PeerInfo> {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: both out-pointers are valid for the call.
    if unsafe { libc::getpeereid(fd, &mut uid, &mut gid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pid: libc::pid_t = 0;
    let mut len = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: `pid` and `len` are valid and `len` is `pid`'s size.
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERPID,
            (&mut pid as *mut libc::pid_t).cast(),
            &mut len,
        )
    };
    if got != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is writable for its full length, which is what we pass.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let exe = (n > 0).then(|| PathBuf::from(OsStr::from_bytes(&buf[..n as usize])));
    Ok(PeerInfo {
        uid,
        pid: u32::try_from(pid).unwrap_or(0),
        exe,
    })
}

/// The peer of the connected Unix socket `fd`.
#[cfg(target_os = "linux")]
pub fn peer_of_fd(fd: std::os::fd::RawFd) -> io::Result<PeerInfo> {
    let mut cred = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    // SAFETY: `cred` and `len` are valid and `len` is `cred`'s size.
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&mut cred as *mut libc::ucred).cast(),
            &mut len,
        )
    };
    if got != 0 {
        return Err(io::Error::last_os_error());
    }
    let exe = std::fs::read_link(format!("/proc/{}/exe", cred.pid)).ok();
    Ok(PeerInfo {
        uid: cred.uid,
        pid: u32::try_from(cred.pid).unwrap_or(0),
        exe,
    })
}

/// Other Unix systems: not supported, so every connection is refused.
#[cfg(all(unix, not(any(target_os = "macos", target_os = "linux"))))]
pub fn peer_of_fd(_fd: std::os::fd::RawFd) -> io::Result<PeerInfo> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "peer identity on this system",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_user_compares_uids() {
        assert!(same_user(501, 501));
        assert!(!same_user(0, 501));
        assert!(!same_user(501, 0));
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_peer_of_a_socket_pair_is_this_process() {
        use std::os::fd::AsRawFd;
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = peer_of_fd(a.as_raw_fd()).unwrap();
        assert_eq!(peer.uid, current_uid());
        assert_eq!(peer.pid, std::process::id());
        let me = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(std::fs::canonicalize(peer.exe.unwrap()).unwrap(), me);
    }
}
