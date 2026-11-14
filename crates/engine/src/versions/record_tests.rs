use super::record::{Cause, Fact, MemoryRecorder, Recorder};
use super::{Method, Tool, Version};

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

fn every_fact() -> Vec<Fact> {
    let mut facts = vec![
        Fact::Seen {
            tool: Tool::Claude,
            version: v("2.1.324"),
        },
        Fact::Seen {
            tool: Tool::Agy,
            version: v("1.3.0-BETA.2"),
        },
    ];
    for cause in [
        Cause::UserUpdate,
        Cause::SelfUpdate,
        Cause::Rollout,
        Cause::Rollback,
        Cause::Outside,
    ] {
        facts.push(Fact::Changed {
            tool: Tool::Codex,
            machine: "Mac; rm -rf ~ \u{2028}\"x\"".into(),
            from: Some(v("0.165.0")),
            to: None,
            method: Method::Npm,
            cause,
        });
    }
    facts.push(Fact::LoadFailed {
        doc: "versions/references",
    });
    facts
}

#[test]
fn audit_words_hold_no_free_text() {
    for fact in every_fact() {
        let (op, result) = fact.audit_words();
        assert!(
            op.chars()
                .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
            "{op}"
        );
        assert!(
            result.chars().all(|c| c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || "_. -".contains(c)
                || c == '-'),
            "{result:?}"
        );
    }
}

#[test]
fn words_name_the_tool_and_versions() {
    let seen = Fact::Seen {
        tool: Tool::Claude,
        version: v("2.1.324"),
    };
    assert_eq!(
        seen.audit_words(),
        ("versions.seen", "claude 2.1.324".to_string())
    );
    let changed = Fact::Changed {
        tool: Tool::Agy,
        machine: "mac".into(),
        from: Some(v("1.4.1")),
        to: Some(v("1.4.2")),
        method: Method::Standalone,
        cause: Cause::Outside,
    };
    assert_eq!(
        changed.audit_words(),
        (
            "versions.changed",
            "mac antigravity 1.4.1 to 1.4.2 outside".to_string()
        )
    );
}

#[test]
fn memory_recorder_keeps_facts_in_order() {
    let recorder = MemoryRecorder::new();
    for fact in every_fact() {
        recorder.record(fact);
    }
    assert_eq!(recorder.facts(), every_fact());
}
