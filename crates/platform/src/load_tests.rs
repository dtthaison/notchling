use super::*;

const STAT_A: &str = "cpu  100 0 100 600 200 0 0 0 0 0\ncpu0 1 2 3 4\nintr 5\n";
const STAT_B: &str = "cpu  150 0 150 800 300 0 0 0 0 0\ncpu0 1 2 3 4\nintr 5\n";

#[test]
fn parses_proc_stat() {
    assert_eq!(parse_proc_stat(STAT_A), Some((200, 1000)));
    assert_eq!(parse_proc_stat("intr 5\n"), None);
    assert_eq!(parse_proc_stat("cpu  x y\n"), None);
}

#[test]
fn parses_proc_stat_cpu_delta() {
    let mut sampler = LoadSampler::new();
    let a = parse_proc_stat(STAT_A).unwrap();
    let b = parse_proc_stat(STAT_B).unwrap();
    assert_eq!(sampler.cpu_step(a), 0, "the first sample reports 0");
    assert_eq!(sampler.cpu_step(b), 25);
}

#[test]
fn cpu_step_survives_a_counter_reset() {
    let mut sampler = LoadSampler::new();
    sampler.cpu_step((500, 1000));
    assert_eq!(sampler.cpu_step((10, 20)), 0);
    assert_eq!(sampler.cpu_step((20, 40)), 50);
}

#[test]
fn parses_meminfo() {
    let text = "MemTotal:        8000000 kB\nMemFree:  100 kB\nMemAvailable:    2000000 kB\n";
    assert_eq!(parse_meminfo(text), Some((5859, 7812)));
    assert_eq!(parse_meminfo("MemTotal: 10 kB\n"), None);
}

#[cfg(any(target_os = "macos", target_os = "linux"))]
#[test]
fn a_live_sample_is_sane() {
    let mut sampler = LoadSampler::new();
    let load = sampler.sample().unwrap();
    assert_eq!(load.cpu_pct, 0);
    assert!(load.mem_total_mb > 0);
    assert!(load.mem_used_mb <= load.mem_total_mb);
    let again = sampler.sample().unwrap();
    assert!(again.cpu_pct <= 100);
}
