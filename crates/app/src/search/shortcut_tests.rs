use serde_json::json;

use super::*;
use crate::testutil::Scratch;

fn mods(ctrl: bool, alt: bool, shift: bool, cmd: bool) -> Mods {
    Mods {
        ctrl,
        alt,
        shift,
        cmd,
    }
}

fn label_of(text: &str) -> Option<String> {
    Shortcut::parse(text).map(|s| s.label())
}

#[test]
fn the_default_is_control_option_p() {
    assert_eq!(DEFAULT.value(), "ctrl+alt+p");
    assert_eq!(DEFAULT.label(), "⌃⌥P");
    assert_eq!(DEFAULT.carbon(), Some((0x23, 0x1800)));
    assert_eq!(Setting::parse("ctrl+alt+p"), Some(Setting::DEFAULT));
}

#[test]
fn values_round_trip_in_one_order() {
    for text in [
        "ctrl+alt+p",
        "shift+cmd+k",
        "alt+space",
        "ctrl+alt+shift+cmd+7",
    ] {
        assert_eq!(
            Shortcut::parse(text).map(|s| s.value()).as_deref(),
            Some(text)
        );
    }
    let loose = Shortcut::parse(" Command + Option + P ").map(|s| s.value());
    assert_eq!(loose.as_deref(), Some("alt+cmd+p"));
}

#[test]
fn what_cannot_be_registered_does_not_parse() {
    for text in [
        "",
        "ctrl+alt",
        "ctrl+alt+pp",
        "hyper+p",
        "ctrl+alt+f5",
        "ctrl+alt+é",
        "+",
    ] {
        assert_eq!(Shortcut::parse(text), None, "{text:?}");
    }
    assert_eq!(Setting::parse("nonsense"), None);
}

#[test]
fn labels_use_the_mac_order() {
    assert_eq!(
        label_of("cmd+shift+alt+ctrl+space").as_deref(),
        Some("⌃⌥⇧⌘Space")
    );
    assert_eq!(Setting::Off.label(), "Off");
    assert_eq!(Setting::DEFAULT.label(), "⌃⌥P");
}

#[test]
fn carbon_codes_and_modifier_bits() {
    let key = |text: &str| Shortcut::parse(text).and_then(|s| s.carbon());
    assert_eq!(key("cmd+shift+p"), Some((0x23, 0x300)));
    assert_eq!(key("ctrl+space"), Some((0x31, 0x1000)));
    assert_eq!(key("alt+a"), Some((0x00, 0x800)));
    assert_eq!(key("ctrl+alt+shift+cmd+0"), Some((0x1D, 0x1B00)));
    let codes: Vec<u32> = CODES.iter().map(|(_, c)| *c).collect();
    let mut unique = codes.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), codes.len(), "one code per key");
}

#[test]
fn a_keystroke_names_a_shortcut() {
    let m = mods(true, true, false, false);
    let want = Shortcut::parse("ctrl+alt+p");
    assert_eq!(Shortcut::from_keystroke("p", m), want);
    assert_eq!(Shortcut::from_keystroke("P", m), want);
    assert_eq!(
        Shortcut::from_keystroke("space", m),
        Shortcut::parse("ctrl+alt+space")
    );
    assert_eq!(Shortcut::from_keystroke("f5", m), None);
    assert_eq!(Shortcut::from_keystroke("escape", m), None);
}

#[test]
fn a_shortcut_needs_control_option_or_command() {
    let bad = |text: &str| Shortcut::parse(text).and_then(|s| s.problem());
    assert!(bad("p").is_some());
    assert!(bad("shift+p").is_some());
    for ok in ["ctrl+p", "alt+p", "cmd+p", "ctrl+alt+p", "shift+cmd+p"] {
        assert_eq!(bad(ok), None, "{ok}");
    }
}

