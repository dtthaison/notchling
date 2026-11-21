use super::*;

const OWNER: i32 = 500;
const OWN: i32 = 9000;

fn proc(pid: i32, ppid: i32, pgid: i32, command: &str) -> Proc {
    Proc {
        pid,
        ppid,
        pgid,
        started: "Sat Nov 21 17:39:00 2026".to_string(),
        zombie: false,
        command: command.to_string(),
    }
}

fn scope() -> Scope {
    Scope {
        owner: OWNER,
        own_pid: OWN,
        own_pgid: OWN,
    }
}

fn table() -> Vec<Proc> {
    vec![
        proc(1, 0, 1, "/sbin/launchd"),
        proc(
            OWNER,
            400,
            OWNER,
            "/Users/me/.local/share/claude/versions/2.1.0/claude",
        ),
        proc(612, OWNER, 612, "/bin/zsh"),
        proc(613, 612, 612, "/bin/sleep"),
        proc(700, 400, 700, "/bin/zsh"),
        proc(701, 700, 700, "/usr/bin/nvim"),
        proc(OWN, 1, OWN, "/usr/local/bin/notchlingd"),
    ]
}

const PS: &str = "    1     0     1 Ss   Sat Nov 21 17:39:00 2026 /sbin/launchd\n  612   500   612 S    Sat Nov  1 07:05:09 2026 /Applications/My App/zsh\n  613   612   612 Z+   Sat Nov 21 17:39:01 2026 /bin/sleep\n";

#[test]
fn ps_lines_become_processes() {
    let procs = parse_ps(PS);
    assert_eq!(procs.len(), 3);
    assert_eq!(procs[1].ppid, 500);
    assert_eq!(procs[1].started, "Sat Nov 1 07:05:09 2026");
    assert_eq!(procs[1].command, "/Applications/My App/zsh");
    assert_eq!(procs[1].name(), "zsh");
    assert!(procs[2].zombie);
    assert!(parse_ps("garbage\n\n  x y z\n").is_empty());
}

#[test]
fn a_writer_below_the_owner_and_its_group_are_the_targets() {
    let found = plan(&[612], &table(), &scope());
    assert_eq!(found.groups.len(), 1);
    assert_eq!(found.groups[0].pgid, 612);
    assert_eq!(found.every_pid(), vec![612, 613]);
    assert!(found.pids.is_empty());
}

#[test]
fn a_writer_that_is_not_below_the_owner_is_skipped() {
    let found = plan(&[701, 700], &table(), &scope());
    assert!(found.is_empty());
}

#[test]
fn with_no_usable_owner_nothing_is_planned() {
    for owner in [0, 1, -3] {
        let scope = Scope { owner, ..scope() };
        assert!(plan(&[612], &table(), &scope).is_empty(), "{owner}");
    }
}

#[test]
fn a_group_with_a_stranger_in_it_is_not_signalled_as_a_group() {
    let mut procs = table();
    procs.push(proc(614, 700, 612, "/usr/bin/vim"));
    let found = plan(&[612, 613], &procs, &scope());
    assert!(found.groups.is_empty());
    assert_eq!(found.every_pid(), vec![612, 613]);
}

#[test]
fn a_group_whose_leader_is_not_a_writer_is_signalled_pid_by_pid() {
    let found = plan(&[613], &table(), &scope());
    assert!(found.groups.is_empty());
    assert_eq!(found.every_pid(), vec![613]);
}

#[test]
fn the_owners_and_our_own_groups_and_the_system_are_never_signalled() {
    let mut procs = table();
    procs.push(proc(620, OWNER, OWNER, "/bin/zsh"));
    let found = plan(&[620], &procs, &scope());
    assert!(found.groups.is_empty());
    assert_eq!(found.every_pid(), vec![620]);

    let mut procs = table();
    procs.push(proc(630, OWNER, OWN, "/bin/sleep"));
    let found = plan(&[630], &procs, &scope());
    assert!(found.groups.is_empty());

    let mut procs = table();
    procs.push(proc(640, OWNER, 1, "/bin/sleep"));
    procs.push(proc(641, OWNER, 0, "/bin/sleep"));
    let found = plan(&[640, 641, 1, 0, -1, OWNER, OWN], &procs, &scope());
    assert!(found.groups.is_empty());
    assert_eq!(found.every_pid(), vec![640, 641]);
}

