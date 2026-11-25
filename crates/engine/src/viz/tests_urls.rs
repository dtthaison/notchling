use super::suggest::{git_branch, suggest, VizSignals, VizTab};
use super::url::Url;
use super::web_url;
use crate::sessions::testutil::TempDir;
use std::path::Path;

fn home_with(launch: Option<&str>) -> TempDir {
    let home = TempDir::new("viz-home");
    if let Some(launch) = launch {
        home.write(".claude/launch.json", launch);
    }
    home
}

fn nothing_listens(_: u16) -> bool {
    false
}

#[test]
fn url_precedence_is_transcript_then_project_launch_then_home_launch() {
    let project = TempDir::new("viz-proj");
    let path = project.root().to_string_lossy().into_owned();
    let home = home_with(Some(&format!(
        r#"{{"configurations":[{{"name":"other-dev","runtimeArgs":["--cwd","/somewhere/else"],"port":4000}},
                           {{"name":"mine-prod","runtimeArgs":["--cwd","{path}"],"port":5002}},
                           {{"name":"mine-dev","runtimeArgs":["--cwd","{path}"],"port":5001}}]}}"#
    )));
    let resolve = |urls: &[&str], cwd: &str, home: &Path| {
        let urls: Vec<String> = urls.iter().map(|s| s.to_string()).collect();
        web_url::resolve(&urls, cwd, home, &nothing_listens)
    };
    assert_eq!(
        resolve(&[], &path, home.root()).as_deref(),
        Some("http://localhost:5001")
    );
    project.write(
        ".claude/launch.json",
        r#"{"configurations":[{"name":"web","port":6000}]}"#,
    );
    assert_eq!(
        resolve(&[], &path, home.root()).as_deref(),
        Some("http://localhost:6000")
    );
    assert_eq!(
        resolve(
            &["http://localhost:3000", "http://127.0.0.1:3001/x"],
            &path,
            home.root()
        )
        .as_deref(),
        Some("http://127.0.0.1:3001/x")
    );
    let bare = home_with(None);
    assert_eq!(
        resolve(&["https://example.com"], "/nowhere", bare.root()),
        None
    );
    assert!(Url::parse_local("file:///etc/passwd").is_none());
}

