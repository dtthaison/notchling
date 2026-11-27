use super::*;
use notchling_engine::accounts::codex::login;
use notchling_engine::accounts::codex::standing::standing;
use notchling_engine::limits::report::{Bar, Group};
use notchling_engine::providers::usage::{Credits, UsageReport as Produced};

const TEAM_BLOCKED: &str =
    include_str!("../../../engine/tests/fixtures/codex/wham-usage-team-blocked.json");
const NOW: f64 = 1_794_734_313.0;

fn on_the_wire(body: &str) -> (UsageReport, Option<Standing>) {
    let (_, grouped) = login::usage(body.as_bytes(), NOW);
    let wire = Produced::from(&grouped.unwrap()).to_json();
    (
        serde_json::from_value(wire).unwrap(),
        standing(body.as_bytes()),
    )
}

#[test]
fn the_captured_response_shows_what_chatgpt_shows_which_is_what_is_left() {
    let (report, standing) = on_the_wire(TEAM_BLOCKED);
    let groups = codex_groups(Some(&report), standing.as_ref(), NOW, Zone::UTC);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].name.as_deref(), Some("Codex"));
    let five = &groups[0].lines[0];
    assert_eq!(five.label, "5 hours");
    assert_eq!(five.percent_text, "0% left");
    assert_eq!(five.percent, 0.0);
    assert_eq!(five.resets.as_deref(), Some("resets in 2h 9m"));
    assert_eq!(five.fill, Fill::Hot);
    assert!(five.left);
    let week = &groups[0].lines[1];
    assert_eq!(week.label, "Week");
    assert_eq!(week.percent_text, "67% left");
    assert_eq!(week.percent, 67.0);
    assert_eq!(week.resets.as_deref(), Some("resets Sat 3:19"));
    assert_eq!(week.fill, Fill::Fine);
    assert_eq!(groups[1].name, None);
    assert!(groups[1].lines.is_empty());
    assert_eq!(
        groups[1].notes,
        vec![LimitNote {
            text: "Workspace is out of credits".into(),
            warning: true
        }]
    );
}

#[test]
fn the_used_view_that_claude_cards_keep_still_shows_the_used_percent() {
    let (report, _) = on_the_wire(TEAM_BLOCKED);
    let groups = groups(&report, NOW, Zone::UTC);
    let texts: Vec<_> = groups[0]
        .lines
        .iter()
        .map(|l| (l.percent_text.as_str(), l.percent, l.left))
        .collect();
    assert_eq!(texts, vec![("100%", 100.0, false), ("33%", 33.0, false)]);
    assert!(groups[0].notes.is_empty());
}

fn one_bar(percent: f64) -> UsageReport {
    UsageReport {
        groups: vec![Group {
            name: Some("Codex".into()),
            bars: vec![Bar {
                label: "Week".into(),
                kind: BarKind::Week,
                percent,
                resets_at: None,
            }],
        }],
        fetched: NOW,
    }
}

fn left_of(used: f64) -> (String, f64, Fill) {
    let groups = codex_groups(Some(&one_bar(used)), None, NOW, Zone::UTC);
    let l = &groups[0].lines[0];
    (l.percent_text.clone(), l.percent, l.fill)
}

#[test]
fn left_rounds_as_chatgpt_does_and_stays_between_0_and_100() {
    assert_eq!(left_of(0.0), ("100% left".into(), 100.0, Fill::Fine));
    assert_eq!(left_of(7.5).0, "93% left");
    assert_eq!(left_of(33.4).0, "67% left");
    assert_eq!(left_of(99.6).0, "0% left");
    assert_eq!(left_of(140.0), ("0% left".into(), 0.0, Fill::Hot));
    assert_eq!(left_of(-5.0).0, "100% left");
}

#[test]
fn the_fill_follows_what_is_used_so_a_nearly_empty_bar_is_hot() {
    assert_eq!(left_of(69.0).2, Fill::Fine);
    assert_eq!(left_of(70.0).2, Fill::Warm);
    assert_eq!(left_of(90.0).2, Fill::Hot);
    assert_eq!(left_of(92.0), ("8% left".into(), 8.0, Fill::Hot));
}

#[test]
fn notes_say_the_limit_credits_and_nothing_else() {
    let text = |s: Standing| -> Vec<(String, bool)> {
        codex_groups(None, Some(&s), NOW, Zone::UTC)
            .into_iter()
            .flat_map(|g| g.notes)
            .map(|n| (n.text, n.warning))
            .collect()
    };
    assert_eq!(
        text(Standing {
            reached: Some(Reached::Limit),
            credits: Some(Credits {
                unlimited: false,
                balance: Some("12.5".into())
            })
        }),
        vec![
            ("Usage limit reached".to_owned(), true),
            ("Credits: 12.5".to_owned(), false)
        ]
    );
    assert_eq!(
        text(Standing {
            reached: None,
            credits: Some(Credits {
                unlimited: true,
                balance: None
            })
        }),
        vec![("Credits: unlimited".to_owned(), false)]
    );
    assert!(codex_groups(None, Some(&Standing::default()), NOW, Zone::UTC).is_empty());
    assert!(codex_groups(None, None, NOW, Zone::UTC).is_empty());
}

#[test]
fn a_standing_with_no_bars_still_makes_a_card() {
    let standing = Standing {
        reached: Some(Reached::Credits),
        credits: None,
    };
    let groups = codex_groups(None, Some(&standing), NOW, Zone::UTC);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].notes[0].text, "Workspace is out of credits");
}
