use std::collections::BTreeSet;

use notchling_engine::install::{detect, plan_hooks, Want, Wanted, SLOTS};

use super::agents::*;

fn written() -> BTreeSet<(String, String)> {
    let wanted = Wanted {
        command: "'/Applications/Notchling.app/Contents/MacOS/notchling' hook".into(),
        prompt: Want::Install,
        pages: Want::Install,
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
        assert!(h.what.ends_with('.') && !h.what.contains('\n'), "{h:?}");
        assert!(h.what.len() < 120, "{h:?}");
    }
    assert!(PASS_NOTE.contains("daemon") && PASS_NOTE.contains("Artifact"));
}

#[test]
fn the_page_and_tool_lists_keep_their_order() {
    assert_eq!(Page::ALL, [Page::General, Page::Agents]);
    assert_eq!(Tool::ALL, [Tool::ClaudeCode]);
    assert_eq!(Tool::ClaudeCode.sections(), [Section::Hooks]);
    assert_eq!(Page::Agents.title(), "Agents");
}
