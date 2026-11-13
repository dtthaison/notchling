use super::keys::{awake_keys, spec, KeySpec, Kind};
use super::store::{Settings, SettingsError};
use serde_json::{Map, Value};

#[derive(Clone, Debug, Default)]
pub struct DefaultsExport {
    pub current: Map<String, Value>,
    pub awake: Option<Map<String, Value>>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub copied_from_awake: Vec<String>,
    pub folded_active: bool,
    pub ignored: Vec<String>,
    pub invalid: Vec<String>,
}

#[derive(Clone, Debug, Default)]
pub struct Migrated {
    pub values: Map<String, Value>,
    pub report: Report,
}

pub fn migrate(input: &DefaultsExport) -> Migrated {
    let mut report = Report::default();
    let mut merged = input.current.clone();
    let already = merged.get("migratedFromAwake").and_then(loose_bool) == Some(true);
    if let (false, Some(awake)) = (already, &input.awake) {
        for spec in awake_keys() {
            if let Some(value) = awake.get(spec.name) {
                if !merged.contains_key(spec.name) {
                    merged.insert(spec.name.to_string(), value.clone());
                    report.copied_from_awake.push(spec.name.to_string());
                }
            }
        }
        merged.insert("migratedFromAwake".to_string(), Value::Bool(true));
    }

    let mut values = Map::new();
    for (name, value) in merged {
        let Some(spec) = spec(&name) else {
            report.ignored.push(name);
            continue;
        };
        match coerce(spec, &value) {
            Some(clean) => {
                values.insert(name, clean);
            }
            None => report.invalid.push(name),
        }
    }

    if let Some(active) = values.remove("active") {
        if !values.contains_key("mode") {
            let mode = if active == Value::Bool(true) {
                "on"
            } else {
                "off"
            };
            values.insert("mode".to_string(), Value::String(mode.to_string()));
            report.folded_active = true;
        }
    }
    Migrated { values, report }
}

pub fn import(settings: &mut Settings, export: &DefaultsExport) -> Result<Report, SettingsError> {
    let migrated = migrate(export);
    let mut values = migrated.values;
    for (name, existing) in settings.values() {
        values.insert(name.clone(), existing.clone());
    }
    settings.replace_all(values)?;
    Ok(migrated.report)
}

fn loose_bool(value: &Value) -> Option<bool> {
    match value {
        Value::Bool(b) => Some(*b),
        Value::Number(n) => match n.as_i64() {
            Some(0) => Some(false),
            Some(1) => Some(true),
            _ => None,
        },
        _ => None,
    }
}

fn coerce(spec: &KeySpec, value: &Value) -> Option<Value> {
    match spec.kind {
        Kind::Bool => loose_bool(value).map(Value::Bool),
        Kind::Int => {
            let n = value.as_i64().or_else(|| {
                value
                    .as_f64()
                    .filter(|f| f.fract() == 0.0 && f.abs() < 9e15)
                    .map(|f| f as i64)
            })?;
            Some(Value::from(n))
        }
        Kind::Double => value.as_f64().map(Value::from),
        Kind::Json => match value {
            Value::Null => None,
            Value::String(text) => Some(
                base64_decode(text)
                    .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
                    .unwrap_or_else(|| value.clone()),
            ),
            other => Some(other.clone()),
        },
        Kind::Any => Some(value.clone()),
        Kind::String | Kind::StringList | Kind::Dict => spec.accepts(value).then(|| value.clone()),
    }
}

