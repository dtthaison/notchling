pub fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        mac_version()
    }
    #[cfg(target_os = "linux")]
    {
        parse_os_release(&std::fs::read_to_string("/etc/os-release").ok()?)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        None
    }
}

#[cfg(target_os = "macos")]
fn mac_version() -> Option<String> {
    let name = c"kern.osproductversion";
    let mut len: libc::size_t = 0;
    let probed = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if probed != 0 || len == 0 {
        return None;
    }
    let mut buf = vec![0u8; len];
    let read = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            buf.as_mut_ptr().cast(),
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    buf.truncate(len);
    while buf.last() == Some(&0) {
        buf.pop();
    }
    String::from_utf8(buf).ok().filter(|v| dotted(v))
}

pub fn dotted(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 32
        && text
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

pub fn parse_os_release(text: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix("VERSION_ID="))
        .map(|value| value.trim().trim_matches('"').to_owned())
        .filter(|value| dotted(value))
}

#[cfg(test)]
#[path = "os_version_tests.rs"]
mod tests;
