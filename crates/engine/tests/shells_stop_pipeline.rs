#![cfg(unix)]

use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

#[cfg(target_os = "macos")]
use notchling_engine::sessions::Roots;
#[cfg(target_os = "macos")]
use notchling_engine::transcript::shells_guard::session_owner;
use notchling_engine::transcript::shells_guard::{current_uid, tmp_roots};
use notchling_engine::transcript::shells_stop::{
    process_table, stop_shell_in, writers_of, Outcome, StopRequest, GRACE,
};

const SESSION: &str = "5e5510aa-0000-4000-8000-0000000000be";
const ID: &str = "bshell0016";

#[cfg(target_os = "macos")]
const CHILD: &str = "NL_ZONE_CHILD";

#[cfg(target_os = "macos")]
fn in_zone(name: &str, body: impl FnOnce()) {
    if std::env::var_os(CHILD).is_some() {
        body();
        return;
    }
    let exe = std::env::current_exe().expect("test binary");
    let out = Command::new(exe)
        .args(["--exact", name, "--nocapture", "--test-threads=1"])
        .env("TZ", "ICT-7")
        .env(CHILD, "1")
        .output()
        .expect("child");
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{said}");
    assert!(said.contains("1 passed"), "{said}");
}

struct Rig {
    root: PathBuf,
    output: PathBuf,
    feed: PathBuf,
    shell: Option<Child>,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let root =
            std::env::temp_dir().join(format!("nl-shells-pipe-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let tasks = root
            .join(format!("claude-{}", current_uid()))
            .join("-work")
            .join(SESSION)
            .join("tasks");
        std::fs::create_dir_all(&tasks).expect("tasks folder");
        let feed = root.join("feed.log");
        std::fs::write(&feed, "").expect("feed");
        Rig {
            output: tasks.join(format!("{ID}.output")),
            root,
            feed,
            shell: None,
        }
    }

    fn start(&mut self, command: &str) -> i32 {
        let file = std::fs::File::create(&self.output).expect("output file");
        let again = file.try_clone().expect("clone");
        let script = command.replace("FEED", &self.feed.to_string_lossy());
        let kid = Command::new("sh")
            .args(["-c", &script])
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(again)
            .process_group(0)
            .spawn()
            .expect("spawn");
        let pgid = kid.id() as i32;
        self.shell = Some(kid);
        pgid
    }

    #[cfg(target_os = "macos")]
    fn start_in_a_session(&mut self, pipeline: &str) -> u32 {
        let script =
            "perl -e 'setpgrp(0,0); exec @ARGV' sh -c \"$PIPELINE\" > \"$OUT\" 2>&1 &\nsleep 300\n:";
        let kid = Command::new("sh")
            .arg0("claude bg-spare")
            .args(["-c", script])
            .env("PIPELINE", pipeline)
            .env("OUT", &self.output)
            .env("FEED", &self.feed)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0)
            .spawn()
            .expect("spawn");
        let pid = kid.id();
        self.shell = Some(kid);
        pid
    }

    fn output(&self) -> &str {
        self.output.to_str().expect("utf8")
    }

