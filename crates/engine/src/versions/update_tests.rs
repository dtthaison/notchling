use super::update::{go_back_target, update_argv};
use super::{can_pin, Method, Tool, Version};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn s(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

#[test]
fn argv_per_method() {
    assert_eq!(
        update_argv(Tool::Claude, Method::Native, None).unwrap(),
        s(&["claude", "update"])
    );
    assert_eq!(
        update_argv(Tool::Claude, Method::Npm, None).unwrap(),
        s(&["npm", "install", "-g", "@anthropic-ai/claude-code@latest"])
    );
    assert_eq!(
        update_argv(Tool::Claude, Method::Brew, None).unwrap(),
        s(&["brew", "upgrade", "--cask", "claude-code"])
    );
    assert_eq!(
        update_argv(Tool::Codex, Method::Npm, None).unwrap(),
        s(&["npm", "install", "-g", "@openai/codex@latest"])
    );
    assert_eq!(
        update_argv(Tool::Codex, Method::Brew, None).unwrap(),
        s(&["brew", "upgrade", "--cask", "codex"])
    );
    let codex = update_argv(Tool::Codex, Method::Standalone, None).unwrap();
    assert_eq!(codex[..2], s(&["sh", "-c"]));
    assert!(codex[2].contains("https://chatgpt.com/codex/install.sh"));
    assert!(!codex[2].contains("--release"));
    assert_eq!(
        update_argv(Tool::Agy, Method::Standalone, None).unwrap(),
        s(&["agy", "update"])
    );
    assert!(update_argv(Tool::Notchling, Method::Release, None).is_err());
    assert!(update_argv(Tool::Claude, Method::Unknown, None).is_err());
}

#[test]
fn exact_only_where_it_can_pin() {
    let to = v("2.1.323");
    assert_eq!(
        update_argv(Tool::Claude, Method::Native, Some(&to)).unwrap(),
        s(&["claude", "install", "2.1.323"])
    );
    assert_eq!(
        update_argv(Tool::Claude, Method::Npm, Some(&to)).unwrap(),
        s(&["npm", "install", "-g", "@anthropic-ai/claude-code@2.1.323"])
    );
    let codex = update_argv(Tool::Codex, Method::Standalone, Some(&v("0.165.0"))).unwrap();
    assert!(
        codex[2].ends_with("sh -s -- --release 0.165.0"),
        "{}",
        codex[2]
    );
    assert_eq!(
        update_argv(Tool::Claude, Method::Brew, Some(&to)).unwrap_err(),
        "Homebrew can't install an exact version"
    );
    assert!(update_argv(Tool::Agy, Method::Standalone, Some(&to)).is_err());
    for tool in Tool::ALL {
        for method in [
            Method::Native,
            Method::Standalone,
            Method::Npm,
            Method::Brew,
            Method::Apt,
            Method::Release,
            Method::Unknown,
        ] {
            let pinned = update_argv(tool, method, Some(&to)).is_ok();
            assert_eq!(
                pinned,
                can_pin(tool, method) && tool != Tool::Notchling,
                "{tool:?} {method:?}"
            );
        }
    }
}

#[test]
fn to_must_be_a_checked_version() {
    let seen = [v("2.1.321"), v("2.1.322"), v("2.1.323"), v("2.1.324")];
    assert_eq!(
        go_back_target(&seen, &v("2.1.324"), &[]),
        Some(v("2.1.323"))
    );
    assert_eq!(
        go_back_target(&seen, &v("2.1.324"), &[v("2.1.323")]),
        Some(v("2.1.322"))
    );
    assert_eq!(go_back_target(&seen, &v("2.1.321"), &[]), None);
    assert_eq!(go_back_target(&[], &v("2.1.324"), &[]), None);
    assert_eq!(
        go_back_target(&seen, &v("2.1.323"), &[]),
        Some(v("2.1.322"))
    );
}
