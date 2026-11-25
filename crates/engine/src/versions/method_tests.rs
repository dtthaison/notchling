use super::{can_pin, Method, Tool};

#[test]
fn paths_map_to_methods() {
    let cases: [(Tool, &str, Option<&str>, Method); 11] = [
        (
            Tool::Claude,
            "/Users/a/.local/bin/claude",
            Some("/Users/a/.local/share/claude/versions/2.1.324"),
            Method::Native,
        ),
        (
            Tool::Claude,
            "/Users/a/.local/share/claude/versions/2.1.324",
            None,
            Method::Native,
        ),
        (
            Tool::Claude,
            "/opt/homebrew/bin/claude",
            Some("../lib/node_modules/@anthropic-ai/claude-code/cli.js"),
            Method::Npm,
        ),
        (
            Tool::Codex,
            "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
            None,
            Method::Npm,
        ),
        (
            Tool::Claude,
            "/opt/homebrew/Caskroom/claude-code/2.1.324/claude",
            None,
            Method::Brew,
        ),
        (
            Tool::Codex,
            "/opt/homebrew/Cellar/codex/0.166.0/bin/codex",
            None,
            Method::Brew,
        ),
        (
            Tool::Codex,
            "/Users/a/.codex/packages/standalone/0.166.0/codex",
            None,
            Method::Standalone,
        ),
        (
            Tool::Agy,
            "/Users/a/.local/bin/agy",
            None,
            Method::Standalone,
        ),
        (
            Tool::Notchling,
            "/home/w/.local/bin/notchling",
            Some("/home/w/.local/share/notchling/versions/1.3.0/notchling"),
            Method::Release,
        ),
        (Tool::Claude, "/usr/local/bin/claude", None, Method::Unknown),
        (
            Tool::Codex,
            "/Users/a/.local/share/claude/versions/2.1.3",
            None,
            Method::Unknown,
        ),
    ];
    for (tool, path, link, want) in cases {
        assert_eq!(
            Method::from_path(tool, path, link),
            want,
            "{tool:?} {path} {link:?}"
        );
    }
}

#[test]
fn brew_and_agy_cannot_pin() {
    assert!(can_pin(Tool::Claude, Method::Native));
    assert!(can_pin(Tool::Claude, Method::Npm));
    assert!(can_pin(Tool::Codex, Method::Standalone));
    assert!(can_pin(Tool::Codex, Method::Npm));
    assert!(can_pin(Tool::Notchling, Method::Release));
    for tool in Tool::ALL {
        assert!(!can_pin(tool, Method::Brew), "{tool:?}");
        assert!(!can_pin(tool, Method::Apt), "{tool:?}");
        assert!(!can_pin(tool, Method::Unknown), "{tool:?}");
    }
    assert!(!can_pin(Tool::Agy, Method::Standalone));
}

#[test]
fn titles_are_plain() {
    assert_eq!(Method::Native.title(), "native installer");
    assert_eq!(Method::Standalone.title(), "standalone installer");
    assert_eq!(Method::Brew.title(), "Homebrew");
    assert_eq!(Method::Npm.title(), "npm");
    assert_eq!(Method::Apt.title(), "apt");
    assert_eq!(Method::Release.title(), "release");
    assert_eq!(Method::Unknown.title(), "unknown");
    assert_eq!(
        serde_json::to_string(&Method::Standalone).unwrap(),
        "\"standalone\""
    );
}
