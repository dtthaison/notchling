#[cfg(target_os = "macos")]
fn main() {
    use notchling_platform::watch::{WatchConfig, Watcher};
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    let mut args = std::env::args().skip(1);
    let seconds: u64 = args.next().and_then(|s| s.parse().ok()).unwrap_or(20);
    let mut folders: Vec<PathBuf> = args.map(PathBuf::from).collect();
    if folders.is_empty() {
        let home = PathBuf::from(std::env::var_os("HOME").expect("HOME"));
        let mut configs = vec![home.join(".claude")];
        if let Ok(lanes) = std::fs::read_dir(home.join(".claude-lanes")) {
            configs.extend(lanes.flatten().map(|e| e.path()));
        }
        for config in configs {
            for sub in ["jobs", "sessions", "projects"] {
                folders.push(config.join(sub));
            }
        }
        folders.push(home.join(".codex/sessions"));
    }

    let cpu = || {
        // SAFETY: an all-zero rusage is valid, and getrusage fills the one it is given.
        let usage = unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            libc::getrusage(libc::RUSAGE_SELF, &mut usage);
            usage
        };
        let secs = |t: libc::timeval| t.tv_sec as f64 + t.tv_usec as f64 / 1e6;
        secs(usage.ru_utime) + secs(usage.ru_stime)
    };

    let started = Instant::now();
    let (watcher, batches) =
        Watcher::start(folders.clone(), WatchConfig::default()).expect("start");
    println!(
        "watching {} folders, started in {:.2?}",
        folders.iter().filter(|f| f.is_dir()).count(),
        started.elapsed()
    );
    let (cpu0, wall0) = (cpu(), Instant::now());
    let mut count = 0;
    let end = wall0 + Duration::from_secs(seconds);
    while let Some(left) = end.checked_duration_since(Instant::now()) {
        if batches.recv_timeout(left).is_ok() {
            count += 1;
        }
    }
    let (used, wall) = (cpu() - cpu0, wall0.elapsed().as_secs_f64());
    drop(watcher);
    println!(
        "cpu {used:.3} s over {wall:.1} s = {:.1}% of a core; {count} batches",
        100.0 * used / wall
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("watch_cpu measures with getrusage on macOS only");
}