pub fn base64_decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() * 3 / 4);
    let (mut acc, mut bits, mut padding) = (0u32, 0u32, 0usize);
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        let v = match c {
            'A'..='Z' => c as u32 - 'A' as u32,
            'a'..='z' => c as u32 - 'a' as u32 + 26,
            '0'..='9' => c as u32 - '0' as u32 + 52,
            '+' => 62,
            '/' => 63,
            '=' => {
                padding += 1;
                continue;
            }
            _ => return None,
        };
        if padding > 0 {
            return None;
        }
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if bits >= 6 || acc != 0 || padding > 2 {
        return None;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn map(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap_or_default()
    }

    const SWIFT_LIST: [&str; 24] = [
        "mode",
        "active",
        "keepDisplayOn",
        "continueAfterSleep",
        "hideNotch",
        "answerFromNotch",
        "keepPages",
        "showPet",
        "replyFromNotch",
        "showLastReply",
        "askedAccessibility",
        "notchWidth",
        "dismissed",
        "hiddenAgents",
        "accounts",
        "untrackedCostAccount",
        "antigravityAccounts",
        "claudeDesktopAccounts",
        "codexAccounts",
        "limitMode",
        "limitSwitchAt",
        "codexLimitMode",
        "antigravityLimitMode",
        "codexNetwork",
    ];

    #[test]
    fn the_key_table_has_the_swift_migration_list_in_order() {
        let ours: Vec<&str> = super::super::keys::awake_keys().map(|k| k.name).collect();
        assert_eq!(ours, SWIFT_LIST);
        assert_eq!(super::super::keys::KEYS.len(), 38);
    }

    #[test]
    fn copies_awake_keys_the_current_domain_lacks() {
        let input = DefaultsExport {
            current: map(json!({"showPet": false})),
            awake: Some(map(
                json!({"showPet": true, "notchWidth": "large", "limitSwitchAt": 80, "keepDisplayOn": true}),
            )),
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
            awake: Some(map(
                json!({"chatOpen": true, "bridgeEnabled": true, "notchWidth": "small"}),
            )),
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
        let none = DefaultsExport {
            current: map(json!({"notchWidth": "small"})),
            awake: None,
        };
        let m = migrate(&none);
        assert_eq!(m.values["notchWidth"], json!("small"));
        assert!(
            !m.values.contains_key("migratedFromAwake"),
            "Swift returns before setting the flag"
        );
    }

    #[test]
    fn awake_v1_active_flag_becomes_mode() {
        let on = migrate(&DefaultsExport {
            current: map(json!({"active": true})),
            awake: None,
        });
        assert_eq!(on.values["mode"], json!("on"));
        assert!(on.report.folded_active);
        assert!(!on.values.contains_key("active"));
        let off = migrate(&DefaultsExport {
            current: map(json!({"active": false})),
            awake: None,
        });
        assert_eq!(off.values["mode"], json!("off"));
        let both = migrate(&DefaultsExport {
            current: map(json!({"active": true, "mode": "whileWorking"})),
            awake: None,
        });
        assert_eq!(both.values["mode"], json!("whileWorking"));
        assert!(!both.report.folded_active);
        let from_awake = migrate(&DefaultsExport {
            current: Map::new(),
            awake: Some(map(json!({"active": true}))),
        });
        assert_eq!(from_awake.values["mode"], json!("on"));
    }

    #[test]
    fn hide_notch_keeps_its_swift_meaning() {
        let m = migrate(&DefaultsExport {
            current: map(json!({"hideNotch": true})),
            awake: None,
        });
        assert_eq!(m.values["hideNotch"], json!(true));
    }

    #[test]
    fn apple_keys_and_window_frames_are_ignored() {
        let input = DefaultsExport {
            current: map(
                json!({"AppleLanguages": ["en"], "NSWindow Frame chat": "1 2 3 4", "showPet": false}),
            ),
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
        assert_eq!(
            invalid,
            vec!["chatProjectFolders", "notchWidth", "replyFromNotch"]
        );
    }

    #[test]
    fn data_holding_json_is_decoded_from_base64() {
        let input = DefaultsExport {
            current: map(
                json!({"accounts": "W3siZW1haWwiOiJhQGIuYyJ9XQ==", "codexPicks": {"codex:1": {"model": "x"}}}),
            ),
            awake: None,
        };
        let m = migrate(&input);
        assert_eq!(m.values["accounts"], json!([{"email": "a@b.c"}]));
        assert_eq!(
            m.values["codexPicks"],
            json!({"codex:1": {"model": "x"}}),
            "already JSON stays"
        );
        let odd = migrate(&DefaultsExport {
            current: map(json!({"accounts": "not base64!"})),
            awake: None,
        });
        assert_eq!(odd.values["accounts"], json!("not base64!"));
    }

    #[test]
    fn import_saves_and_keeps_what_the_new_app_already_has() {
        use super::super::super::sessions::testutil::TempDir;
        let tmp = TempDir::new("import");
        tmp.write(
            "s.json",
            r#"{"version":1,"values":{"showPet":true,"futureKey":1}}"#,
        );
        let mut settings = Settings::open(&tmp.path("s.json")).expect("open");
        let export = DefaultsExport {
            current: map(json!({"showPet": false, "notchWidth": "small"})),
            awake: None,
        };
        let report = import(&mut settings, &export).expect("import");
        assert!(report.ignored.is_empty());
        let again = Settings::open(&tmp.path("s.json")).expect("reopen");
        assert_eq!(
            again.get_bool("showPet"),
            Some(true),
            "the new app's value wins"
        );
        assert_eq!(again.get_str("notchWidth").as_deref(), Some("small"));
        assert_eq!(again.raw("futureKey"), Some(&json!(1)));
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