#[test]
fn api_paths_become_the_origin_queries_are_dropped_and_origins_are_deduped() {
    let urls: Vec<String> = [
        "http://localhost:3000/",
        "http://localhost:3040/api/auth/discord/?token=abc",
        "http://localhost:3040/api/users",
        "http://localhost:5173/dashboard?x=1#top",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    assert_eq!(
        web_url::page_candidates(&urls),
        [
            "http://localhost:5173/dashboard",
            "http://localhost:3040",
            "http://localhost:3000/"
        ]
    );
}

#[test]
fn a_listening_server_beats_a_newer_dead_one() {
    let bare = home_with(None);
    let urls: Vec<String> = ["http://localhost:3000", "http://localhost:3040/api/x"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let on_3000 = |port: u16| port == 3000;
    assert_eq!(
        web_url::resolve(&urls, "", bare.root(), &on_3000).as_deref(),
        Some("http://localhost:3000")
    );
    assert_eq!(
        web_url::resolve(&urls, "", bare.root(), &nothing_listens).as_deref(),
        Some("http://localhost:3040"),
        "nothing listens: the newest still comes back so the phone can name it"
    );
    assert_eq!(
        web_url::live_candidates(&urls, "", bare.root(), &on_3000, 5),
        ["http://localhost:3000"]
    );
}

#[test]
fn the_probe_sees_a_real_listener() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().expect("addr").port();
    assert!(web_url::tcp_is_listening(port));
    drop(listener);
    assert!(!web_url::tcp_is_listening(port));
    assert!(!web_url::tcp_is_listening(0));
}

#[test]
fn urls_parse_the_way_the_web_tab_needs() {
    let url = Url::parse("http://user:pw@localhost:3000/a/b?x=1#top").expect("url");
    assert_eq!(
        (
            url.host.as_str(),
            url.port,
            url.path.as_str(),
            url.query.as_deref(),
            url.fragment.as_deref()
        ),
        ("localhost", Some(3000), "/a/b", Some("x=1"), Some("top"))
    );
    assert_eq!(
        url.render(),
        "http://localhost:3000/a/b?x=1#top",
        "user info is dropped"
    );
    assert_eq!(url.origin(), "http://localhost:3000");
    assert_eq!(
        Url::parse("https://localhost")
            .expect("url")
            .effective_port(),
        443
    );
    assert!(Url::parse("http://localhost:99999").is_none());
    assert!(Url::parse("http://localhost:3000/a b").is_none());
    assert!(Url::parse_local("http://localhost.evil.com:3000").is_none());
    assert!(Url::parse_local("http://evil.com@localhost:3000").is_some());
    assert!(Url::parse_local("http://localhost@evil.com:3000").is_none());
    assert!(Url::parse_local("ftp://localhost").is_none());
}

pub(super) fn folder(files: &[(&str, &str)], directories: &[&str]) -> TempDir {
    let root = TempDir::new("viz-folder");
    for directory in directories {
        std::fs::create_dir_all(root.path(directory)).expect("mkdir");
    }
    for (path, content) in files {
        root.write(path, content);
    }
    root
}

fn suggested(dir: &TempDir) -> VizTab {
    suggest(&dir.root().to_string_lossy(), &VizSignals::default())
}

#[test]
fn suggester_rules_with_signals() {
    let empty = folder(&[], &[]);
    let cwd = empty.root().to_string_lossy().into_owned();
    let none = VizSignals::default();
    let url = VizSignals {
        has_localhost_url: true,
        ..VizSignals::default()
    };
    let mermaid = VizSignals {
        has_recent_mermaid: true,
        ..VizSignals::default()
    };
    let both = VizSignals {
        has_recent_mermaid: true,
        has_localhost_url: true,
    };
    assert_eq!(suggest(&cwd, &none), VizTab::Flow);
    assert_eq!(suggest(&cwd, &url), VizTab::Web);
    assert_eq!(suggest(&cwd, &mermaid), VizTab::Diagram);
    assert_eq!(
        suggest(&cwd, &both),
        VizTab::Diagram,
        "a fresh diagram beats everything"
    );
    empty.write("pubspec.yaml", "name: a");
    assert_eq!(
        suggest(&cwd, &url),
        VizTab::Phone,
        "cwd rules come before the URL"
    );
}

#[test]
fn phone_for_flutter_xcode_and_android_folders() {
    assert_eq!(
        suggested(&folder(&[("pubspec.yaml", "name: a")], &[])),
        VizTab::Phone
    );
    assert_eq!(suggested(&folder(&[], &["App.xcodeproj"])), VizTab::Phone);
    assert_eq!(suggested(&folder(&[], &["android"])), VizTab::Phone);
}

#[test]
fn web_for_next_vite_or_react() {
    for package in [
        r#"{"dependencies":{"next":"15"}}"#,
        r#"{"devDependencies":{"vite":"6"}}"#,
        r#"{"dependencies":{"react":"19"}}"#,
    ] {
        assert_eq!(
            suggested(&folder(&[("package.json", package)], &[])),
            VizTab::Web
        );
    }
}

#[test]
fn entity_for_prisma_drift_or_sql() {
    assert_eq!(
        suggested(&folder(&[("prisma/schema.prisma", "model A {}")], &[])),
        VizTab::Entity
    );
    assert_eq!(
        suggested(&folder(&[("schema.sql", "create table a();")], &[])),
        VizTab::Entity
    );
    assert_eq!(
        suggested(&folder(
            &[("lib/db/tables.dart", "class A extends Table {}")],
            &[]
        )),
        VizTab::Entity
    );
}

#[test]
fn flow_is_the_fallback() {
    assert_eq!(suggested(&folder(&[], &[])), VizTab::Flow);
    assert_eq!(
        suggested(&folder(
            &[("package.json", r#"{"dependencies":{"lodash":"4"}}"#)],
            &[]
        )),
        VizTab::Flow
    );
    assert_eq!(suggest("", &VizSignals::default()), VizTab::Flow);
    assert_eq!(
        suggest("/definitely/not/here", &VizSignals::default()),
        VizTab::Flow
    );
    assert_eq!(
        suggested(&folder(&[("package.json", "{not json")], &[])),
        VizTab::Flow,
        "a broken package.json falls through"
    );
}

#[test]
fn phone_wins_over_entity_when_both_match() {
    let both = folder(&[("pubspec.yaml", "x"), ("prisma/schema.prisma", "y")], &[]);
    assert_eq!(suggested(&both), VizTab::Phone);
}

#[test]
fn drift_search_gives_up_after_four_hundred_entries() {
    let root = TempDir::new("viz-drift");
    for n in 0..450 {
        root.write(&format!("lib/a{n:03}.dart"), "");
    }
    root.write("lib/zz/tables.dart", "");
    assert_eq!(suggested(&root), VizTab::Flow);
}

#[test]
fn git_branch_from_head() {
    let attached = folder(
        &[(".git/HEAD", "ref: refs/heads/feat/agent-console\n")],
        &[],
    );
    assert_eq!(
        git_branch(&attached.root().to_string_lossy()).as_deref(),
        Some("feat/agent-console")
    );
    let detached = folder(&[(".git/HEAD", "0123456789abcdef\n")], &[]);
    assert_eq!(
        git_branch(&detached.root().to_string_lossy()).as_deref(),
        Some("0123456")
    );
    assert_eq!(git_branch(&folder(&[], &[]).root().to_string_lossy()), None);
    let nested = folder(
        &[(".git/HEAD", "ref: refs/heads/main\n"), ("a/b/keep", "")],
        &[],
    );
    assert_eq!(
        git_branch(&nested.path("a/b").to_string_lossy()).as_deref(),
        Some("main"),
        "found from a subfolder"
    );
}
