use super::*;

#[test]
fn a_name_made_only_of_generic_words_has_no_service_part() {
    for name in [
        "dashboards",
        "api",
        "login",
        "Login",
        "dashboards-basic-auth",
        "api_key",
        "admin-password",
        "prod.token",
    ] {
        assert!(generic(name), "{name}");
    }
}

#[test]
fn a_name_with_a_service_or_host_part_passes() {
    for name in [
        "workflowspace-cdn-stag-basic-auth",
        "emqx-dashboard-dev-admin",
        "github",
        "npm",
        "windscribe",
        "m1-admin",
        "claude-worker",
        "aws-prod",
        "gh",
    ] {
        assert!(!generic(name), "{name}");
    }
}

#[test]
fn an_empty_name_is_not_called_generic_here() {
    assert!(!generic(""));
    assert!(!generic("-"));
}

#[test]
fn the_refusal_names_the_asked_name_and_the_rule() {
    let line = refusal("dashboards-basic-auth");
    assert!(line.starts_with("\"dashboards-basic-auth\""));
    assert!(line.contains(RULE));
    assert!(line.contains("workflowspace-cdn-stag-basic-auth"));
}

#[test]
fn the_skill_teaches_the_naming_rule_and_the_rename() {
    use super::super::skill::{content, SkillTool};
    for tool in SkillTool::ALL {
        let text = content("/x/notchling", &[], tool);
        assert!(text.contains(RULE), "{tool:?}");
        assert!(!text.contains("{naming}"), "{tool:?}");
        assert!(text.contains("The user saved it as X, not Y"), "{tool:?}");
    }
}
