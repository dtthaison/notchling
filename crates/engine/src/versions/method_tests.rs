use super::{can_pin, Method, Tool};

#[test]
fn paths_map_to_methods() {
    let m = Method::from_path;
    assert_eq!(
        m(
            Tool::Claude,
            "/Users/a/.local/bin/claude",
            Some("/Users/a/.local/share/claude/versions/2.1.324")
        ),
        Method::Native
    );
    assert_eq!(
        m(
            Tool::Codex,
            "/opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
            None
        ),
        Method::Npm
    );
    assert_eq!(
        m(
            Tool::Claude,
            "/opt/homebrew/Caskroom/claude-code/2.1.324/claude",
            None
        ),
        Method::Brew
    );
    assert_eq!(
        m(
            Tool::Codex,
            "/Users/a/.codex/packages/standalone/releases/0.166.0/codex",
            None
        ),
        Method::Standalone
    );
    assert_eq!(
        m(Tool::Agy, "/Users/a/.local/bin/agy", None),
        Method::Standalone
    );
    assert_eq!(
        m(
            Tool::Notchling,
            "/Users/a/.local/share/notchling/versions/0.9.0/notchling",
            None
        ),
        Method::Release
    );
    assert_eq!(m(Tool::Claude, "/usr/local/bin/claude", None), Method::Unknown);
}

#[test]
fn brew_and_agy_cannot_pin() {
    assert!(!can_pin(Tool::Claude, Method::Brew));
    assert!(!can_pin(Tool::Claude, Method::Unknown));
    assert!(!can_pin(Tool::Agy, Method::Standalone));
    assert!(can_pin(Tool::Claude, Method::Native));
    assert!(can_pin(Tool::Codex, Method::Npm));
    assert!(can_pin(Tool::Notchling, Method::Release));
}
