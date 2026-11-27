#![cfg(unix)]

use std::path::PathBuf;
use std::process::Command;

use notchling_engine::sessions::Roots;
use notchling_engine::transcript::shells_guard::session_owner;
use notchling_engine::transcript::shells_stop::{process_table, stop_plan, Proc, Scope};

const SESSION: &str = "5e5510aa-0000-4000-8000-0000000000bf";
const CHILD: &str = "NL_ZONE_CHILD";

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

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Home {
        let dir = std::env::temp_dir().join(format!("nl-shells-zone-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join(".claude/sessions")).expect("sessions");
        Home(dir)
    }

    fn roots(&self) -> Roots {
        Roots::new(&self.0, &self.0.join("data"))
    }

    fn session(&self, pid: u32, started: &str) {
        let body = serde_json::json!({"pid": pid, "sessionId": SESSION, "procStart": started});
        let file = self.0.join(format!(".claude/sessions/{pid}.json"));
        std::fs::write(file, body.to_string()).expect("session file");
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn live(pid: i32, started: &str, command: &str) -> Proc {
    Proc {
        pid,
        ppid: 1,
        pgid: pid,
        started: started.to_string(),
        zombie: false,
        command: command.to_string(),
    }
}

fn words(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn a_process_started_one_zone_offset_off_is_not_the_owner() {
    in_zone(
        "a_process_started_one_zone_offset_off_is_not_the_owner",
        || {
            let home = Home::new("offset");
            home.session(4101, "Sun Nov 22 08:00:22 2026");
            let same = vec![live(4101, "Sun Nov 22 08:00:22 2026", "claude")];
            assert_eq!(session_owner(&home.roots(), SESSION, &same), Some(4101));
            for started in ["Sun Nov 22 15:00:22 2026", "Sun Nov 22 01:00:22 2026"] {
                let reused = vec![live(4101, started, "claude")];
                assert_eq!(
                    session_owner(&home.roots(), SESSION, &reused),
                    None,
                    "{started}"
                );
            }
        },
    );
}

#[test]
fn the_process_table_reads_start_times_in_utc_in_any_zone() {
    in_zone(
        "the_process_table_reads_start_times_in_utc_in_any_zone",
        || {
            let me = std::process::id() as i32;
            let table = process_table().expect("process table");
            let mine = table.iter().find(|p| p.pid == me).expect("own row");
            let out = Command::new("ps")
                .env("LC_ALL", "C")
                .env("TZ", "UTC")
                .args(["-o", "lstart=", "-p", &me.to_string()])
                .output()
                .expect("ps");
            assert_eq!(
                words(&mine.started),
                words(&String::from_utf8_lossy(&out.stdout))
            );
        },
    );
}

#[test]
fn a_claude_code_process_titled_with_its_role_is_the_owner() {
    let started = "Sun Nov 22 08:00:22 2026";
    for (tag, title) in [
        ("spare", "claude bg-spare"),
        ("host", "claude bg-pty-host"),
        ("path", "/Users/me/.local/bin/claude bg-spare"),
        ("spaces", "/Applications/Claude Code/claude bg-spare"),
        ("name", "/Users/First Last/.local/bin/claude bg-spare"),
        ("plain", "/Users/First Last/.local/bin/claude"),
    ] {
        let home = Home::new(tag);
        home.session(4102, started);
        let table = vec![live(4102, started, title)];
        assert_eq!(
            session_owner(&home.roots(), SESSION, &table),
            Some(4102),
            "{title}"
        );
    }
}

#[test]
fn a_process_that_only_mentions_claude_is_no_owner() {
    let started = "Sun Nov 22 08:00:22 2026";
    for (tag, title) in [
        ("tmux", "tmux claude"),
        ("claudex", "claudex bg-spare"),
        ("editor", "vim claude.md"),
        ("dirs", "/Users/First Last/claude-tools/run bg-spare"),
    ] {
        let home = Home::new(tag);
        home.session(4103, started);
        let table = vec![live(4103, started, title)];
        assert_eq!(
            session_owner(&home.roots(), SESSION, &table),
            None,
            "{title}"
        );
    }
}

#[test]
fn a_claude_titled_writer_whose_path_has_spaces_is_never_planned() {
    let started = "Sun Nov 22 08:00:22 2026";
    let scope = Scope {
        owner: 500,
        own_pid: 9000,
        own_pgid: 9000,
    };
    let table = |title: &str| {
        vec![
            live(1, started, "/sbin/launchd"),
            Proc {
                ppid: 1,
                ..live(500, started, "claude bg-spare")
            },
            Proc {
                ppid: 500,
                pgid: 600,
                ..live(600, started, title)
            },
        ]
    };
    for title in [
        "/Users/First Last/.local/bin/claude bg-spare",
        "/Applications/Claude Code/claude bg-spare",
    ] {
        let found = stop_plan(&[600], &table(title), &scope, false);
        assert!(found.is_empty(), "{title}: {found:?}");
    }
    let found = stop_plan(&[600], &table("/bin/zsh"), &scope, false);
    assert_eq!(found.every_pid(), vec![600]);
}

#[test]
fn a_versioned_native_install_is_the_owner_and_is_never_planned() {
    let started = "Sun Nov 22 08:00:22 2026";
    let scope = Scope {
        owner: 500,
        own_pid: 9000,
        own_pgid: 9000,
    };
    for title in [
        "/Users/me/.local/share/claude/versions/2.1.0",
        "/Users/First Last/.local/share/claude/versions/2.1.0",
    ] {
        let home = Home::new("versioned");
        home.session(4104, started);
        let table = vec![live(4104, started, title)];
        assert_eq!(
            session_owner(&home.roots(), SESSION, &table),
            Some(4104),
            "{title}"
        );
        let procs = vec![
            live(1, started, "/sbin/launchd"),
            Proc {
                ppid: 1,
                ..live(500, started, "claude bg-spare")
            },
            Proc {
                ppid: 500,
                pgid: 600,
                ..live(600, started, title)
            },
        ];
        let found = stop_plan(&[600], &procs, &scope, false);
        assert!(found.is_empty(), "{title}: {found:?}");
    }
}