#[test]
fn claude_node_notchling_and_other_tools_are_never_targets() {
    let mut procs = table();
    for (pid, command) in [
        (10, "/usr/local/bin/node"),
        (11, "/Applications/Notchling.app/Contents/MacOS/Notchling"),
        (12, "/Users/me/.local/share/claude/versions/2.1.0/claude"),
        (13, "/opt/homebrew/Caskroom/claude-code/2.1.0/claude-bin"),
        (14, "/Users/me/target/debug/notchling"),
        (15, "/usr/local/bin/codex"),
    ] {
        procs.push(proc(pid, OWNER, pid, command));
    }
    assert!(plan(&[10, 11, 12, 13, 14, 15], &procs, &scope()).is_empty());
}

#[test]
fn a_zombie_is_not_a_target() {
    let mut procs = table();
    procs[2].zombie = true;
    assert!(plan(&[612], &procs, &scope()).is_empty());
}

#[test]
fn two_writers_of_one_group_are_one_group() {
    let found = plan(&[612, 613], &table(), &scope());
    assert_eq!(found.groups.len(), 1);
    assert!(found.pids.is_empty());
}

#[cfg(unix)]
mod live {
    use super::super::*;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Child, Command, Stdio};

    const SESSION: &str = "5e5510aa-0000-4000-8000-0000000000aa";
    const ID: &str = "bshell0001";

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    struct Kids(Vec<Child>);

    impl Drop for Kids {
        fn drop(&mut self) {
            for kid in &mut self.0 {
                let _ = kid.kill();
                let _ = kid.wait();
            }
        }
    }

    fn tasks(tag: &str) -> (Dir, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("nl-shells-stop-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let tasks = root
            .join(format!("claude-{}", shells_guard::current_uid()))
            .join("-work")
            .join(SESSION)
            .join("tasks");
        std::fs::create_dir_all(&tasks).expect("tasks folder");
        let output = tasks.join(format!("{ID}.output"));
        (Dir(root.clone()), root, output)
    }

    fn lead(output: &Path, script: &str, term_ignored: bool) -> Child {
        let file = std::fs::File::create(output).expect("output file");
        let script = if term_ignored {
            format!("trap '' TERM; {script}")
        } else {
            script.to_string()
        };
        Command::new("sh")
            .args(["-c", &script])
            .stdout(file)
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn")
    }

    fn pause() {
        std::thread::sleep(Duration::from_millis(400));
    }

    fn me() -> u32 {
        std::process::id()
    }

    fn request<'a>(path: &'a Path, owner: Option<u32>) -> StopRequest<'a> {
        StopRequest {
            session: SESSION,
            id: ID,
            path: path.to_str().expect("utf8"),
            owner,
            orphans: false,
        }
    }

    fn alive(pid: u32) -> bool {
        process_table().is_some_and(|t| t.iter().any(|p| p.pid == pid as i32 && !p.zombie))
    }

    #[test]
    fn a_shell_and_its_children_below_the_owner_are_ended() {
        let (_dir, root, output) = tasks("end");
        let mut kids = Kids(vec![lead(&output, "sleep 60 & wait", false)]);
        pause();
        let leader = kids.0[0].id();
        let found = writers_of(output.to_str().expect("utf8"));
        assert!(found.contains(&(leader as i32)));
        assert!(found.len() >= 2, "{found:?}");
        let uid = shells_guard::current_uid();
        let outcome =
            stop_shell_in(&request(&output, Some(me())), &[root], uid, GRACE).expect("allowed");
        assert_eq!(outcome.survivors, 0);
        assert!(outcome.targets >= 2);
        let _ = kids.0[0].wait();
        assert!(writers_of(output.to_str().expect("utf8")).is_empty());
    }

    #[test]
    fn a_symlinked_output_never_reaches_the_processes_of_its_target() {
        let (_dir, root, output) = tasks("link");
        let real = root.join("real.txt");
        let mut kids = Kids(vec![lead(&real, "sleep 60", false)]);
        pause();
        std::os::unix::fs::symlink(&real, &output).expect("symlink");
        let uid = shells_guard::current_uid();
        let refused = stop_shell_in(&request(&output, Some(me())), &[root], uid, GRACE);
        assert_eq!(refused, Err(Refusal::NotAShellOutput));
        assert!(alive(kids.0[0].id()));
        let _ = kids.0[0].kill();
    }

    #[test]
    fn a_path_outside_the_sessions_tasks_folder_is_refused() {
        let (_dir, root, output) = tasks("outside");
        let stray = root.join("stray.output");
        let mut kids = Kids(vec![lead(&stray, "sleep 60", false)]);
        pause();
        let uid = shells_guard::current_uid();
        let refused = stop_shell_in(&request(&stray, Some(me())), &[root], uid, GRACE);
        assert_eq!(refused, Err(Refusal::NotAShellOutput));
        assert!(alive(kids.0[0].id()));
        let _ = (kids.0[0].kill(), output);
    }

    #[test]
    fn with_no_owner_nothing_is_touched() {
        let (_dir, root, output) = tasks("noowner");
        let mut kids = Kids(vec![lead(&output, "sleep 60", false)]);
        pause();
        let uid = shells_guard::current_uid();
        let refused = stop_shell_in(
            &request(&output, None),
            std::slice::from_ref(&root),
            uid,
            GRACE,
        );
        assert_eq!(refused, Err(Refusal::OwnerUnknown));
        let refused = stop_shell_in(&request(&output, Some(1)), &[root], uid, GRACE);
        assert_eq!(refused, Err(Refusal::OwnerUnknown));
        assert!(alive(kids.0[0].id()));
        let _ = kids.0[0].kill();
    }

    #[test]
    fn a_writer_that_is_not_below_the_owner_is_left_running() {
        let (_dir, root, output) = tasks("stranger");
        let mut kids = Kids(vec![
            lead(&output, "sleep 60", false),
            Command::new("sleep")
                .arg("60")
                .process_group(0)
                .spawn()
                .expect("other"),
        ]);
        pause();
        let other = kids.0[1].id();
        let uid = shells_guard::current_uid();
        let outcome =
            stop_shell_in(&request(&output, Some(other)), &[root], uid, GRACE).expect("allowed");
        assert_eq!(outcome.targets, 0);
        assert!(alive(kids.0[0].id()));
        let _ = kids.0[0].kill();
    }

    #[test]
    fn a_pid_that_changed_since_the_plan_is_not_killed_after_the_grace() {
        let (_dir, _root, output) = tasks("reuse");
        let mut kids = Kids(vec![lead(&output, "exec sleep 60", true)]);
        pause();
        let pid = kids.0[0].id();
        let table = process_table().expect("table");
        let real = table
            .iter()
            .find(|p| p.pid == pid as i32)
            .expect("child in table");
        let stale = Plan {
            groups: Vec::new(),
            pids: vec![Target {
                pid: pid as i32,
                started: "Mon Jan 1 00:00:00 1990".to_string(),
            }],
        };
        let scope = Scope {
            owner: me() as i32,
            own_pid: me() as i32,
            own_pgid: 0,
        };
        let outcome = execute(&stale, &scope, Duration::from_millis(300));
        assert_eq!(outcome.killed, 0);
        assert!(alive(pid), "the process kept running past the grace");

        let current = Plan {
            groups: Vec::new(),
            pids: vec![Target {
                pid: pid as i32,
                started: real.started.clone(),
            }],
        };
        let outcome = execute(&current, &scope, Duration::from_millis(300));
        assert_eq!(outcome.killed, 1);
        assert_eq!(outcome.survivors, 0);
        let _ = kids.0[0].wait();
    }
}

