use super::knob::{
    place_state, plan_off, plan_on, service_state, state, Knob, KnobState, Place,
    AGY_KNOB_VERIFIED, CODEX_DAEMON_KNOB_VERIFIED,
};
use super::Tool;
use crate::install::edit::{key, set_member};
use crate::install::jsonc;
use crate::install::render::Json;

const TOKEN: &str = "sk-ant-oat01-PLANTED";

fn settings() -> String {
    format!(
        "{{\n  // keep me\n  \"model\": \"opus\",\n  \"env\": {{\n    \"ANTHROPIC_API_KEY\": \"{TOKEN}\",\n    \"OTHER\": \"x\"\n  }},\n  \"hooks\": {{\n    \"PreToolUse\": [{{\"matcher\": \"Bash\", \"hooks\": [{{\"type\": \"command\", \"command\": \"/x/notchling hook\"}}]}}]\n  }}\n}}\n"
    )
}

fn no_env() -> String {
    "{\n  // keep me\n  \"model\": \"opus\",\n  \"hooks\": {}\n}\n".to_string()
}

fn dir() -> Place {
    Place::ConfigDir("/home/w/.claude".to_string())
}

fn codex() -> Place {
    Place::CodexHome("/home/w/.codex".to_string())
}

fn value(text: &str) -> serde_json::Value {
    jsonc::to_value(&jsonc::parse(text).unwrap(), text)
}

fn off(text: &str) -> String {
    plan_off(Knob::ClaudeDisableAutoupdater, &dir(), text)
        .unwrap()
        .unwrap()
        .new_text
}

#[test]
fn one_key_added_everything_else_byte_for_byte() {
    let old = settings();
    let new = off(&old);
    let old_lines: Vec<&str> = old.lines().collect();
    let new_lines: Vec<&str> = new.lines().collect();
    assert_eq!(new_lines.len(), old_lines.len() + 1);
    let at = new_lines
        .iter()
        .position(|l| l.contains("DISABLE_AUTOUPDATER"))
        .unwrap();
    assert_eq!(new_lines[at].trim(), "\"DISABLE_AUTOUPDATER\": \"1\"");
    for (i, line) in old_lines.iter().enumerate() {
        let shown = if i < at {
            new_lines[i]
        } else {
            new_lines[i + 1]
        };
        if i + 1 == at {
            assert_eq!(shown, format!("{line},"));
        } else {
            assert_eq!(shown, *line);
        }
    }
    assert_eq!(value(&new)["env"]["DISABLE_AUTOUPDATER"], "1");
    assert_eq!(value(&new)["env"]["ANTHROPIC_API_KEY"], TOKEN);
}

#[test]
fn env_made_when_missing() {
    let old = no_env();
    let new = off(&old);
    let mut want = value(&old);
    want["env"] = serde_json::json!({"DISABLE_AUTOUPDATER": "1"});
    assert_eq!(value(&new), want);
    assert!(new.contains("// keep me"));
    assert!(new.contains("\"model\": \"opus\""));
    let blank = off("");
    assert_eq!(value(&blank)["env"]["DISABLE_AUTOUPDATER"], "1");
    let empty_object = off("{}");
    assert_eq!(value(&empty_object)["env"]["DISABLE_AUTOUPDATER"], "1");
}

#[test]
fn already_set_plans_nothing() {
    let new = off(&settings());
    assert_eq!(
        plan_off(Knob::ClaudeDisableAutoupdater, &dir(), &new).unwrap(),
        None
    );
    assert_eq!(state(Knob::ClaudeDisableAutoupdater, &new), KnobState::Set);
    let codex_set = "check_for_update_on_startup = false\n";
    assert_eq!(
        plan_off(Knob::CodexUpdateCheck, &codex(), codex_set).unwrap(),
        None
    );
}

#[test]
fn duplicate_env_refuses() {
    let two_env = "{\"env\": {\"A\": \"1\"}, \"env\": {\"B\": \"2\"}}";
    assert_eq!(
        plan_off(Knob::ClaudeDisableAutoupdater, &dir(), two_env),
        Err("duplicate_key".to_string())
    );
    let two_keys = "{\"env\": {\"DISABLE_AUTOUPDATER\": \"0\", \"DISABLE_AUTOUPDATER\": \"1\"}}";
    assert_eq!(
        plan_off(Knob::ClaudeDisableAutoupdater, &dir(), two_keys),
        Err("duplicate_key".to_string())
    );
    assert_eq!(
        plan_on(Knob::ClaudeDisableAutoupdater, &dir(), two_keys),
        Err("duplicate_key".to_string())
    );
}

