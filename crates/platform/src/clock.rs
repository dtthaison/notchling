use crate::{Error, Result};

#[cfg(target_os = "macos")]
#[repr(C)]
struct Timebase {
    numer: u32,
    denom: u32,
}

#[cfg(target_os = "macos")]
extern "C" {
    fn mach_continuous_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> libc::c_int;
}

#[cfg(target_os = "macos")]
pub fn boot_ms() -> Result<u64> {
    let mut base = Timebase { numer: 0, denom: 0 };
    if unsafe { mach_timebase_info(&mut base) } != 0 || base.denom == 0 {
        return Err(Error::System("mach_timebase_info failed".into()));
    }
    let ticks = unsafe { mach_continuous_time() } as u128;
    let nanos = ticks * u128::from(base.numer) / u128::from(base.denom);
    Ok((nanos / 1_000_000) as u64)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn boot_ms() -> Result<u64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    if unsafe { libc::clock_gettime(libc::CLOCK_BOOTTIME, &mut ts) } != 0 {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    Ok(ts.tv_sec as u64 * 1000 + ts.tv_nsec as u64 / 1_000_000)
}

#[cfg(not(unix))]
pub fn boot_ms() -> Result<u64> {
    Err(Error::Unsupported("a boot clock"))
}

#[cfg(target_os = "macos")]
pub fn boot_id() -> Result<String> {
    let mut tv = libc::timeval {
        tv_sec: 0,
        tv_usec: 0,
    };
    let mut size = std::mem::size_of::<libc::timeval>();
    let name = c"kern.boottime";
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut tv as *mut libc::timeval).cast(),
            &mut size,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 || size != std::mem::size_of::<libc::timeval>() {
        return Err(Error::Io(std::io::Error::last_os_error()));
    }
    Ok(format!("{}.{:06}", tv.tv_sec, tv.tv_usec))
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn boot_id() -> Result<String> {
    let text = std::fs::read_to_string("/proc/sys/kernel/random/boot_id")?;
    let id = text.trim();
    if id.is_empty() {
        return Err(Error::System("the boot id is empty".into()));
    }
    Ok(id.to_string())
}

#[cfg(not(unix))]
pub fn boot_id() -> Result<String> {
    Err(Error::Unsupported("a boot id"))
}

#[cfg(test)]
#[path = "clock_tests.rs"]
mod tests;