#[cfg(unix)]
mod slash_tmp {
    use super::super::*;
    use std::os::unix::process::CommandExt;
    use std::process::{Child, Command, Stdio};

    const SESSION: &str = "5e5510aa-0000-4000-8000-0000000000ac";
    const ID: &str = "bshell0002";

    struct Base(PathBuf);

    impl Drop for Base {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn shell(tag: &str, uid: u32) -> (Base, PathBuf, Child) {
        let base = PathBuf::from("/tmp")
            .join(format!("claude-{uid}"))
            .join(format!("nl-shells-slash-tmp-{tag}-{}", std::process::id()));
        let tasks = base.join(SESSION).join("tasks");
        std::fs::create_dir_all(&tasks).expect("tasks folder");
        let output = tasks.join(format!("{ID}.output"));
        let file = std::fs::File::create(&output).expect("output file");
        let kid = Command::new("sh")
            .args(["-c", "sleep 60"])
            .stdout(file)
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn");
        (Base(base), output, kid)
    }

    #[test]
    fn shells_under_slash_tmp_are_found_by_their_real_names() {
        let uid = shells_guard::current_uid();
        let (_a, first, mut one) = shell("a", uid);
        let (_b, second, mut two) = shell("b", uid);
        std::thread::sleep(Duration::from_millis(400));
        let paths = vec![
            first.to_str().expect("utf8").to_string(),
            second.to_str().expect("utf8").to_string(),
        ];
        let found = writers_of_many(&paths);
        assert_eq!(found[&paths[0]], vec![one.id() as i32], "{found:?}");
        assert_eq!(found[&paths[1]], vec![two.id() as i32], "{found:?}");
        let request = StopRequest {
            session: SESSION,
            id: ID,
            path: &paths[0],
            owner: Some(std::process::id()),
            orphans: false,
        };
        let outcome = stop_shell_in(&request, &shells_guard::tmp_roots(), uid, GRACE);
        let second_alive = two.try_wait().expect("wait").is_none();
        let _ = one.kill();
        let _ = one.wait();
        let _ = two.kill();
        let _ = two.wait();
        let outcome = outcome.expect("allowed");
        assert!(
            outcome.targets >= 1 && outcome.survivors == 0,
            "{outcome:?}"
        );
        assert!(second_alive, "the other shell is left running");
    }
}

fn orphans() -> Vec<Proc> {
    vec![
        proc(1, 0, 1, "/sbin/launchd"),
        proc(800, 1, 800, "/bin/zsh"),
        proc(801, 800, 800, "/bin/sleep"),
        proc(900, 1, 900, "/bin/zsh"),
        proc(612, OWNER, 612, "/bin/zsh"),
        proc(OWN, 1, OWN, "/usr/local/bin/notchlingd"),
    ]
}

#[test]
fn an_orphaned_shell_tree_with_one_root_under_launchd_is_a_target_pid_by_pid() {
    let got = orphan_targets(&[800, 801], &orphans(), &scope());
    assert_eq!(got.iter().map(|t| t.pid).collect::<Vec<_>>(), [800, 801]);
}

#[test]
fn two_roots_or_a_root_with_a_live_parent_are_not_orphans() {
    assert!(orphan_targets(&[800, 900], &orphans(), &scope()).is_empty());
    assert!(orphan_targets(&[612], &orphans(), &scope()).is_empty());
    assert!(orphan_targets(&[404], &orphans(), &scope()).is_empty());
}

#[test]
fn a_protected_or_own_writer_refuses_the_whole_orphan_tree() {
    let mut procs = orphans();
    procs.push(proc(802, 800, 800, "/usr/local/bin/node"));
    assert!(orphan_targets(&[800, 801, 802], &procs, &scope()).is_empty());
    assert!(orphan_targets(&[OWN], &procs, &scope()).is_empty());
}

#[test]
fn ps_runs_in_the_c_locale() {
    assert!(ps()
        .get_envs()
        .any(|(key, value)| key == "LC_ALL" && value == Some(std::ffi::OsStr::new("C"))));
}

#[cfg(unix)]
mod orphaned {
    use super::super::*;
    use std::os::unix::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};