#[test]
fn undo_removes_only_ours() {
    let old = settings();
    let on = off(&old);
    let back = plan_on(Knob::ClaudeDisableAutoupdater, &dir(), &on)
        .unwrap()
        .unwrap();
    assert_eq!(back.new_text, old);
    let created = off(&no_env());
    let undone = plan_on(Knob::ClaudeDisableAutoupdater, &dir(), &created)
        .unwrap()
        .unwrap();
    assert_eq!(value(&undone.new_text), value(&no_env()));
    assert!(undone.shown.starts_with("settings.json: remove"));
    let codex_on = "model = \"x\"\ncheck_for_update_on_startup = false\n";
    let toml = plan_on(Knob::CodexUpdateCheck, &codex(), codex_on)
        .unwrap()
        .unwrap();
    assert_eq!(toml.new_text, "model = \"x\"\n");
}

#[test]
fn undo_leaves_a_users_zero() {
    let zero = "{\"env\": {\"DISABLE_AUTOUPDATER\": \"0\", \"X\": \"y\"}}";
    assert_eq!(
        state(Knob::ClaudeDisableAutoupdater, zero),
        KnobState::Other("0".to_string())
    );
    assert_eq!(
        plan_on(Knob::ClaudeDisableAutoupdater, &dir(), zero).unwrap(),
        None
    );
    let turned = plan_off(Knob::ClaudeDisableAutoupdater, &dir(), zero)
        .unwrap()
        .unwrap();
    assert_eq!(value(&turned.new_text)["env"]["DISABLE_AUTOUPDATER"], "1");
    assert_eq!(value(&turned.new_text)["env"]["X"], "y");
    let true_codex = "check_for_update_on_startup = true\n";
    assert_eq!(
        plan_on(Knob::CodexUpdateCheck, &codex(), true_codex).unwrap(),
        None
    );
}

#[test]
fn shown_line_never_holds_file_content() {
    let plan = plan_off(Knob::ClaudeDisableAutoupdater, &dir(), &settings())
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.shown,
        "settings.json: \"env\": {\"DISABLE_AUTOUPDATER\": \"1\"}"
    );
    assert!(!plan.shown.contains(TOKEN));
    assert!(!plan.shown.contains("ANTHROPIC"));
    assert_eq!(plan.from_sha256.len(), 64);
    assert_ne!(plan.from_sha256, super::knob::sha256_hex(&plan.new_text));
}

#[test]
fn codex_check_is_top_level() {
    let text = "model = \"x\"\n\n[features]\nunified_exec = true\n";
    let plan = plan_off(Knob::CodexUpdateCheck, &codex(), text)
        .unwrap()
        .unwrap();
    assert_eq!(
        plan.new_text,
        "model = \"x\"\ncheck_for_update_on_startup = false\n\n[features]\nunified_exec = true\n"
    );
    assert_eq!(
        plan.shown,
        "config.toml: check_for_update_on_startup = false"
    );
    assert_eq!(Knob::CodexUpdateCheck.tool(), Tool::Codex);
    assert_eq!(
        plan_off(Knob::CodexUpdateCheck, &codex(), "a = \n"),
        Err("not_toml".to_string())
    );
}

#[test]
fn codex_daemon_knob_only_when_verified() {
    const { assert!(!CODEX_DAEMON_KNOB_VERIFIED) };
    assert!(!Knob::CodexDaemonAutoStart.verified());
    assert_eq!(
        plan_off(Knob::CodexDaemonAutoStart, &codex(), ""),
        Err("not verified yet".to_string())
    );
    assert_eq!(
        plan_on(Knob::CodexDaemonAutoStart, &codex(), ""),
        Err("not verified yet".to_string())
    );
}

#[test]
fn agy_knob_only_when_verified() {
    const { assert!(!AGY_KNOB_VERIFIED) };
    assert!(!Knob::AgyEnv.verified());
    let service = Place::ServiceFile("/home/w/.config/systemd/user/sd.service".to_string());
    assert_eq!(
        plan_off(Knob::AgyEnv, &service, ""),
        Err("not verified yet".to_string())
    );
    assert_eq!(Knob::AgyEnv.tool(), Tool::Agy);
}

