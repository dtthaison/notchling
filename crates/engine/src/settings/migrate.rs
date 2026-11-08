//! Migration from the Swift app's UserDefaults. Reading the real defaults domain is the app's job: it
//! exports `local.notchling` (and `local.awake.menubar` for an upgrade from Awake) to a JSON object and
//! passes it here.

use serde_json::{Map, Value};

/// What the app exports from the Swift app's UserDefaults.
#[derive(Clone, Debug, Default)]
pub struct DefaultsExport {
    /// The `local.notchling` domain.
    pub current: Map<String, Value>,
    /// The old `local.awake.menubar` domain, when it exists.
    pub awake: Option<Map<String, Value>>,
}

/// What a migration did, for the app to log.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    /// Keys copied from the Awake domain because the current domain lacked them.
    pub copied_from_awake: Vec<String>,
    /// `mode` was made from Awake v1's `active` flag.
    pub folded_active: bool,
    /// Keys in the export that aren't settings (Apple's own, window frames); dropped.
    pub ignored: Vec<String>,
    /// Settings whose value had the wrong type; dropped.
    pub invalid: Vec<String>,
}

/// The new settings and what happened.
#[derive(Clone, Debug, Default)]
pub struct Migrated {
    pub values: Map<String, Value>,
    pub report: Report,
}

/// Turns the exported defaults into new settings. Same rules as `AppDelegate.migrateFromAwake`: when
/// `migratedFromAwake` is already set, or there is no Awake domain, nothing is copied; otherwise each
/// of the 24 listed keys is copied from the Awake domain unless the current one has it, and
/// `migratedFromAwake` is set. Then every value is checked against its key (a bool exported as 0 or 1
/// becomes a bool; `Data` holding JSON, exported as base64, becomes the JSON), and `active` is folded
/// into `mode`.
pub fn migrate(_input: &DefaultsExport) -> Migrated {
    todo!()
}

