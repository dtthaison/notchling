use super::latest::{agy_platform, host, parse, source, source_for};
use super::{Tool, Version};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

const CLAUDE: &str = include_str!("../../tests/fixtures/versions/latest-claude.txt");
const CODEX: &str = include_str!("../../tests/fixtures/versions/latest-codex.json");
const AGY: &str = include_str!("../../tests/fixtures/versions/latest-agy.json");

#[test]
fn each_body_parses() {
    assert_eq!(parse(Tool::Claude, CLAUDE), Some(v("2.1.325")));
    assert_eq!(parse(Tool::Codex, CODEX), Some(v("0.169.0")));
    assert_eq!(parse(Tool::Agy, AGY), Some(v("1.5.8")));
    assert_eq!(parse(Tool::Notchling, CLAUDE), None);
}

#[test]
fn junk_body_is_none() {
    for tool in [Tool::Claude, Tool::Codex, Tool::Agy] {
        for body in [
            "",
            "latest",
            "<html>2.1.325</html>",
            "{\"version\": 3}",
            "{\"version\": \"1.0.0; rm -rf ~\"}",
            "not json",
            "2.1",
        ] {
            let got = parse(tool, body);
            if tool == Tool::Claude && body.is_empty() {
                assert_eq!(got, None);
            }
            assert_ne!(got, Some(v("2.1.325")), "{tool:?} {body:?}");
        }
    }
    assert_eq!(parse(Tool::Claude, "2.1.325; echo hi\n"), None);
    assert_eq!(parse(Tool::Codex, &"x".repeat(2 * 1024 * 1024)), None);
}

#[test]
fn sources_are_https_on_vendor_hosts() {
    let hosts = [
        "downloads.claude.ai",
        "registry.npmjs.org",
        "antigravity-cli-auto-updater-974169037036.us-central1.run.app",
    ];
    for tool in [Tool::Claude, Tool::Codex, Tool::Agy] {
        for (os, arch) in [
            ("macos", "aarch64"),
            ("macos", "x86_64"),
            ("linux", "x86_64"),
            ("linux", "aarch64"),
        ] {
            let url = source_for(tool, os, arch).unwrap();
            assert!(url.starts_with("https://"), "{url}");
            assert!(hosts.contains(&host(&url).unwrap()), "{url}");
            assert!(!url.contains('?'), "{url}");
        }
    }
    assert_eq!(source_for(Tool::Notchling, "macos", "aarch64"), None);
    assert_eq!(source_for(Tool::Agy, "windows", "x86_64"), None);
    assert!(source(Tool::Claude).is_some());
}

#[test]
fn agy_platform_names_follow_its_installer() {
    assert_eq!(agy_platform("macos", "aarch64"), Some("darwin_arm64"));
    assert_eq!(agy_platform("macos", "x86_64"), Some("darwin_amd64"));
    assert!(source_for(Tool::Agy, "macos", "aarch64")
        .unwrap()
        .ends_with("/manifests/darwin_arm64.json"));
}
