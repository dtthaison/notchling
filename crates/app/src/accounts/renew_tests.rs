use super::limits::Zone;
use super::model::{page, RowContent, CLAUDE_CODE};
use super::renders::{snapshot, Shown, NOW};
use super::wire::{parse_claude_usage, Renewal};
use crate::switches::Switches;
use serde_json::json;

fn team_card(renewal: Option<Renewal>) -> RowContent {
    let mut s = snapshot(Shown::Claude);
    s.claude_usage.errors.insert(
        "a4".into(),
        "Its login has expired. Claude Code renews it the next time it runs.".into(),
    );
    if let Some(state) = renewal {
        s.claude_usage.renewals.insert("a4".into(), state);
    }
    page(CLAUDE_CODE, &s, &Switches::all_on(), false, NOW, Zone::UTC).rows[3]
        .content
        .clone()
}

#[test]
fn a_card_says_renewing_while_claude_code_renews_its_login_and_why_it_could_not() {
    assert_eq!(
        team_card(Some(Renewal::Renewing)),
        RowContent::Text {
            text: "Renewing login…".into(),
            warning: false
        }
    );
    assert_eq!(
        team_card(Some(Renewal::Failed)),
        RowContent::Text {
            text: "Claude Code couldn't renew this login. Sign in again from ⋯.".into(),
            warning: true
        }
    );
    let RowContent::Text { text, warning } = team_card(None) else {
        panic!("a reason");
    };
    assert!(warning && text.starts_with("Couldn't read limits. Its login has expired."));
}

#[test]
fn the_usage_reply_names_the_logins_being_renewed() {
    let usage = parse_claude_usage(&json!({
        "reports": {},
        "errors": {},
        "renewals": {"a": "renewing", "b": "failed", "c": "other", "d": 3}
    }));
    assert_eq!(usage.renewals.len(), 2);
    assert_eq!(usage.renewals["a"], Renewal::Renewing);
    assert_eq!(usage.renewals["b"], Renewal::Failed);
    assert!(parse_claude_usage(&json!({"reports": {}}))
        .renewals
        .is_empty());
}
