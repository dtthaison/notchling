use notchling_engine::sessions::fresh::Process;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
const FD_VNODE_PATH_INFO: libc::c_int = 2;
#[cfg(target_os = "macos")]
const FD_VNODE_PATH_INFO_SIZE: usize = 1200;
#[cfg(target_os = "macos")]
const FD_VNODE_PATH_OFFSET: usize = 176;

#[cfg(target_os = "macos")]
pub fn start_time(pid: u32) -> Option<u64> {
    let pid = libc::c_int::try_from(pid).ok()?;
    // SAFETY: proc_bsdinfo is plain data; all zeroes is a valid value.
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: the buffer is a proc_bsdinfo of the size passed.
    let n = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            (&mut info as *mut libc::proc_bsdinfo).cast(),
            size,
        )
    };
    (n == size).then(|| info.pbi_start_tvsec * 1_000_000 + info.pbi_start_tvusec)
}

#[cfg(target_os = "linux")]
pub fn start_time(pid: u32) -> Option<u64> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    let after_name = stat.rsplit_once(')')?.1;
    after_name.split_whitespace().nth(19)?.parse().ok()
}

#[cfg(target_os = "macos")]
pub fn started_ms(pid: u32) -> Option<i64> {
    i64::try_from(start_time(pid)? / 1_000).ok()
}

#[cfg(target_os = "linux")]
pub fn started_ms(pid: u32) -> Option<i64> {
    let ticks = start_time(pid)?;
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let boot: u64 = stat
        .lines()
        .find_map(|line| line.strip_prefix("btime "))?
        .trim()
        .parse()
        .ok()?;
    // SAFETY: sysconf only reads a system setting.
    let hz = unsafe { libc::sysconf(libc::_SC_CLK_TCK) };
    let hz = u64::try_from(hz).ok().filter(|hz| *hz > 0)?;
    i64::try_from(boot.checked_mul(1_000)? + ticks.checked_mul(1_000)? / hz).ok()
}

#[cfg(unix)]
pub fn process(pid: u32) -> Process {
    let Some(target) = libc::pid_t::try_from(pid).ok().filter(|pid| *pid > 0) else {
        return Process::Gone;
    };
    // SAFETY: signal 0 delivers nothing; `kill` only checks that the process exists.
    if unsafe { libc::kill(target, 0) } != 0 {
        return match std::io::Error::last_os_error().raw_os_error() {
            Some(libc::EPERM) => Process::Other,
            _ => Process::Gone,
        };
    }
    Process::Ours {
        started_ms: started_ms(pid),
        ticks: cfg!(target_os = "linux").then(|| start_time(pid)).flatten(),
    }
}

#[cfg(not(unix))]
pub fn process(_pid: u32) -> Process {
    Process::Ours {
        started_ms: None,
        ticks: None,
    }
}

#[cfg(target_os = "macos")]
pub fn argv(pid: u32) -> Option<Vec<String>> {
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut mib = [libc::CTL_KERN, libc::KERN_PROCARGS2, pid];
    let mut size: libc::size_t = 0;
    // SAFETY: a null buffer only asks for the size; mib has the length passed.
    let r = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            std::ptr::null_mut(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 || size < 4 {
        return None;
    }
    let mut buf = vec![0u8; size];
    // SAFETY: the buffer holds `size` writable bytes.
    let r = unsafe {
        libc::sysctl(
            mib.as_mut_ptr(),
            mib.len() as libc::c_uint,
            buf.as_mut_ptr().cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if r != 0 {
        return None;
    }
    buf.truncate(size);
    parse_procargs2(&buf)
}

#[cfg(any(target_os = "macos", test))]
pub(crate) fn parse_procargs2(buf: &[u8]) -> Option<Vec<String>> {
    let argc = u32::from_ne_bytes(buf.get(..4)?.try_into().ok()?) as usize;
    let mut rest = buf.get(4..)?;
    let exec_end = rest.iter().position(|b| *b == 0)?;
    rest = &rest[exec_end..];
    let start = rest.iter().position(|b| *b != 0)?;
    rest = &rest[start..];
    let mut out = Vec::with_capacity(argc);
    for part in rest.split(|b| *b == 0).take(argc) {
        out.push(String::from_utf8_lossy(part).into_owned());
    }
    (out.len() == argc && argc > 0).then_some(out)
}

#[cfg(target_os = "linux")]
pub fn argv(pid: u32) -> Option<Vec<String>> {
    let raw = std::fs::read(format!("/proc/{pid}/cmdline")).ok()?;
    let raw = raw.strip_suffix(&[0]).unwrap_or(&raw);
    if raw.is_empty() {
        return None;
    }
    Some(
        raw.split(|b| *b == 0)
            .map(|p| String::from_utf8_lossy(p).into_owned())
            .collect(),
    )
}

#[cfg(target_os = "macos")]
pub fn stdout_path(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut buf = [0u8; FD_VNODE_PATH_INFO_SIZE];
    // SAFETY: the buffer is writable and its size is passed.
    let n = unsafe {
        libc::proc_pidfdinfo(
            pid,
            1,
            FD_VNODE_PATH_INFO,
            buf.as_mut_ptr().cast(),
            buf.len() as libc::c_int,
        )
    };
    if n != FD_VNODE_PATH_INFO_SIZE as libc::c_int {
        return None;
    }
    let raw: Vec<u8> = buf[FD_VNODE_PATH_OFFSET..]
        .iter()
        .copied()
        .take_while(|b| *b != 0)
        .collect();
    (!raw.is_empty()).then(|| PathBuf::from(std::ffi::OsStr::from_bytes(&raw)))
}

#[cfg(target_os = "linux")]
pub fn stdout_path(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/fd/1"))
        .ok()
        .filter(|p| p.is_absolute())
}

#[cfg(target_os = "macos")]
pub fn exe(pid: u32) -> Option<PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let pid = libc::c_int::try_from(pid).ok()?;
    let mut buf = [0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is writable and its size is passed.
    let n = unsafe { libc::proc_pidpath(pid, buf.as_mut_ptr().cast(), buf.len() as u32) };
    let n = usize::try_from(n)
        .ok()
        .filter(|n| *n > 0 && *n <= buf.len())?;
    Some(PathBuf::from(std::ffi::OsStr::from_bytes(&buf[..n])))
}

#[cfg(target_os = "linux")]
pub fn exe(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn start_time(_pid: u32) -> Option<u64> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn started_ms(_pid: u32) -> Option<i64> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn argv(_pid: u32) -> Option<Vec<String>> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn stdout_path(_pid: u32) -> Option<PathBuf> {
    None
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
pub fn exe(_pid: u32) -> Option<PathBuf> {
    None
}