/// Standard base64 (with or without padding; whitespace ignored); `None` for anything else.
pub fn base64_decode(_text: &str) -> Option<Vec<u8>> {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    /// The list in `AppDelegate.migrateFromAwake`, copied from the Swift source.
    const SWIFT_LIST: [&str; 24] = [
        "mode", "active", "keepDisplayOn", "continueAfterSleep", "hideNotch", "answerFromNotch",
        "keepPages", "showPet", "replyFromNotch", "showLastReply", "askedAccessibility", "notchWidth",
        "dismissed", "hiddenAgents", "accounts", "untrackedCostAccount", "antigravityAccounts",
        "claudeDesktopAccounts", "codexAccounts", "limitMode", "limitSwitchAt", "codexLimitMode",
        "antigravityLimitMode", "codexNetwork",
    ];

    #[test]
    fn the_key_table_has_the_swift_migration_list_in_order() {
        let ours: Vec<&str> = super::super::keys::awake_keys().map(|k| k.name).collect();
        assert_eq!(ours, SWIFT_LIST);
        assert_eq!(super::super::keys::KEYS.len(), 36);
    }

    #[test]
    fn copies_awake_keys_the_current_domain_lacks() {
        let input = DefaultsExport {
            current: map(json!({"showPet": false})),
            awake: Some(map(json!({"showPet": true, "notchWidth": "large", "limitSwitchAt": 80, "keepDisplayOn": true}))),
        };
        let m = migrate(&input);
        assert_eq!(m.values["showPet"], json!(false), "the current value wins");
        assert_eq!(m.values["notchWidth"], json!("large"));
        assert_eq!(m.values["limitSwitchAt"], json!(80));
        assert_eq!(m.values["keepDisplayOn"], json!(true));
        assert_eq!(m.values["migratedFromAwake"], json!(true));
        let mut copied = m.report.copied_from_awake.clone();
        copied.sort();
        assert_eq!(copied, vec!["keepDisplayOn", "limitSwitchAt", "notchWidth"]);
    }

    #[test]
    fn an_awake_key_outside_the_list_is_not_copied() {
        let input = DefaultsExport {
            current: Map::new(),
            awake: Some(map(json!({"chatOpen": true, "bridgeEnabled": true, "notchWidth": "small"}))),
        };
        let m = migrate(&input);
        assert!(!m.values.contains_key("chatOpen"));
        assert!(!m.values.contains_key("bridgeEnabled"));
        assert_eq!(m.values["notchWidth"], json!("small"));
    }

    #[test]
    fn nothing_is_copied_once_migrated_or_without_an_awake_domain() {
        let done = DefaultsExport {
            current: map(json!({"migratedFromAwake": true})),
            awake: Some(map(json!({"notchWidth": "large"}))),
        };
        assert!(!migrate(&done).values.contains_key("notchWidth"));
        let none = DefaultsExport { current: map(json!({"notchWidth": "small"})), awake: None };
        let m = migrate(&none);
        assert_eq!(m.values["notchWidth"], json!("small"));
        assert!(!m.values.contains_key("migratedFromAwake"), "Swift returns before setting the flag");
    }

    #[test]
    fn awake_v1_active_flag_becomes_mode() {
        let on = migrate(&DefaultsExport { current: map(json!({"active": true})), awake: None });
        assert_eq!(on.values["mode"], json!("on"));
        assert!(on.report.folded_active);
        assert!(!on.values.contains_key("active"));
        let off = migrate(&DefaultsExport { current: map(json!({"active": false})), awake: None });
        assert_eq!(off.values["mode"], json!("off"));
        // An explicit mode wins over the flag.
        let both = migrate(&DefaultsExport { current: map(json!({"active": true, "mode": "whileWorking"})), awake: None });
        assert_eq!(both.values["mode"], json!("whileWorking"));
        assert!(!both.report.folded_active);
        // Folding also works for an `active` that came from the Awake domain.
        let from_awake = migrate(&DefaultsExport { current: Map::new(), awake: Some(map(json!({"active": true}))) });
        assert_eq!(from_awake.values["mode"], json!("on"));
    }

    #[test]
    fn hide_notch_keeps_its_swift_meaning() {
        let m = migrate(&DefaultsExport { current: map(json!({"hideNotch": true})), awake: None });
        assert_eq!(m.values["hideNotch"], json!(true));
    }

    #[test]
    fn apple_keys_and_window_frames_are_ignored() {
        let input = DefaultsExport {
            current: map(json!({"AppleLanguages": ["en"], "NSWindow Frame chat": "1 2 3 4", "showPet": false})),
            awake: None,
        };
        let m = migrate(&input);
        assert_eq!(m.values.len(), 1);
        let mut ignored = m.report.ignored.clone();
        ignored.sort();
        assert_eq!(ignored, vec!["AppleLanguages", "NSWindow Frame chat"]);
    }

    #[test]
    fn values_are_coerced_or_dropped() {
        let input = DefaultsExport {
            current: map(json!({
                "showPet": 0, "keepPages": 1, "limitSwitchAt": 85.0, "chatPageWidth": 600,
                "notchWidth": "huge", "replyFromNotch": "yes", "dismissed": {"a": 1.5},
                "claudeDesktopAccounts": ["u1", "u2"], "chatProjectFolders": [1]
            })),
            awake: None,
        };
        let m = migrate(&input);
        assert_eq!(m.values["showPet"], json!(false));
        assert_eq!(m.values["keepPages"], json!(true));
        assert_eq!(m.values["limitSwitchAt"], json!(85));
        assert_eq!(m.values["chatPageWidth"], json!(600.0));
        assert_eq!(m.values["dismissed"], json!({"a": 1.5}));
        assert_eq!(m.values["claudeDesktopAccounts"], json!(["u1", "u2"]));
        let mut invalid = m.report.invalid.clone();
        invalid.sort();
        assert_eq!(invalid, vec!["chatProjectFolders", "notchWidth", "replyFromNotch"]);
    }

    #[test]
    fn data_holding_json_is_decoded_from_base64() {
        // base64 of [{"email":"a@b.c"}]
        let input = DefaultsExport {
            current: map(json!({"accounts": "W3siZW1haWwiOiJhQGIuYyJ9XQ==", "codexPicks": {"codex:1": {"model": "x"}}})),
            awake: None,
        };
        let m = migrate(&input);
        assert_eq!(m.values["accounts"], json!([{"email": "a@b.c"}]));
        assert_eq!(m.values["codexPicks"], json!({"codex:1": {"model": "x"}}), "already JSON stays");
        // Text that isn't base64 JSON is kept as given.
        let odd = migrate(&DefaultsExport { current: map(json!({"accounts": "not base64!"})), awake: None });
        assert_eq!(odd.values["accounts"], json!("not base64!"));
    }

    #[test]
    fn base64_decoding() {
        assert_eq!(base64_decode("aGk=").as_deref(), Some(&b"hi"[..]));
        assert_eq!(base64_decode("aGk").as_deref(), Some(&b"hi"[..]));
        assert_eq!(base64_decode("a G\nk=").as_deref(), Some(&b"hi"[..]));
        assert_eq!(base64_decode("").as_deref(), Some(&b""[..]));
        assert_eq!(base64_decode("a$k="), None);
        assert_eq!(base64_decode("a"), None);
    }
}
