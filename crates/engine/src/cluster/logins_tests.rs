use super::*;
use crate::cluster::id::Tool as ClusterTool;
use crate::cluster::testkit::versions;
use crate::cluster::versions::{compat, Compat, Purpose};
use std::path::PathBuf;
use std::sync::Mutex;

#[test]
fn codex_status_reads_signed_in() {
    assert!(codex_signed_in(0, "Logged in using ChatGPT\n"));
    assert!(codex_signed_in(0, "logged in using an API key"));
    assert!(!codex_signed_in(1, "Not logged in\n"));
    assert!(!codex_signed_in(0, "Not logged in"));
    assert!(!codex_signed_in(1, "Logged in using ChatGPT"));
    assert!(!codex_signed_in(0, ""));
    assert_eq!(codex_state(0, "Logged in using ChatGPT"), Some(true));
    assert_eq!(codex_state(1, "Not logged in"), Some(false));
    assert_eq!(codex_state(127, "sh: codex: not found"), None);
    assert_eq!(codex_state(124, ""), None);
    assert_eq!(codex_state(2, "error: unexpected argument"), None);
}

#[test]
fn agy_signed_in_by_file() {
    let home = PathBuf::from("/h");
    let seen = Mutex::new(Vec::new());
    let has = |p: &Path| {
        seen.lock().unwrap().push(p.to_path_buf());
        p == Path::new("/h/.gemini/jetski-standalone-oauth-token")
    };
    assert!(agy_signed_in(&has, &home));
    assert!(!agy_signed_in(&|_| false, &home));
    assert_eq!(
        seen.lock().unwrap().as_slice(),
        [PathBuf::from("/h/.gemini/jetski-standalone-oauth-token")]
    );
}

#[test]
fn a_missing_agy_file_on_a_mac_is_not_told_but_on_linux_it_is_signed_out() {
    let home = PathBuf::from("/h");
    assert_eq!(agy_state("macos", &|_| false, &home), None);
    assert_eq!(agy_state("linux", &|_| false, &home), Some(false));
    assert_eq!(agy_state("macos", &|_| true, &home), Some(true));
    assert_eq!(agy_state("linux", &|_| true, &home), Some(true));
}

fn with(node: &mut crate::cluster::Versions, tool: crate::versions::Tool, v: Option<bool>) {
    match v {
        Some(v) => node.logins.insert(tool, v),
        None => node.logins.remove(&tool),
    };
}

#[test]
fn dispatch_refuses_signed_out_tool() {
    let mac = versions("macos");
    let mut node = versions("macos");
    with(&mut node, crate::versions::Tool::Agy, Some(false));
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(ClusterTool::Agy)),
        Compat::Refuse("Antigravity isn't signed in on m1".to_string())
    );
    with(&mut node, crate::versions::Tool::Codex, Some(false));
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(ClusterTool::Codex)),
        Compat::Refuse("Codex isn't signed in on m1".to_string())
    );
}

#[test]
fn an_unknown_or_signed_in_login_keeps_todays_behaviour() {
    let mac = versions("macos");
    let mut node = versions("macos");
    for state in [None, Some(true)] {
        with(&mut node, crate::versions::Tool::Agy, state);
        assert_eq!(
            compat(&mac, &node, "m1", Purpose::Dispatch(ClusterTool::Agy)),
            Compat::Ok
        );
    }
    with(&mut node, crate::versions::Tool::Codex, Some(false));
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Dispatch(ClusterTool::Agy)),
        Compat::Ok
    );
    assert_eq!(
        compat(&mac, &node, "m1", Purpose::Link),
        Compat::Ok,
        "a signed-out tool never stops the link"
    );
}

struct Scripted {
    calls: Mutex<Vec<Vec<String>>>,
    answer: Option<(i32, String)>,
}

impl StatusRunner for Scripted {
    fn run(&self, argv: &[String], timeout: Duration) -> Option<(i32, String)> {
        assert_eq!(timeout, TIMEOUT);
        assert!(argv_read_only(argv), "{argv:?}");
        self.calls.lock().unwrap().push(argv.to_vec());
        self.answer.clone()
    }
}

fn scripted(answer: Option<(i32, &str)>) -> Scripted {
    Scripted {
        calls: Mutex::new(Vec::new()),
        answer: answer.map(|(c, t)| (c, t.to_string())),
    }
}

