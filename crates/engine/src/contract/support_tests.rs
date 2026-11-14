use super::antigravity::AntigravityProvider;
use super::claude::ClaudeProvider;
use super::codex::CodexProvider;
use super::{Capability, Kind, Provider, Support, Tool};

fn all() -> [&'static dyn Provider; 3] {
    [&ClaudeProvider, &CodexProvider, &AntigravityProvider]
}

#[test]
fn the_providers_are_in_tool_order() {
    let tools: Vec<Tool> = all().iter().map(|p| p.tool()).collect();
    assert_eq!(tools, Tool::ALL);
}

#[test]
fn every_cell_has_a_short_line() {
    for p in all() {
        for c in Capability::ALL {
            let line = p.support(c).line();
            let at = format!("{} {}", p.tool().name(), c.id());
            assert!(!line.is_empty(), "{at}");
            assert!(line.chars().count() <= 120, "{at}: {line}");
            assert_eq!(line.trim(), line, "{at}");
            assert!(!line.contains('\n'), "{at}");
        }
    }
}

#[test]
fn fallbacks_name_a_flow_that_covers_them() {
    for p in all() {
        for c in Capability::ALL {
            match p.support(c) {
                Support::Fallback { flow, .. } => {
                    assert!(flow.covers(c), "{} {}", p.tool().name(), c.id())
                }
                Support::Gap {
                    meanwhile: Some(flow),
                    ..
                } => assert!(flow.covers(c), "{} {}", p.tool().name(), c.id()),
                Support::Gap { .. } | Support::Native { .. } | Support::NotApplicable { .. } => {}
            }
        }
    }
}

#[test]
fn gaps_name_a_phase_or_a_design() {
    for p in all() {
        for c in Capability::ALL {
            if let Support::Gap { plan, .. } = p.support(c) {
                let at = format!("{} {}: {plan}", p.tool().name(), c.id());
                let phase = plan
                    .strip_prefix("phase ")
                    .and_then(|r| r.split_once(": "))
                    .is_some_and(|(n, _)| matches!(n, "1" | "2" | "3" | "4" | "5" | "6"));
                let design =
                    plan.starts_with("design: 0019") || plan.starts_with("design: version-sync");
                assert!(phase || design, "{at}");
            }
        }
    }
}

#[test]
fn the_design_gaps_are_the_setup_and_version_rows() {
    let design = |p: &dyn Provider| -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|c| matches!(p.support(*c), Support::Gap { plan, .. } if plan.starts_with("design: ")))
            .collect()
    };
    assert_eq!(
        design(&ClaudeProvider),
        [
            Capability::SetupRules,
            Capability::VersionDetect,
            Capability::VersionUpdate
        ]
    );
    let other = [
        Capability::SetupRules,
        Capability::SetupHooks,
        Capability::VersionDetect,
        Capability::VersionUpdate,
    ];
    assert_eq!(design(&CodexProvider), other);
    assert_eq!(design(&AntigravityProvider), other);
}

#[test]
fn built_setup_is_native() {
    for p in all() {
        assert_eq!(p.support(Capability::SetupSkills).kind(), Kind::Native);
    }
    assert_eq!(
        ClaudeProvider.support(Capability::SetupHooks).kind(),
        Kind::Native
    );
}

#[test]
fn the_wildcard_lint_is_on_in_every_declaration() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/contract");
    for dir in ["claude", "codex", "antigravity"] {
        let text = std::fs::read_to_string(root.join(dir).join("support.rs")).unwrap();
        assert!(
            text.starts_with("#![deny(clippy::wildcard_enum_match_arm)]"),
            "{dir}"
        );
        assert!(!text.contains("_ =>"), "{dir}");
    }
}
