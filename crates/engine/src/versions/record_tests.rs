use super::check::{CheckPart, CheckResult, CheckTier};
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
    facts.push(Fact::Checked {
        tool: Tool::Claude,
        version: v("2.1.324"),
        tier: CheckTier::FilesFlags,
        result: CheckResult::Failed,
        failed_part: Some(CheckPart::ExecFlags),
        unknown_count: 3,
        duration_ms: 1200,
    });
    facts.push(Fact::LoadFailed {
        doc: "versions/references",
    });
    facts.push(Fact::Drift {
        tool: Tool::Codex,
        machine: "vps; rm -rf ~".into(),
        expected: v("0.166.0"),
        found: v("0.167.0"),
    });
    facts.push(Fact::SelfUpdate {
        tool: Tool::Claude,
        machine: "vps".into(),
        action: "off".into(),
        ok: false,
        detail: "Didn't change ~/.claude/settings.json: changed_since_plan".into(),
    });
    facts
}

#[test]
fn drift_and_self_update_words() {
    let drift = Fact::Drift {
        tool: Tool::Codex,
        machine: "vps".into(),
        expected: v("0.166.0"),
        found: v("0.167.0"),
    };
    assert_eq!(
        drift.audit_words(),
        (
            "versions.drift",
            "vps codex 0.167.0 expected 0.166.0".to_string()
        )
    );
    let update = Fact::SelfUpdate {
        tool: Tool::Agy,
        machine: "vps".into(),
        action: "on".into(),
        ok: true,
        detail: "Self-update on in ~/x".into(),
    };
    assert_eq!(
        update.audit_words(),
        (
            "versions.self_update",
            "vps antigravity on ok self-update_on_in___x".to_string()
        )
    );
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

#[test]
fn a_checked_fact_is_fixed_words() {
    let fact = Fact::Checked {
        tool: Tool::Codex,
        version: v("0.166.0"),
        tier: CheckTier::FilesFlags,
        result: CheckResult::PartlyChecked,
        failed_part: None,
        unknown_count: 0,
        duration_ms: 12,
    };
    assert_eq!(
        fact.audit_words(),
        (
            "versions.checked",
            "codex 0.166.0 files_flags partly none 0 12".to_string()
        )
    );
}
