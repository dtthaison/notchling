use std::collections::BTreeSet;

use notchling_engine::install::{detect, plan_hooks, Want, Wanted, SLOTS};

use super::agents::*;

fn written() -> BTreeSet<(String, String)> {
    let wanted = Wanted {
        command: "'/Applications/Notchling.app/Contents/MacOS/notchling' hook".into(),
        prompt: Want::Install,
        pages: Want::Install,
        fence: Want::Install,
    };
    let plan = plan_hooks("{}", &wanted).expect("an empty settings file takes the hooks");
    detect(&plan.text)
        .expect("what install wrote can be read back")
        .into_iter()
        .map(|f| (f.event, f.matcher.unwrap_or_default()))
        .collect()
}

#[test]
fn every_hook_install_writes_has_a_text_and_every_text_is_written() {
    let texts: BTreeSet<_> = HOOKS
        .iter()
        .map(|h| (h.event.to_owned(), h.matcher.to_owned()))
        .collect();
    assert_eq!(texts.len(), HOOKS.len(), "no slot is explained twice");
    assert_eq!(texts, written());
    for slot in SLOTS {
        assert!(info(slot.event, slot.matcher).is_some(), "{slot:?}");
    }
    assert_eq!(HOOKS.len(), SLOTS.len());
}

#[test]
fn the_texts_are_one_plain_sentence_each() {
    for h in HOOKS {
        let what = h.what.english();
        assert!(what.ends_with('.') && !what.contains('\n'), "{h:?}");
        assert!(what.len() < 120, "{h:?}");
    }
    let pass_note = notchling_locale::settings::HOOKS_PASS_NOTE.english();
    assert!(pass_note.contains("daemon") && pass_note.contains("Artifact"));
}

#[test]
fn the_page_and_tool_lists_keep_their_order() {
    assert_eq!(
        Page::ALL,
        [
            Page::General,
            Page::Organizations,
            Page::Agents,
            Page::Skills,
            Page::Costs,
            Page::Activity
        ]
    );
    assert_eq!(
        Tool::ALL,
        [Tool::ClaudeCode, Tool::Codex, Tool::Antigravity]
    );
    assert_eq!(
        Tool::ClaudeCode.sections(),
        [
            Section::Settings,
            Section::Client,
            Section::Chrome,
            Section::Hooks,
            Section::Capabilities
        ]
    );
    assert_eq!(
        Tool::Codex.sections(),
        [Section::Settings, Section::Capabilities]
    );
    assert_eq!(
        Tool::Antigravity.sections(),
        [Section::Settings, Section::Capabilities]
    );
    assert_eq!(Page::Organizations.title(), "Organizations");
    assert_eq!(Page::Agents.title(), "Agents");
    assert_eq!(Page::Skills.title(), "Skills & Models");
    assert_eq!(Page::Activity.title(), "Activity");
}
