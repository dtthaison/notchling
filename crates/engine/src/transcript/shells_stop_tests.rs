use super::*;

fn proc(pid: i32, pgid: i32, command: &str) -> Proc {
    Proc {
        pid,
        pgid,
        command: command.to_string(),
    }
}

const PS: &str = "    1     1 /sbin/launchd\n  500   500 /Users/me/.local/share/claude/versions/2.1.0/claude\n  612   612 /bin/zsh\n  613   612 /bin/sleep\n  700   700 /bin/zsh\n";

#[test]
fn ps_lines_become_processes() {
    let procs = parse_ps(PS);
    assert_eq!(procs.len(), 5);
    assert_eq!(procs[2], proc(612, 612, "/bin/zsh"));
    assert_eq!(procs[1].name(), "claude");
    assert!(parse_ps("garbage\n\n  x y z\n").is_empty());
}

#[test]
fn a_writer_and_its_process_group_are_the_targets() {
    let procs = parse_ps(PS);
    let found = plan(&[612], &procs, 9000, 9000);
    assert_eq!(
        found.groups,
        vec![Group {
            pgid: 612,
            members: vec![612, 613]
        }]
    );
    assert!(found.pids.is_empty());
    assert_eq!(found.every_pid(), vec![612, 613]);
}

#[test]
fn claude_node_and_notchling_are_never_targets() {
    let procs = vec![
        proc(10, 10, "/usr/local/bin/node"),
        proc(
            11,
            11,
            "/Applications/Notchling.app/Contents/MacOS/Notchling",
        ),
        proc(
            12,
            12,
            "/Users/me/.local/share/claude/versions/2.1.0/claude",
        ),
        proc(13, 13, "/Users/me/bin/claude"),
    ];
    assert!(plan(&[10, 11, 12, 13], &procs, 9000, 9000).is_empty());
}

#[test]
fn a_group_that_holds_claude_is_not_signalled_as_a_group() {
    let procs = vec![
        proc(20, 20, "/Users/me/bin/claude"),
        proc(21, 20, "/bin/zsh"),
    ];
    let found = plan(&[21], &procs, 9000, 9000);
    assert!(found.groups.is_empty());
    assert_eq!(found.pids, vec![21]);
}

#[test]
fn our_own_process_and_group_and_the_system_are_left_alone() {
    let procs = vec![
        proc(1, 1, "/sbin/launchd"),
        proc(30, 30, "/bin/zsh"),
        proc(31, 30, "/bin/sleep"),
        proc(32, 32, "/bin/zsh"),
    ];
    assert!(plan(&[1], &procs, 9000, 9000).is_empty());
    assert!(plan(&[30], &procs, 30, 30).is_empty());
    let found = plan(&[31], &procs, 9000, 30);
    assert!(found.groups.is_empty());
    assert_eq!(found.pids, vec![31]);
    assert!(plan(&[999], &procs, 9000, 9000).is_empty());
    assert!(plan(&[0, -1], &procs, 9000, 9000).is_empty());
}

#[test]
fn two_writers_of_one_group_are_one_group() {
    let procs = vec![proc(40, 40, "/bin/zsh"), proc(41, 40, "/bin/sleep")];
    let found = plan(&[40, 41], &procs, 9000, 9000);
    assert_eq!(found.groups.len(), 1);
    assert!(found.pids.is_empty());
}

#[cfg(unix)]
#[test]
fn a_fake_writer_is_ended_by_term() {
    let dir = std::env::temp_dir().join(format!("shells-stop-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("dir");
    let out = dir.join("fake.output");
    let file = std::fs::File::create(&out).expect("file");
    let mut child = std::process::Command::new("sleep")
        .arg("60")
        .stdout(file)
        .spawn()
        .expect("spawn");
    std::thread::sleep(Duration::from_millis(300));
    let path = out.to_string_lossy().into_owned();
    assert_eq!(writers_of(&path), vec![child.id() as i32]);
    let outcome = stop_output(&path, GRACE);
    assert_eq!(outcome.targets, 1);
    assert_eq!(outcome.survivors, 0);
    let _ = child.wait();
    assert!(writers_of(&path).is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}