#[test]
fn places_must_be_safe_and_fit() {
    let odd = Place::ConfigDir("/home/w/$(x)".to_string());
    assert_eq!(
        plan_off(Knob::ClaudeDisableAutoupdater, &odd, ""),
        Err("unsafe_path".to_string())
    );
    assert_eq!(
        plan_off(Knob::ClaudeDisableAutoupdater, &codex(), ""),
        Err("wrong_place".to_string())
    );
    let service = Place::ServiceFile("/etc/systemd/system/sd.service".to_string());
    assert_eq!(
        plan_off(Knob::ClaudeDisableAutoupdater, &service, ""),
        Err("set_by_the_service_step".to_string())
    );
}

#[test]
fn state_names_what_it_found() {
    let claude = Knob::ClaudeDisableAutoupdater;
    assert_eq!(state(claude, ""), KnobState::NotSet);
    assert_eq!(state(claude, "{}"), KnobState::NotSet);
    assert_eq!(state(claude, "nope"), KnobState::CantRead("not_json"));
    assert_eq!(state(claude, "[1]"), KnobState::CantRead("not_an_object"));
    assert_eq!(
        state(claude, "{\"env\": 3}"),
        KnobState::CantRead("env_not_an_object")
    );
    assert_eq!(
        state(claude, "{\"env\": {\"DISABLE_AUTOUPDATER\": \"true\"}}"),
        KnobState::Other("true".to_string())
    );
    assert_eq!(
        state(claude, "{\"env\": {\"DISABLE_AUTOUPDATER\": 1}}"),
        KnobState::Other("other".to_string())
    );
    assert_eq!(
        state(
            Knob::CodexUpdateCheck,
            "check_for_update_on_startup = \"no\"\n"
        ),
        KnobState::Other("other".to_string())
    );
}

#[test]
fn service_files_are_read_for_the_variable() {
    let unit = "[Service]\nEnvironment=PATH=/usr/bin\nEnvironment=DISABLE_AUTOUPDATER=1\n";
    let quoted = "[Service]\nEnvironment=\"DISABLE_AUTOUPDATER=1\" \"X=y\"\n";
    let plist = "<key>EnvironmentVariables</key><dict>\n<key>DISABLE_AUTOUPDATER</key>\n<string>1</string></dict>";
    for text in [unit, quoted, plist] {
        assert_eq!(
            service_state(Knob::ClaudeDisableAutoupdater, text),
            KnobState::Set
        );
    }
    assert_eq!(
        service_state(
            Knob::ClaudeDisableAutoupdater,
            "[Service]\nEnvironment=PATH=/x\n"
        ),
        KnobState::NotSet
    );
    assert_eq!(
        service_state(
            Knob::ClaudeDisableAutoupdater,
            "Environment=DISABLE_AUTOUPDATER=0\n"
        ),
        KnobState::Other("other".to_string())
    );
    let service = Place::ServiceFile("/etc/systemd/system/sd.service".to_string());
    assert_eq!(
        place_state(Knob::ClaudeDisableAutoupdater, &service, unit),
        KnobState::Set
    );
    assert_eq!(
        service_state(Knob::CodexUpdateCheck, unit),
        KnobState::CantRead("no_service_knob")
    );
}

#[test]
fn set_member_replaces_or_appends() {
    let text = "{\n  \"a\": 1,\n  \"env\": {\n    \"K\": \"0\",\n    \"L\": \"z\"\n  }\n}\n";
    let path = [key("env")];
    let replaced = set_member(text, &path, "K", &Json::Str("1".to_string())).unwrap();
    assert_eq!(
        replaced,
        "{\n  \"a\": 1,\n  \"env\": {\n    \"K\": \"1\",\n    \"L\": \"z\"\n  }\n}\n"
    );
    let appended = set_member(text, &path, "M", &Json::Str("1".to_string())).unwrap();
    assert_eq!(value(&appended)["env"]["M"], "1");
    assert_eq!(value(&appended)["env"]["K"], "0");
    let twice = "{\"env\": {\"K\": \"0\", \"K\": \"1\"}}";
    assert!(set_member(twice, &path, "K", &Json::Str("1".to_string())).is_err());
    assert!(set_member(text, &[key("nope")], "K", &Json::Str("1".to_string())).is_err());
}