#[test]
fn probes_never_refresh() {
    for bad in [
        "codex login",
        "codex login --device-auth",
        "codex login --with-api-key",
        "codex login logout",
        "codex logout",
        "codex refresh",
        "agy refresh",
        "claude setup-token",
        "sh -c 'codex login'",
        "codex login status; codex login",
        "codex login\nstatus",
    ] {
        assert!(!read_only(bad), "{bad}");
    }
    for ok in [
        "codex login status",
        "codex login status 2>&1",
        "command -v codex >/dev/null 2>&1 && codex login status 2>&1",
        "/opt/homebrew/bin/codex login status",
        "[ -s \"$HOME/.gemini/jetski-standalone-oauth-token\" ]",
    ] {
        assert!(read_only(ok), "{ok}");
    }
    let run = scripted(Some((0, "Logged in using ChatGPT")));
    let home = PathBuf::from("/h");
    let src = Sources {
        codex: Some("/usr/local/bin/codex"),
        agy_installed: true,
        os: "linux",
        home: &home,
    };
    let got = probe(&src, &run, &|_| true);
    assert_eq!(
        got.get(&crate::versions::Tool::Codex),
        Some(&true),
        "{got:?}"
    );
    assert_eq!(
        run.calls.lock().unwrap().as_slice(),
        [vec![
            "/usr/local/bin/codex".to_string(),
            "login".to_string(),
            "status".to_string()
        ]]
    );
}

#[test]
fn a_hostile_codex_path_never_runs_a_write() {
    let run = scripted(Some((0, "Logged in")));
    let home = PathBuf::from("/h");
    let src = Sources {
        codex: Some("/x/codex login refresh"),
        agy_installed: false,
        os: "linux",
        home: &home,
    };
    assert!(probe(&src, &run, &|_| false).is_empty());
    assert!(run.calls.lock().unwrap().is_empty());
}

#[test]
fn only_installed_tools_are_probed_and_a_failed_run_is_unknown() {
    let home = PathBuf::from("/h");
    let none = Sources {
        codex: None,
        agy_installed: false,
        os: "linux",
        home: &home,
    };
    let run = scripted(None);
    assert!(probe(&none, &run, &|_| true).is_empty());
    assert!(run.calls.lock().unwrap().is_empty());
    let some = Sources {
        codex: Some("codex"),
        agy_installed: true,
        os: "linux",
        home: &home,
    };
    let got = probe(&some, &run, &|_| false);
    assert_eq!(got.get(&crate::versions::Tool::Codex), None);
    assert_eq!(got.get(&crate::versions::Tool::Agy), Some(&false));
}

#[test]
fn the_tracker_reports_a_flip_once() {
    let mut t = Tracker::default();
    let mut now = BTreeMap::new();
    assert!(t.update(&now).is_empty());
    now.insert(crate::versions::Tool::Codex, true);
    assert!(t.update(&now).is_empty());
    now.insert(crate::versions::Tool::Codex, false);
    let flips = t.update(&now);
    assert_eq!(
        flips,
        vec![Flip {
            tool: crate::versions::Tool::Codex,
            signed_in: false,
            seen_before: true
        }]
    );
    assert!(t.update(&now).is_empty());
    now.insert(crate::versions::Tool::Codex, true);
    assert_eq!(
        t.update(&now),
        vec![Flip {
            tool: crate::versions::Tool::Codex,
            signed_in: true,
            seen_before: true
        }]
    );
    let mut fresh = Tracker::default();
    now.insert(crate::versions::Tool::Agy, false);
    let first = fresh.update(&now);
    assert_eq!(first.len(), 1);
    assert!(!first[0].signed_in);
    assert!(!first[0].seen_before);
}

#[test]
fn the_texts_name_the_node_and_the_command() {
    let (title, detail) = needed_text(crate::versions::Tool::Codex, "w1");
    assert_eq!(title, "Codex isn't signed in on w1");
    assert!(detail.contains("codex login --device-auth"), "{detail}");
    let (title, _) = needed_text(crate::versions::Tool::Agy, "w1");
    assert_eq!(title, "Antigravity isn't signed in on w1");
    assert!(how_to(crate::versions::Tool::Codex, "w1").contains("codex login --device-auth"));
}
