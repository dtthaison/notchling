use crate::vault_caller::{Processes, SystemProcesses};

#[test]
fn system_facts_for_this_process() {
    let me = std::process::id();
    let procs = SystemProcesses;
    assert!(procs.start_time(me).is_some());
    assert_eq!(procs.start_time(me), procs.start_time(me));
    let argv = procs.argv(me).unwrap();
    let binary = std::env::current_exe()
        .unwrap()
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert!(argv[0].ends_with(&binary), "{argv:?} vs {binary}");
    assert!(procs.exe(me).is_some());
    assert!(procs.start_time(u32::MAX - 1).is_none());
    assert!(procs.argv(u32::MAX - 1).is_none());
    assert!(procs.stdout_path(u32::MAX - 1).is_none());
    assert!(procs.exe(u32::MAX - 1).is_none());
}

#[test]
fn a_child_has_its_own_start_time_argv_and_stdout_file() {
    let dir = std::env::temp_dir().join(format!("notchlingd-facts-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let out = dir.join("out.txt");
    let file = std::fs::File::create(&out).unwrap();
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .stdout(file)
        .spawn()
        .unwrap();
    let pid = child.id();
    let procs = SystemProcesses;
    let seen = procs
        .stdout_path(pid)
        .and_then(|p| std::fs::canonicalize(p).ok());
    let argv = procs.argv(pid);
    let started = procs.start_time(pid);
    let mine = procs.start_time(std::process::id());
    let _ = child.kill();
    let _ = child.wait();
    let real = std::fs::canonicalize(&out).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(seen, Some(real));
    let argv = argv.unwrap();
    assert_eq!(argv.last().map(String::as_str), Some("30"));
    assert!(argv[0].ends_with("sleep"), "{argv:?}");
    assert!(started.is_some());
    assert!(mine.is_some());
}

#[test]
fn a_pipe_has_no_stdout_path() {
    let mut child = std::process::Command::new("sleep")
        .arg("30")
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let seen = SystemProcesses.stdout_path(child.id());
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(seen, None);
}

#[test]
fn procargs_split_into_argv_after_the_exec_path() {
    let mut raw = 3u32.to_ne_bytes().to_vec();
    raw.extend(b"/bin/sh\0\0\0sh\0-c\0echo hi\0HOME=/x\0");
    let argv = crate::process_facts::parse_procargs2(&raw).unwrap();
    assert_eq!(argv, ["sh", "-c", "echo hi"]);
    assert_eq!(crate::process_facts::parse_procargs2(&raw[..6]), None);
}