#[test]
fn command_with_a_letter_warns_but_is_allowed() {
    let clash = |text: &str| Shortcut::parse(text).and_then(|s| s.clash());
    assert!(clash("cmd+p").is_some());
    assert!(clash("shift+cmd+p").is_some());
    assert_eq!(clash("alt+cmd+p"), None);
    assert_eq!(clash("cmd+space"), None);
    assert_eq!(clash("cmd+1"), None);
    assert_eq!(clash("ctrl+alt+p"), None);
    assert_eq!(Shortcut::parse("cmd+p").and_then(|s| s.problem()), None);
}

#[test]
fn the_setting_is_a_shortcut_or_off() {
    assert_eq!(Setting::parse("off"), Some(Setting::Off));
    assert_eq!(Setting::parse("Off"), Some(Setting::Off));
    assert_eq!(Setting::Off.value(), "off");
    assert_eq!(Setting::from_value(&json!("off")), Setting::Off);
    assert_eq!(Setting::from_value(&json!("alt+space")).label(), "⌥Space");
    assert_eq!(Setting::from_value(&json!("garbage")), Setting::DEFAULT);
    assert_eq!(Setting::from_value(&json!(3)), Setting::DEFAULT);
}

#[test]
fn the_key_is_listed_with_the_default() {
    let spec = notchling_engine::settings::spec(KEY).expect("the key is listed");
    assert_eq!(
        spec.default_value().as_ref().and_then(|v| v.as_str()),
        Some(DEFAULT.value().as_str())
    );
}

#[test]
fn the_setting_is_read_from_the_store_and_defaults_without_one() {
    let s = Scratch::new("shortcut");
    assert_eq!(read(s.path()), Setting::DEFAULT, "no file");
    let mut store = Settings::open(&s.path().join("settings.json")).expect("open settings");
    assert_eq!(from_settings(&store), Setting::DEFAULT, "no value");
    store.set(KEY, json!("off")).expect("set");
    assert_eq!(read(s.path()), Setting::Off);
    store.set(KEY, json!("cmd+alt+k")).expect("set");
    assert_eq!(read(s.path()).label(), "⌥⌘K");
    store.set(KEY, json!("???")).expect("set");
    assert_eq!(read(s.path()), Setting::DEFAULT, "an unreadable value");
}

#[test]
fn inside_notchling_the_key_stays_command_shift_p() {
    assert_eq!(IN_WINDOW_KEY, "cmd-shift-p");
    assert_eq!(IN_WINDOW_LABEL, "⌘⇧P");
    assert_eq!(label_of("cmd+shift+p").as_deref(), Some("⇧⌘P"));
}

#[test]
fn a_change_made_elsewhere_applies_only_for_this_key_and_only_when_it_differs() {
    let on = Setting::DEFAULT;
    assert_eq!(changed_to("chatHistory", &json!("off"), on), None);
    assert_eq!(
        changed_to(KEY, &json!("ctrl+alt+p"), on),
        None,
        "already active"
    );
    assert_eq!(
        changed_to(KEY, &json!("alt+space"), on).map(|s| s.label()),
        Some("⌥Space".to_owned())
    );
    assert_eq!(changed_to(KEY, &json!("off"), on), Some(Setting::Off));
    assert_eq!(changed_to(KEY, &json!("off"), Setting::Off), None);
    assert_eq!(
        changed_to(KEY, &json!("ctrl+alt+p"), Setting::Off),
        Some(Setting::DEFAULT),
        "a key that failed at start is tried again"
    );
    assert_eq!(
        changed_to(KEY, &json!("garbage"), Setting::Off),
        Some(Setting::DEFAULT),
        "an unreadable value means what the next start would use"
    );
}

#[test]
fn a_refusal_says_which_key_stays() {
    assert_eq!(
        refusal("Another app holds it.", Setting::DEFAULT),
        "Another app holds it. ⌃⌥P stays."
    );
    assert_eq!(
        refusal("Another app holds it.", Setting::Off),
        "Another app holds it."
    );
}
