#![cfg(unix)]

mod common;

use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

struct Dir(PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn scratch(tag: &str) -> Dir {
    let dir = PathBuf::from(format!("/tmp/nl-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("notchling")).unwrap();
    Dir(dir)
}

struct Daemon(Child);

impl Drop for Daemon {
    fn drop(&mut self) {
        if matches!(self.0.try_wait(), Ok(None)) {
            // SAFETY: signalling our own child by its pid.
            unsafe { libc::kill(self.0.id() as libc::pid_t, libc::SIGTERM) };
            let deadline = Instant::now() + Duration::from_secs(5);
            while matches!(self.0.try_wait(), Ok(None)) && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(50));
            }
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

fn notchling(root: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_notchling"));
    c.env("NOTCHLING_DATA_ROOT", root)
        .env("HOME", root)
        .env("PATH", "/usr/bin:/bin")
        .env_remove(notchling_local_api::APP_TIER_PATHS_ENV)
        .env_remove("NOTCHLING_CLUSTER_LOOPBACK")
        .env_remove("CODEX_HOME");
    c
}

fn start_daemon(root: &Path) -> Daemon {
    let mut cmd = notchling(root);
    cmd.arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let daemon = Daemon(cmd.spawn().unwrap());
    let socket = root.join("notchling/notchlingd.sock");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            return daemon;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no daemon on {}", socket.display());
}

fn run(root: &Path, args: &[&str], input: &str) -> (i32, String, String) {
    let mut child = notchling(root)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(input.as_bytes());
    }
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

#[test]
fn a_fixture_daemon_is_a_node_that_links_nowhere() {
    let dir = scratch("cluster-e2e");
    let root = dir.0.as_path();
    let _daemon = start_daemon(root);

    let (code, out, err) = run(root, &["cluster", "status", "--json"], "");
    assert_eq!(code, 0, "{err}");
    let status: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["cluster"], Value::Null, "{status}");
    let me = status["me"].as_str().unwrap();
    assert_eq!(me.len(), 16, "{status}");
    assert!(me.chars().all(|c| c.is_ascii_hexdigit()));
    let identity = root.join("notchling/cluster/node");
    assert!(identity.join("node.pem").is_file());
    assert!(identity.join("node.key").is_file());

    let (code, _, err) = run(root, &["cluster", "join", "nlj1.secret"], "");
    assert_eq!(code, 2);
    assert!(err.contains("pass the token on stdin"), "{err}");
    assert!(!err.contains("nlj1.secret"), "{err}");

    let (code, _, err) = run(root, &["cluster", "token"], "");
    assert_eq!(code, 2, "{err}");
    assert!(err.contains("Usage"), "{err}");

    let (code, _, err) = run(
        root,
        &["cluster", "join", "--token-stdin"],
        "nlj1.not-a-token\n",
    );
    assert_eq!(code, 1, "{err}");

    let (code, out, _) = run(root, &["cluster", "status", "--json"], "");
    assert_eq!(code, 0);
    let status: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(status["listening"], Value::Null, "{status}");
    assert_eq!(status["link"], false, "a bad token leaves the switch off");
    let audit = common::audit::raw_lines(root).join("\n");
    assert!(!audit.contains("nlj1."), "no token in the audit log");
}

fn node_daemon(root: &Path, home: &Path) -> Daemon {
    let mut cmd = notchling(root);
    cmd.env("HOME", home)
        .env(
            notchling_local_api::APP_TIER_PATHS_ENV,
            std::env::current_exe().unwrap(),
        )
        .env("NOTCHLING_CLUSTER_LOOPBACK", "1")
        .env("NOTCHLING_CLUSTER_PORT", "0")
        .arg("daemon")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let daemon = Daemon(cmd.spawn().unwrap());
    let socket = common::socket(root);
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if std::os::unix::net::UnixStream::connect(&socket).is_ok() {
            return daemon;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("no daemon on {}", socket.display());
}

fn app(root: &Path) -> notchling_local_api::Client {
    let mut client = common::connect(root, notchling_local_api::ClientKind::App);
    client.set_timeout(Duration::from_secs(30));
    client
}

fn eventually(what: &str, mut check: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if check() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("never happened: {what}");
}

#[test]
fn two_daemons_join_and_run_a_shell_job() {
    let dir = common::scratch("cluster-shell");
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch");
    let (root_a, root_b) = (dir.0.join("a"), dir.0.join("b"));
    common::copy_tree(&fixture, &root_a);
    common::copy_tree(&fixture, &root_b);
    let (home_a, home_b) = (dir.0.join("home-a"), dir.0.join("home-b"));
    let work = dir.0.join("work");
    for folder in [&home_a, &home_b, &work] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let _daemon_a = node_daemon(&root_a, &home_a);
    let _daemon_b = node_daemon(&root_b, &home_b);
    let (mut a, mut b) = (app(&root_a), app(&root_b));

    a.call("cluster.create", serde_json::json!({"name": "e2e"}))
        .unwrap();
    a.call(
        "settings.set_switch",
        serde_json::json!({"switch": "cluster", "on": true}),
    )
    .unwrap();
    let token = a.call("cluster.join.token", serde_json::json!({})).unwrap()["token"]
        .as_str()
        .unwrap()
        .to_string();
    let joined = b
        .call("cluster.join", serde_json::json!({"token": token}))
        .unwrap();
    assert_eq!(joined["state"], "pending", "{joined}");

    let mut item = String::new();
    eventually("the join shows in the primary's inbox", || {
        let list = a.call("inbox.list", serde_json::json!({})).unwrap();
        let found = list["items"].as_array().and_then(|items| {
            items
                .iter()
                .find(|i| i["kind"] == "node_join" && i["state"] == "open")
        });
        match found {
            Some(i) => {
                item = i["id"].as_str().unwrap().to_string();
                true
            }
            None => false,
        }
    });
    a.call(
        "inbox.answer",
        serde_json::json!({"id": item, "action": "allow"}),
    )
    .unwrap();

    let worker = b.call("cluster.status", serde_json::json!({})).unwrap()["me"]
        .as_str()
        .unwrap()
        .to_string();
    eventually("the worker is online", || {
        let status = a.call("cluster.status", serde_json::json!({})).unwrap();
        status["members"].as_array().is_some_and(|members| {
            members
                .iter()
                .any(|m| m["id"] == worker && m["online"] == true && m["state"] == "active")
        })
    });

    let job = serde_json::json!({
        "node": worker, "tool": "shell", "cwd": work.to_string_lossy(),
        "command": "printf ok", "key": "e2e-shell-1",
    });
    let off = a.call("cluster.dispatch", job.clone()).unwrap_err();
    assert!(
        off.to_string().contains("acting is off"),
        "an app dispatch needs this Mac's acting switch: {off}"
    );
    a.call("settings.set_acting", serde_json::json!({"on": true}))
        .unwrap();
    let mut reply = Value::Null;
    eventually("the worker takes the job", || {
        match a.call("cluster.dispatch", job.clone()) {
            Ok(r) => {
                reply = r;
                true
            }
            Err(_) => false,
        }
    });
    let id = reply["job"].as_str().expect("a job id").to_string();
    assert!(
        reply["session"].as_str().unwrap().starts_with("shell:"),
        "{reply}"
    );

    let log = root_b
        .join("notchling/cluster/jobs")
        .join(format!("{id}.log"));
    eventually("the job's log says ok", || {
        std::fs::read_to_string(&log).is_ok_and(|text| text == "ok")
    });
    eventually("the primary sees the job done", || {
        let status = a.call("cluster.status", serde_json::json!({})).unwrap();
        status["jobs"]
            .as_array()
            .is_some_and(|jobs| jobs.iter().any(|j| j["id"] == id && j["state"] == "done"))
    });
    let rows = b
        .call("sessions.list", serde_json::json!({"tools": "all"}))
        .unwrap();
    assert!(
        rows.as_array().unwrap().iter().any(|row| {
            row["id"].as_str().is_some_and(|i| i.starts_with("shell:")) && row["tool"] == "shell"
        }),
        "the worker lists its shell job: {rows}"
    );
}

struct Pair {
    _dir: common::Dir,
    _daemon_a: Daemon,
    _daemon_b: Daemon,
    root_a: PathBuf,
    a: notchling_local_api::Client,
    b: notchling_local_api::Client,
    worker: String,
}

fn pair(tag: &str) -> Pair {
    let dir = common::scratch(tag);
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../app/fixtures/notch");
    let (root_a, root_b) = (dir.0.join("a"), dir.0.join("b"));
    common::copy_tree(&fixture, &root_a);
    common::copy_tree(&fixture, &root_b);
    let (home_a, home_b) = (dir.0.join("home-a"), dir.0.join("home-b"));
    for folder in [&home_a, &home_b] {
        std::fs::create_dir_all(folder).unwrap();
    }
    let daemon_a = node_daemon(&root_a, &home_a);
    let daemon_b = node_daemon(&root_b, &home_b);
    let (mut a, mut b) = (app(&root_a), app(&root_b));
    a.call("cluster.create", serde_json::json!({"name": "e2e"}))
        .unwrap();
    a.call(
        "settings.set_switch",
        serde_json::json!({"switch": "cluster", "on": true}),
    )
    .unwrap();
    let worker = b.call("cluster.status", serde_json::json!({})).unwrap()["me"]
        .as_str()
        .unwrap()
        .to_string();
    Pair {
        _dir: dir,
        _daemon_a: daemon_a,
        _daemon_b: daemon_b,
        root_a,
        a,
        b,
        worker,
    }
}

fn items(a: &mut notchling_local_api::Client) -> Vec<Value> {
    a.call("inbox.list", serde_json::json!({})).unwrap()["items"]
        .as_array()
        .cloned()
        .unwrap_or_default()
}

fn online(a: &mut notchling_local_api::Client, worker: &str) -> bool {
    let status = a.call("cluster.status", serde_json::json!({})).unwrap();
    status["members"].as_array().is_some_and(|members| {
        members
            .iter()
            .any(|m| m["id"] == worker && m["online"] == true && m["state"] == "active")
    })
}

#[test]
fn invite_joins_without_inbox_item() {
    let mut p = pair("cluster-invite");
    let made =
        p.a.call(
            "cluster.invite",
            serde_json::json!({"node": p.worker, "name": "box9", "acting": false}),
        )
        .unwrap();
    let token = made["token"].as_str().unwrap().to_string();
    assert!(token.starts_with("nlj1."), "{made}");
    assert!(made["expiresMs"].as_i64().unwrap() > 0, "{made}");
    let status = p.a.call("cluster.status", serde_json::json!({})).unwrap();
    assert_eq!(status["invites"][0]["node"], p.worker, "{status}");
    assert!(!status.to_string().contains(&token));

    p.b.call("cluster.join", serde_json::json!({"token": token}))
        .unwrap();
    let worker = p.worker.clone();
    eventually("the invited node is active", || online(&mut p.a, &worker));

    let all = items(&mut p.a);
    assert!(
        all.iter().all(|i| i["kind"] != "node_join"),
        "an invite raises no Add item: {all:?}"
    );
    let notice = all
        .iter()
        .find(|i| i["title"] == "box9 joined the cluster")
        .unwrap_or_else(|| panic!("no notice in {all:?}"));
    assert_eq!(notice["kind"], "notice");
    assert_eq!(notice["actions"].as_array().map(Vec::len), Some(0));
    let status = p.a.call("cluster.status", serde_json::json!({})).unwrap();
    assert_eq!(
        status["invites"].as_array().map(Vec::len),
        Some(0),
        "{status}"
    );
    let audit = common::audit::raw_lines(&p.root_a).join("\n");
    assert!(!audit.contains("nlj1."), "no token in the audit log");
    assert!(!audit.contains(&token));
}

#[test]
fn token_without_invite_still_asks() {
    let mut p = pair("cluster-plain-token");
    let token =
        p.a.call("cluster.join.token", serde_json::json!({}))
            .unwrap()["token"]
            .as_str()
            .unwrap()
            .to_string();
    let joined =
        p.b.call("cluster.join", serde_json::json!({"token": token}))
            .unwrap();
    assert_eq!(joined["state"], "pending", "{joined}");
    eventually("the Add item", || {
        items(&mut p.a)
            .iter()
            .any(|i| i["kind"] == "node_join" && i["state"] == "open")
    });
    assert!(!online(&mut p.a, &p.worker));
    assert!(items(&mut p.a)
        .iter()
        .all(|i| i["title"] != "box9 joined the cluster"));
}