    fn wait_for_writers(&self, at_least: usize) -> Vec<i32> {
        let until = Instant::now() + Duration::from_secs(5);
        loop {
            let found = writers_of(self.output());
            if found.len() >= at_least || Instant::now() >= until {
                return found;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn stop(&self) -> Outcome {
        let request = StopRequest {
            session: SESSION,
            id: ID,
            path: self.output(),
            owner: Some(std::process::id()),
            orphans: false,
        };
        let mut roots = vec![self.root.clone()];
        roots.extend(tmp_roots());
        stop_shell_in(&request, &roots, current_uid(), GRACE).expect("allowed")
    }

    fn reap(&mut self) {
        if let Some(kid) = self.shell.as_mut() {
            let _ = kid.wait();
        }
    }
}

impl Rig {
    fn finish(&mut self) {
        let mine = unsafe { libc::getpgrp() };
        let table = process_table().unwrap_or_default();
        let mut groups: Vec<i32> = Vec::new();
        if let Some(kid) = self.shell.as_ref() {
            let root = kid.id() as i32;
            let mut family = vec![root];
            loop {
                let more: Vec<i32> = table
                    .iter()
                    .filter(|p| family.contains(&p.ppid) && !family.contains(&p.pid))
                    .map(|p| p.pid)
                    .collect();
                if more.is_empty() {
                    break;
                }
                family.extend(more);
            }
            groups.extend(
                table
                    .iter()
                    .filter(|p| family.contains(&p.pid))
                    .map(|p| p.pgid),
            );
        }
        let writers = writers_of(self.output());
        groups.extend(
            table
                .iter()
                .filter(|p| writers.contains(&p.pid))
                .map(|p| p.pgid),
        );
        groups.sort_unstable();
        groups.dedup();
        for pgid in groups {
            if pgid > 1 && pgid != mine {
                unsafe { libc::kill(-pgid, libc::SIGKILL) };
            }
        }
        if let Some(mut kid) = self.shell.take() {
            let _ = kid.kill();
            let _ = kid.wait();
        }
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.finish();
    }
}

fn members(pgid: i32) -> Vec<i32> {
    process_table()
        .unwrap_or_default()
        .iter()
        .filter(|p| p.pgid == pgid && !p.zombie)
        .map(|p| p.pid)
        .collect()
}

fn assert_ended(rig: &mut Rig, pgid: i32, expected_writers: usize) {
    let writers = rig.wait_for_writers(expected_writers);
    assert!(
        writers.len() >= expected_writers,
        "the pipeline never came up: {writers:?}"
    );
    let outcome = rig.stop();
    rig.reap();
    assert!(outcome.stopped(), "{outcome:?}");
    assert!(outcome.targets >= expected_writers, "{outcome:?}");
    assert!(members(pgid).is_empty(), "left: {:?}", members(pgid));
    assert!(writers_of(rig.output()).is_empty());
}

#[test]
fn a_subshell_with_a_backgrounded_tail_piped_into_grep_is_ended() {
    let mut rig = Rig::new("subshell");
    let pgid = rig.start("( tail -f FEED & wait ) | grep -m1 neverwritten");
    assert_ended(&mut rig, pgid, 4);
}

#[test]
fn a_plain_tail_piped_into_grep_is_ended() {
    let mut rig = Rig::new("plain");
    let pgid = rig.start("tail -f FEED | grep -m1 neverwritten");
    assert_ended(&mut rig, pgid, 3);
}

#[cfg(target_os = "macos")]
fn utc_start(pid: u32) -> String {
    let out = Command::new("ps")
        .env("LC_ALL", "C")
        .env("TZ", "UTC")
        .args(["-o", "lstart=", "-p", &pid.to_string()])
        .output()
        .expect("ps");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[cfg(target_os = "macos")]
#[test]
fn a_background_sessions_pipeline_is_ended_the_way_the_app_does() {
    in_zone(
        "a_background_sessions_pipeline_is_ended_the_way_the_app_does",
        session_pipeline,
    );
}

#[cfg(target_os = "macos")]
fn session_pipeline() {
    let mut rig = Rig::new("session");
    let owner = rig.start_in_a_session("( tail -f \"$FEED\" & wait ) | grep -m1 neverwritten");
    let writers = rig.wait_for_writers(4);
    assert!(
        writers.len() >= 4,
        "the pipeline never came up: {writers:?}"
    );
    let me = owner as i32;
    let until = Instant::now() + Duration::from_secs(5);
    let before = loop {
        let table = process_table().expect("process table");
        let ready = table
            .iter()
            .any(|p| p.ppid == me && !p.zombie && !writers.contains(&p.pid));
        if ready || Instant::now() >= until {
            break table;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let owner_started = before
        .iter()
        .find(|p| p.pid == me)
        .map(|p| p.started.clone())
        .expect("owner row");
    let pipeline = before
        .iter()
        .find(|p| p.ppid == me && writers.contains(&p.pid))
        .map(|p| p.pgid)
        .expect("shell leader");
    let sibling = before
        .iter()
        .find(|p| p.ppid == me && !writers.contains(&p.pid))
        .map(|p| p.pid)
        .expect("the owner's other child");
    let home = rig.root.join("home");
    std::fs::create_dir_all(home.join(".claude/sessions")).expect("sessions");
    let body = serde_json::json!({
        "pid": owner,
        "sessionId": SESSION,
        "procStart": utc_start(owner),
    });
    std::fs::write(
        home.join(format!(".claude/sessions/{owner}.json")),
        body.to_string(),
    )
    .expect("session file");
    let found = session_owner(&Roots::new(&home, &home.join("data")), SESSION, &before);
    let request = StopRequest {
        session: SESSION,
        id: ID,
        path: rig.output(),
        owner: found,
        orphans: true,
    };
    let mut roots = vec![rig.root.clone()];
    roots.extend(tmp_roots());
    let outcome = stop_shell_in(&request, &roots, current_uid(), GRACE).expect("allowed");
    let left: Vec<i32> = writers_of(rig.output());
    assert!(
        outcome.stopped() && left.is_empty(),
        "owner {found:?}, {outcome:?}, still writing {left:?}"
    );
    assert_eq!(found, Some(owner));
    assert!(outcome.targets >= 4, "{outcome:?}");
    assert!(
        members(pipeline).is_empty(),
        "left: {:?}",
        members(pipeline)
    );
    let after = process_table().expect("process table");
    assert!(
        after
            .iter()
            .any(|p| p.pid == me && !p.zombie && p.started == owner_started),
        "the owner was signalled"
    );
    assert!(
        after.iter().any(|p| p.pid == sibling && !p.zombie),
        "the owner's other child was signalled"
    );
    rig.finish();
    assert!(
        members(pipeline).is_empty(),
        "left: {:?}",
        members(pipeline)
    );
    assert!(process_table()
        .unwrap_or_default()
        .iter()
        .all(|p| (p.pid != me && p.pid != sibling) || p.zombie));
}
