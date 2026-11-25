use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct State {
    pub on_battery: bool,
    pub low_power: bool,
    pub thermal_serious: bool,
}

pub fn state() -> State {
    imp::state()
}

pub fn background_thread(on: bool) -> Result<()> {
    imp::background_thread(on)
}

pub fn parse_power_supply(entries: &[(String, String)]) -> bool {
    let mut names: Vec<&str> = entries
        .iter()
        .filter_map(|(file, _)| file.split_once('/').map(|(name, _)| name))
        .collect();
    names.sort_unstable();
    names.dedup();
    let value = |name: &str, key: &str| {
        entries
            .iter()
            .find(|(file, _)| file.split_once('/') == Some((name, key)))
            .map(|(_, v)| v.trim())
    };
    let mut has_battery = false;
    let mut mains = false;
    for name in names {
        match value(name, "type") {
            Some("Battery") => has_battery = true,
            Some(_) if value(name, "online") == Some("1") => mains = true,
            _ => {}
        }
    }
    has_battery && !mains
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::ffi::{c_char, c_void};

    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> *const c_void;
        fn IOPSGetProvidingPowerSourceType(info: *const c_void) -> *const c_void;
    }

    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFRelease(value: *const c_void);
        fn CFStringGetCString(
            text: *const c_void,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> u8;
    }

    const UTF8: u32 = 0x0800_0100;

    fn on_battery() -> bool {
        let info = unsafe { IOPSCopyPowerSourcesInfo() };
        if info.is_null() {
            return false;
        }
        let kind = unsafe { IOPSGetProvidingPowerSourceType(info) };
        let mut buffer = [0 as c_char; 64];
        let read = !kind.is_null()
            && unsafe {
                CFStringGetCString(kind, buffer.as_mut_ptr(), buffer.len() as isize, UTF8)
            } != 0;
        let text = if read {
            unsafe { std::ffi::CStr::from_ptr(buffer.as_ptr()) }
                .to_string_lossy()
                .into_owned()
        } else {
            String::new()
        };
        unsafe { CFRelease(info) };
        text == "Battery Power"
    }

    pub(super) fn state() -> State {
        use objc2_foundation::{NSProcessInfo, NSProcessInfoThermalState};
        let info = NSProcessInfo::processInfo();
        let thermal = info.thermalState();
        State {
            on_battery: on_battery(),
            low_power: info.isLowPowerModeEnabled(),
            thermal_serious: thermal == NSProcessInfoThermalState::Serious
                || thermal == NSProcessInfoThermalState::Critical,
        }
    }

    pub(super) fn background_thread(on: bool) -> Result<()> {
        let class = if on {
            libc::qos_class_t::QOS_CLASS_BACKGROUND
        } else {
            libc::qos_class_t::QOS_CLASS_DEFAULT
        };
        match unsafe { libc::pthread_set_qos_class_self_np(class, 0) } {
            0 => Ok(()),
            code => Err(Error::System(format!("thread priority failed ({code})"))),
        }
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use super::*;

    pub(super) fn state() -> State {
        let mut entries = Vec::new();
        if let Ok(dir) = std::fs::read_dir("/sys/class/power_supply") {
            for entry in dir.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                for key in ["type", "online"] {
                    if let Ok(text) = std::fs::read_to_string(entry.path().join(key)) {
                        entries.push((format!("{name}/{key}"), text));
                    }
                }
            }
        }
        State {
            on_battery: parse_power_supply(&entries),
            ..State::default()
        }
    }

    pub(super) fn background_thread(on: bool) -> Result<()> {
        let tid = unsafe { libc::syscall(libc::SYS_gettid) } as libc::id_t;
        let nice = if on { 19 } else { 0 };
        match unsafe { libc::setpriority(libc::PRIO_PROCESS, tid, nice) } {
            0 => Ok(()),
            _ => Err(Error::Io(std::io::Error::last_os_error())),
        }
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux")))]
mod imp {
    use super::*;

    pub(super) fn state() -> State {
        State::default()
    }

    pub(super) fn background_thread(_on: bool) -> Result<()> {
        Err(Error::Unsupported("a background thread priority"))
    }
}