    const SESSION: &str = "5e5510aa-0000-4000-8000-0000000000ad";
    const ID: &str = "bshell0003";

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tasks(tag: &str) -> (Dir, PathBuf, PathBuf) {
        let root =
            std::env::temp_dir().join(format!("nl-shells-orphan-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let tasks = root
            .join(format!("claude-{}", shells_guard::current_uid()))
            .join("-work")
            .join(SESSION)
            .join("tasks");
        std::fs::create_dir_all(&tasks).expect("tasks folder");
        let output = tasks.join(format!("{ID}.output"));
        (Dir(root.clone()), root, output)
    }

    fn orphan(output: &Path) {
        let file = std::fs::File::create(output).expect("output file");
        Command::new("sh")
            .args(["-c", "sh -c 'sleep 60 & wait' & exit 0"])
            .stdout(file)
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn")
            .wait()
            .expect("wait");
        std::thread::sleep(Duration::from_millis(500));
    }

    fn reap(pids: &[i32]) {
        for pid in pids {
            unsafe {
                libc::kill(*pid, libc::SIGKILL);
            }
        }
    }

    fn request<'a>(path: &'a Path, owner: Option<u32>, orphans: bool) -> StopRequest<'a> {
        StopRequest {
            session: SESSION,
            id: ID,
            path: path.to_str().expect("utf8"),
            owner,
            orphans,
        }
    }

