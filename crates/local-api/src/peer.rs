#[cfg(unix)]
use std::io;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PeerInfo {
    pub uid: u32,
    pub pid: u32,
    pub exe: Option<PathBuf>,
    pub generation: Option<u64>,
}

impl PeerInfo {
    pub fn same_process(&self, later: &PeerInfo) -> bool {
        self.uid == later.uid
            && self.pid == later.pid
            && self.generation.is_some()
            && self.generation == later.generation
            && self.exe.is_some()
            && self.exe == later.exe
    }
}

pub fn same_user(peer_uid: u32, my_uid: u32) -> bool {
    peer_uid == my_uid
}

#[cfg(unix)]
pub fn current_uid() -> u32 {
    // SAFETY: getuid takes nothing and can't fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
pub fn current_uid() -> u32 {
    0
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[derive(Clone, Copy, Default)]
struct AuditToken {
    val: [u32; 8],
}

#[cfg(target_os = "macos")]
extern "C" {
    fn proc_pidpath_audittoken(
        token: *mut AuditToken,
        buffer: *mut libc::c_void,
        size: u32,
    ) -> libc::c_int;
}

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
    let mut token = AuditToken::default();
    let mut len = std::mem::size_of::<AuditToken>() as libc::socklen_t;
    // SAFETY: `token` and `len` are valid and `len` is `token`'s size.
    let got = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_LOCAL,
            libc::LOCAL_PEERTOKEN,
            (&mut token as *mut AuditToken).cast(),
            &mut len,
        )
    };
    if got != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut buf = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the token is a copy we own; the buffer is writable for its full length.
    let n =
        unsafe { proc_pidpath_audittoken(&mut token, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let exe = (n > 0).then(|| PathBuf::from(OsStr::from_bytes(&buf[..n as usize])));
    Ok(PeerInfo {
        uid,
        pid: token.val[5],
        exe,
        generation: Some(u64::from(token.val[7])),
    })
}

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
    let generation = std::fs::read_to_string(format!("/proc/{}/stat", cred.pid))
        .ok()
        .and_then(|stat| start_time(&stat));
    let exe = std::fs::read_link(format!("/proc/{}/exe", cred.pid)).ok();
    Ok(PeerInfo {
        uid: cred.uid,
        pid: u32::try_from(cred.pid).unwrap_or(0),
        exe,
        generation,
    })
}

#[cfg(any(target_os = "linux", test))]
fn start_time(stat: &str) -> Option<u64> {
    let rest = &stat[stat.rfind(')')? + 1..];
    rest.split_whitespace().nth(19)?.parse().ok()
}

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

    fn info(pid: u32, exe: Option<&str>, generation: Option<u64>) -> PeerInfo {
        PeerInfo {
            uid: 501,
            pid,
            exe: exe.map(PathBuf::from),
            generation,
        }
    }

    #[test]
    fn the_same_process_needs_pid_generation_and_file_to_agree() {
        let app = info(10, Some("/A/app"), Some(7));
        assert!(app.same_process(&app.clone()));
        assert!(!app.same_process(&info(10, Some("/A/app"), Some(8))));
        assert!(!app.same_process(&info(10, Some("/bin/sh"), Some(7))));
        assert!(!app.same_process(&info(11, Some("/A/app"), Some(7))));
        assert!(!app.same_process(&info(10, None, Some(7))));
        assert!(!info(10, Some("/A/app"), None).same_process(&info(10, Some("/A/app"), None)));
        let mut other_user = app.clone();
        other_user.uid = 0;
        assert!(!app.same_process(&other_user));
    }

    #[test]
    fn the_start_time_is_read_past_a_name_with_spaces_and_parentheses() {
        let stat =
            "4242 (a b) c) S 1 4242 4242 0 -1 4194560 100 0 0 0 1 2 0 0 20 0 1 0 987654 1000 50";
        assert_eq!(start_time(stat), Some(987654));
        assert_eq!(start_time("garbage"), None);
    }

    #[cfg(any(target_os = "macos", target_os = "linux"))]
    #[test]
    fn the_peer_of_a_socket_pair_is_this_process() {
        use std::os::fd::AsRawFd;
        let (a, _b) = std::os::unix::net::UnixStream::pair().unwrap();
        let peer = peer_of_fd(a.as_raw_fd()).unwrap();
        assert_eq!(peer.uid, current_uid());
        assert_eq!(peer.pid, std::process::id());
        assert!(peer.generation.is_some());
        assert!(peer.same_process(&peer_of_fd(a.as_raw_fd()).unwrap()));
        let me = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
        assert_eq!(std::fs::canonicalize(peer.exe.unwrap()).unwrap(), me);
    }
}
