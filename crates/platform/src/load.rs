use crate::{Error, Result};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Load {
    pub cpu_pct: u8,
    pub mem_used_mb: u64,
    pub mem_total_mb: u64,
}

pub struct LoadSampler {
    prev: Option<(u64, u64)>,
    #[cfg(target_os = "macos")]
    host: libc::mach_port_t,
}

impl Default for LoadSampler {
    fn default() -> Self {
        LoadSampler::new()
    }
}

impl LoadSampler {
    pub fn new() -> LoadSampler {
        LoadSampler {
            prev: None,
            #[cfg(target_os = "macos")]
            host: unsafe { mach_host_self() },
        }
    }

    pub fn cpu_step(&mut self, sample: (u64, u64)) -> u8 {
        let prev = self.prev.replace(sample);
        let Some((busy0, total0)) = prev else {
            return 0;
        };
        let (busy1, total1) = sample;
        if total1 <= total0 || busy1 < busy0 {
            return 0;
        }
        let busy = busy1 - busy0;
        let total = total1 - total0;
        ((busy.min(total) * 100) / total) as u8
    }

    pub fn sample(&mut self) -> Result<Load> {
        let ticks = self.cpu_ticks()?;
        let (mem_used_mb, mem_total_mb) = self.memory()?;
        Ok(Load {
            cpu_pct: self.cpu_step(ticks),
            mem_used_mb,
            mem_total_mb,
        })
    }

    #[cfg(target_os = "linux")]
    fn cpu_ticks(&self) -> Result<(u64, u64)> {
        let text = std::fs::read_to_string("/proc/stat")?;
        parse_proc_stat(&text).ok_or_else(|| Error::System("/proc/stat has no cpu line".into()))
    }

    #[cfg(target_os = "linux")]
    fn memory(&self) -> Result<(u64, u64)> {
        let text = std::fs::read_to_string("/proc/meminfo")?;
        parse_meminfo(&text).ok_or_else(|| Error::System("/proc/meminfo is incomplete".into()))
    }

    #[cfg(target_os = "macos")]
    fn cpu_ticks(&self) -> Result<(u64, u64)> {
        let mut info = libc::host_cpu_load_info {
            cpu_ticks: [0; libc::CPU_STATE_MAX as usize],
        };
        let mut count = libc::HOST_CPU_LOAD_INFO_COUNT;
        let rc = unsafe {
            libc::host_statistics(
                self.host,
                libc::HOST_CPU_LOAD_INFO,
                (&mut info as *mut libc::host_cpu_load_info).cast(),
                &mut count,
            )
        };
        if rc != libc::KERN_SUCCESS {
            return Err(Error::System(format!("host_statistics failed ({rc})")));
        }
        let t = info.cpu_ticks.map(u64::from);
        let busy = t[libc::CPU_STATE_USER as usize]
            + t[libc::CPU_STATE_SYSTEM as usize]
            + t[libc::CPU_STATE_NICE as usize];
        Ok((busy, busy + t[libc::CPU_STATE_IDLE as usize]))
    }

    #[cfg(target_os = "macos")]
    fn memory(&self) -> Result<(u64, u64)> {
        let mut total: u64 = 0;
        let mut size = std::mem::size_of::<u64>();
        let name = c"hw.memsize";
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                (&mut total as *mut u64).cast(),
                &mut size,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return Err(Error::Io(std::io::Error::last_os_error()));
        }
        let mut vm: libc::vm_statistics64 = unsafe { std::mem::zeroed() };
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let rc = unsafe {
            libc::host_statistics64(
                self.host,
                libc::HOST_VM_INFO64,
                (&mut vm as *mut libc::vm_statistics64).cast(),
                &mut count,
            )
        };
        if rc != libc::KERN_SUCCESS {
            return Err(Error::System(format!("host_statistics64 failed ({rc})")));
        }
        let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(0) as u64;
        let pages = u64::from(vm.active_count)
            + u64::from(vm.wire_count)
            + u64::from(vm.compressor_page_count);
        let used = (pages * page).min(total);
        Ok((used / MB, total / MB))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn cpu_ticks(&self) -> Result<(u64, u64)> {
        Err(Error::Unsupported("reading the load"))
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    fn memory(&self) -> Result<(u64, u64)> {
        Err(Error::Unsupported("reading the load"))
    }
}

#[cfg(target_os = "macos")]
const MB: u64 = 1024 * 1024;

#[cfg(target_os = "macos")]
extern "C" {
    fn mach_host_self() -> libc::mach_port_t;
}

pub fn parse_proc_stat(text: &str) -> Option<(u64, u64)> {
    let line = text.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .map(|f| f.parse::<u64>().ok())
        .collect::<Option<Vec<u64>>>()?;
    if fields.len() < 4 {
        return None;
    }
    let total: u64 = fields.iter().take(8).sum();
    let idle = fields[3] + fields.get(4).copied().unwrap_or(0);
    Some((total.saturating_sub(idle), total))
}

pub fn parse_meminfo(text: &str) -> Option<(u64, u64)> {
    let field = |name: &str| {
        text.lines().find_map(|l| {
            let rest = l.strip_prefix(name)?.strip_prefix(':')?;
            rest.split_whitespace().next()?.parse::<u64>().ok()
        })
    };
    let total = field("MemTotal")?;
    let available = field("MemAvailable")?;
    let used = total.saturating_sub(available);
    Some((used / 1024, total / 1024))
}

#[cfg(test)]
#[path = "load_tests.rs"]
mod tests;