    #[test]
    fn the_ui_ends_an_orphaned_shell_whose_owner_is_gone() {
        let (_dir, root, output) = tasks("ui");
        orphan(&output);
        let text = output.to_str().expect("utf8");
        let before = writers_of(text);
        assert!(before.len() >= 2, "{before:?}");
        let uid = shells_guard::current_uid();
        let outcome = stop_shell_in(&request(&output, None, true), &[root], uid, GRACE);
        let after = writers_of(text);
        reap(&before);
        let outcome = outcome.expect("allowed");
        assert!(
            outcome.targets >= 2 && outcome.survivors == 0,
            "{outcome:?}"
        );
        assert!(after.is_empty(), "{after:?}");
    }

    #[test]
    fn an_agents_stop_never_ends_an_orphan() {
        let (_dir, root, output) = tasks("agent");
        orphan(&output);
        let text = output.to_str().expect("utf8");
        let before = writers_of(text);
        let uid = shells_guard::current_uid();
        let none = stop_shell_in(
            &request(&output, None, false),
            std::slice::from_ref(&root),
            uid,
            GRACE,
        );
        let some = stop_shell_in(
            &request(&output, Some(std::process::id()), false),
            std::slice::from_ref(&root),
            uid,
            GRACE,
        );
        let after = writers_of(text);
        reap(&before);
        assert_eq!(none, Err(Refusal::OwnerUnknown));
        assert_eq!(some.expect("allowed").targets, 0);
        assert_eq!(after.len(), before.len());
    }

    #[test]
    fn the_ui_leaves_a_writer_whose_parent_is_alive() {
        let (_dir, root, output) = tasks("parent");
        let file = std::fs::File::create(&output).expect("output file");
        let mut kid = Command::new("sh")
            .args(["-c", "sleep 60"])
            .stdout(file)
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn");
        std::thread::sleep(Duration::from_millis(400));
        let uid = shells_guard::current_uid();
        let outcome = stop_shell_in(&request(&output, None, true), &[root], uid, GRACE);
        let alive = kid.try_wait().expect("wait").is_none();
        let _ = kid.kill();
        let _ = kid.wait();
        assert_eq!(outcome.expect("allowed").targets, 0);
        assert!(alive);
    }
}
