use crate::daemon::Ops;
use crate::install_ops::*;
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, INSTALL, INSTALL_OFF};
use notchling_engine::install::{hook_command, ToolRoots};
use notchling_local_api::{ops, ErrorCode, Handler, OpSpec};
use notchling_platform::paths::Os;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

const BINARY: &str = "/opt/notchling/bin/notchling";
const USER_SETTINGS: &str = r#"{
  "env": { "API_TOKEN": "sk-ant-api03-NEVERSHOWN" },
  "hooks": {
    "PreToolUse": [
      { "matcher": "Bash", "hooks": [{ "type": "command", "command": "~/bin/guard-bash.sh" }] }
    ]
  },
  "model": "opus"
}
"#;
const SWIFT_HOOK: &str = r#"{
  "hooks": {
    "PermissionRequest": [
      { "matcher": "*", "hooks": [{ "type": "command", "command": "'/Applications/Notchling.app/Contents/MacOS/Notchling' --hook", "timeout": 300 }] }
    ]
  }
}
"#;

struct Dir(PathBuf);

impl Dir {
    fn new(tag: &str) -> Dir {
        let dir = std::env::temp_dir().join(format!("nl-installops-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        Dir(dir)
    }

    fn path(&self, rel: &str) -> PathBuf {
        self.0.join(rel)
    }

    fn tree(&self) -> Vec<String> {
        fn walk(dir: &Path, base: &Path, out: &mut Vec<String>) {
            let mut entries: Vec<_> = fs::read_dir(dir).unwrap().map(|e| e.unwrap()).collect();
            entries.sort_by_key(|e| e.file_name());
            for e in entries {
                let rel = e
                    .path()
                    .strip_prefix(base)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned();
                out.push(rel);
                if e.file_type().unwrap().is_dir() {
                    walk(&e.path(), base, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.0, &self.0, &mut out);
        out
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn rig(dir: &Dir, settings: Option<&str>) -> InstallSetup {
    for tool in [".claude", ".codex", ".gemini/config"] {
        fs::create_dir_all(dir.path(&format!("home/{tool}"))).unwrap();
    }
    if let Some(text) = settings {
        fs::write(dir.path("home/.claude/settings.json"), text).unwrap();
    }
    let tools = ToolRoots {
        claude: dir.path("home/.claude"),
        codex: dir.path("home/.codex"),
        antigravity: dir.path("home/.gemini/config"),
    };
    InstallSetup {
        settings: dir.path("home/.claude/settings.json"),
        binary: Some(PathBuf::from(BINARY)),
        os: Os::Mac,
        skills: default_skills(Path::new(BINARY), &tools.codex),
        tools,
    }
}

fn ops_for(setup: InstallSetup, on: bool) -> (InstallOps, Arc<SwitchFlag>) {
    let switch = SwitchFlag::in_memory(INSTALL, INSTALL_OFF, on);
    let stamp = Mutex::new(1_794_000_000_000u64);
    let ops = InstallOps::new(setup, switch.clone()).with_clock(Box::new(move || {
        let mut s = stamp.lock().unwrap();
        *s += 1;
        *s
    }));
    (ops, switch)
}

fn settings_text(dir: &Dir) -> String {
    fs::read_to_string(dir.path("home/.claude/settings.json")).unwrap()
}

fn data(text: &str) -> Value {
    serde_json::from_str(text).unwrap()
}

#[test]
fn the_switch_starts_off_and_the_ops_are_app_tier() {
    assert!(!SwitchFlag::off(INSTALL, INSTALL_OFF).is_on());
    let dir = Dir::new("spec");
    let (ops, _) = ops_for(rig(&dir, None), false);
    for op in [ops::INSTALL_PLAN, ops::INSTALL_APPLY, ops::INSTALL_REMOVE] {
        assert_eq!(ops.spec(op), Some(OpSpec::APP), "{op}");
    }
    assert_eq!(ops.spec("install.other"), None);
    let handler = Ops::new(Arc::new(Mutex::new(json!([])))).with_group(Arc::new(InstallOps::new(
        rig(&dir, None),
        SwitchFlag::off(INSTALL, INSTALL_OFF),
    )));
    assert_eq!(handler.spec(ops::INSTALL_APPLY), Some(OpSpec::APP));
}

#[test]
fn a_plan_works_with_the_switch_off_and_reads_only() {
    let dir = Dir::new("plan");
    let (ops, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), false);
    let before = dir.tree();
    let reply = ops.run(ops::INSTALL_PLAN, &json!({})).unwrap();
    assert_eq!(reply["on"], false);
    assert_eq!(reply["applied"], false);
    assert!(reply.get("message").is_none());
    let plan = &reply["plan"];
    assert_eq!(plan["action"], "install");
    assert_eq!(plan["changed"], true);
    assert_eq!(plan["blocked"], Value::Null);
    assert_eq!(plan["hooks"]["state"], "present");
    assert_eq!(plan["hooks"]["changes"].as_array().unwrap().len(), 5);
    assert!(plan["hooks"]["changes"][0]
        .as_str()
        .unwrap()
        .contains("add PreToolUse (AskUserQuestion)"));
    let skills = plan["skills"].as_array().unwrap();
    assert_eq!(skills.len(), 4);
    assert!(skills.iter().all(|s| s["action"] == "write"));
    assert_eq!(dir.tree(), before);
    assert_eq!(settings_text(&dir), USER_SETTINGS);

    let reply = ops
        .run(ops::INSTALL_PLAN, &json!({"action": "remove"}))
        .unwrap();
    assert_eq!(reply["plan"]["action"], "remove");
    assert_eq!(reply["plan"]["changed"], false);
}

#[test]
fn apply_and_remove_with_the_switch_off_answer_the_dry_run_and_write_nothing() {
    let dir = Dir::new("off");
    let (ops, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), false);
    let before = dir.tree();
    for op in [ops::INSTALL_APPLY, ops::INSTALL_REMOVE] {
        let reply = ops.run(op, &json!({})).unwrap();
        assert_eq!(reply["applied"], false, "{op}");
        assert_eq!(reply["on"], false, "{op}");
        assert_eq!(
            reply["message"],
            "install is off: the Swift app owns hooks and skills until the maintainer turns this on"
        );
        assert_eq!(reply["message"], INSTALL_OFF);
        assert!(reply["plan"]["hooks"]["file"]
            .as_str()
            .unwrap()
            .ends_with("settings.json"));
        assert_eq!(dir.tree(), before, "{op}");
        assert_eq!(settings_text(&dir), USER_SETTINGS);
    }
    let apply = ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert_eq!(
        apply["plan"]["hooks"]["changes"].as_array().unwrap().len(),
        5
    );
    assert_eq!(apply["plan"]["action"], "install");
    let remove = ops.run(ops::INSTALL_REMOVE, &json!({})).unwrap();
    assert_eq!(remove["plan"]["action"], "remove");
}

#[test]
fn a_switch_turned_off_while_a_call_waits_for_the_writing_lock_stops_the_write() {
    for (tag, op) in [
        ("race-apply", ops::INSTALL_APPLY),
        ("race-remove", ops::INSTALL_REMOVE),
    ] {
        let dir = Dir::new(tag);
        let (ops, switch) = ops_for(rig(&dir, Some(USER_SETTINGS)), true);
        let ops = Arc::new(ops);
        let before = dir.tree();
        let held = ops.hold_writes();
        let waiting = {
            let ops = ops.clone();
            std::thread::spawn(move || ops.run(op, &json!({})))
        };
        std::thread::sleep(std::time::Duration::from_millis(200));
        switch.set(false);
        drop(held);
        let reply = waiting.join().unwrap().unwrap();
        assert_eq!(reply["applied"], false, "{op}");
        assert_eq!(reply["on"], false, "{op}");
        assert_eq!(reply["message"], INSTALL_OFF, "{op}");
        assert_eq!(dir.tree(), before, "{op}");
        assert_eq!(settings_text(&dir), USER_SETTINGS, "{op}");
    }
}

#[test]
fn with_the_switch_on_apply_installs_hooks_and_skills_and_remove_undoes_it() {
    let dir = Dir::new("on");
    let (ops, switch) = ops_for(rig(&dir, Some(USER_SETTINGS)), true);
    assert!(switch.is_on());
    let reply = ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert_eq!(reply["applied"], true);
    assert_eq!(reply["on"], true);
    assert!(reply.get("message").is_none());
    let backup = PathBuf::from(reply["backup"].as_str().unwrap());
    assert_eq!(fs::read_to_string(&backup).unwrap(), USER_SETTINGS);
    assert!(backup
        .file_name()
        .unwrap()
        .to_string_lossy()
        .starts_with("settings.json.notchling-backup-"));

    let installed = data(&settings_text(&dir));
    let command = hook_command(Path::new(BINARY), Os::Mac);
    assert_eq!(
        installed["hooks"]["PermissionRequest"][0]["hooks"][0]["command"],
        command.as_str()
    );
    assert_eq!(
        installed["hooks"]["PreToolUse"][0],
        data(USER_SETTINGS)["hooks"]["PreToolUse"][0]
    );
    assert_eq!(installed["env"], data(USER_SETTINGS)["env"]);
    for (tool, name) in [
        (".claude", "notchling-delegate"),
        (".claude", "notchling-pages"),
        (".codex", "notchling-delegate"),
        (".gemini/config", "notchling-delegate"),
    ] {
        let skill = dir.path(&format!("home/{tool}/skills/{name}"));
        assert!(skill.join("SKILL.md").is_file(), "{tool} {name}");
        assert!(fs::read_to_string(skill.join("SKILL.md"))
            .unwrap()
            .contains(BINARY));
    }
    assert!(!dir.path("home/.codex/skills/notchling-pages").exists());
    assert!(reply["plan"]["skills"]
        .as_array()
        .unwrap()
        .iter()
        .all(|s| s["error"].is_null()));

    let again = ops.run(ops::INSTALL_APPLY, &json!({})).unwrap();
    assert_eq!(again["applied"], true);
    assert_eq!(again["plan"]["changed"], false);
    assert!(again.get("backup").is_none());

    let removed = ops.run(ops::INSTALL_REMOVE, &json!({})).unwrap();
    assert_eq!(removed["applied"], true);
    assert_eq!(settings_text(&dir), USER_SETTINGS);
    assert!(!dir.path("home/.claude/skills/notchling-delegate").exists());
    assert!(!dir.path("home/.codex/skills/notchling-delegate").exists());
    assert!(!dir.path("home/.claude/skills/notchling-pages").exists());
}

#[test]
fn what_narrows_the_call_and_a_bad_one_is_refused() {
    let dir = Dir::new("what");
    let (ops, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), true);
    ops.run(ops::INSTALL_APPLY, &json!({"what": ["page_hooks"]}))
        .unwrap();
    let v = data(&settings_text(&dir));
    assert!(v["hooks"].get("PermissionRequest").is_none());
    assert_eq!(v["hooks"]["PostToolUse"][0]["matcher"], "Artifact");
    assert!(!dir.path("home/.claude/skills").exists());

    ops.run(ops::INSTALL_APPLY, &json!({"what": ["notchling-pages"]}))
        .unwrap();
    assert!(dir
        .path("home/.claude/skills/notchling-pages/SKILL.md")
        .is_file());
    assert!(!dir.path("home/.codex/skills").exists());

    for bad in [
        json!({"what": ["nonsense"]}),
        json!({"what": "page_hooks"}),
        json!({"what": [7]}),
        json!({"with_swift_hooks": "yes"}),
    ] {
        let e = ops.run(ops::INSTALL_APPLY, &bad).unwrap_err();
        assert_eq!(e.code, ErrorCode::BadRequest, "{bad}");
    }
    let e = ops
        .run(ops::INSTALL_PLAN, &json!({"action": "sideways"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::BadRequest);
}

#[test]
fn the_swift_apps_hook_blocks_an_install_unless_told_and_never_a_removal() {
    let dir = Dir::new("swift");
    let (ops, _) = ops_for(rig(&dir, Some(SWIFT_HOOK)), true);
    let plan = ops.run(ops::INSTALL_PLAN, &json!({})).unwrap();
    assert!(plan["plan"]["blocked"]
        .as_str()
        .unwrap()
        .contains("Swift app's hook"));
    let present = &plan["plan"]["hooks"]["present"][0];
    assert_eq!(present["origin"], "swift");
    assert_eq!(present["managed"], false);
    assert!(present["command"]
        .as_str()
        .unwrap()
        .ends_with("Notchling' --hook"));

    let before = dir.tree();
    let e = ops.run(ops::INSTALL_APPLY, &json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.contains("Swift app"));
    assert_eq!(dir.tree(), before);
    assert_eq!(settings_text(&dir), SWIFT_HOOK);
    ops.run(ops::INSTALL_APPLY, &json!({"what": ["notchling-pages"]}))
        .unwrap();

    let removed = ops
        .run(
            ops::INSTALL_REMOVE,
            &json!({"what": ["prompt_hooks", "page_hooks"]}),
        )
        .unwrap();
    assert_eq!(removed["plan"]["changed"], false);
    assert_eq!(settings_text(&dir), SWIFT_HOOK);

    ops.run(ops::INSTALL_APPLY, &json!({"with_swift_hooks": true}))
        .unwrap();
    let v = data(&settings_text(&dir));
    assert_eq!(v["hooks"]["PermissionRequest"].as_array().unwrap().len(), 2);
    assert!(v["hooks"]["PermissionRequest"][0]["hooks"][0]["command"]
        .as_str()
        .unwrap()
        .contains("--hook"));
}

#[test]
fn a_broken_settings_file_is_reported_refused_and_never_written() {
    let dir = Dir::new("broken");
    let broken = "{ \"env\": { \"API_TOKEN\": \"sk-ant-api03-NEVERSHOWN\" } oops";
    let (ops, _) = ops_for(rig(&dir, Some(broken)), true);
    let plan = ops.run(ops::INSTALL_PLAN, &json!({})).unwrap();
    assert_eq!(plan["plan"]["hooks"]["state"], "refused");
    assert!(plan["plan"]["hooks"]["refused"]
        .as_str()
        .unwrap()
        .contains("not valid JSON"));
    assert_eq!(plan["plan"]["hooks"]["changed"], false);
    let before = dir.tree();
    let e = ops.run(ops::INSTALL_APPLY, &json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert_eq!(dir.tree(), before, "no backup, no skills, nothing");
    assert_eq!(settings_text(&dir), broken);
    assert!(!e.message.contains("NEVERSHOWN"), "{}", e.message);
    assert!(!plan.to_string().contains("NEVERSHOWN"));
}

#[test]
fn nothing_of_the_users_own_file_is_in_a_reply() {
    let dir = Dir::new("quiet");
    let (ops, _) = ops_for(rig(&dir, Some(USER_SETTINGS)), true);
    for op in [ops::INSTALL_PLAN, ops::INSTALL_APPLY, ops::INSTALL_REMOVE] {
        let reply = ops.run(op, &json!({})).unwrap().to_string();
        assert!(!reply.contains("NEVERSHOWN"), "{op}");
        assert!(
            !reply.contains("guard-bash"),
            "{op}: the user's own hooks are not listed"
        );
    }
}

#[test]
fn a_skill_folder_that_is_not_ours_is_skipped_with_its_reason() {
    let dir = Dir::new("foreign");
    let (ops, _) = ops_for(rig(&dir, None), true);
    let swift = dir.path("home/.claude/skills/notchling-delegate");
    fs::create_dir_all(&swift).unwrap();
    fs::write(
        swift.join("SKILL.md"),
        "---\nname: notchling-delegate\n---\nswift's\n",
    )
    .unwrap();
    let reply = ops
        .run(ops::INSTALL_APPLY, &json!({"what": ["notchling-delegate"]}))
        .unwrap();
    let skills = reply["plan"]["skills"].as_array().unwrap();
    let claude = skills.iter().find(|s| s["tool"] == "claude").unwrap();
    assert_eq!(claude["action"], "skip");
    assert!(claude["reason"]
        .as_str()
        .unwrap()
        .contains("Swift app wrote it"));
    assert_eq!(
        fs::read_to_string(swift.join("SKILL.md")).unwrap(),
        "---\nname: notchling-delegate\n---\nswift's\n"
    );
    assert!(dir
        .path("home/.codex/skills/notchling-delegate/SKILL.md")
        .is_file());
}

#[test]
fn a_missing_settings_file_is_made_and_a_missing_binary_is_a_clean_failure() {
    let dir = Dir::new("fresh");
    let (ops, _) = ops_for(rig(&dir, None), true);
    let reply = ops
        .run(ops::INSTALL_APPLY, &json!({"what": ["prompt_hooks"]}))
        .unwrap();
    assert_eq!(reply["plan"]["hooks"]["state"], "missing");
    assert!(reply.get("backup").is_none());
    assert!(data(&settings_text(&dir))["hooks"]["PermissionRequest"].is_array());

    let mut setup = rig(&dir, None);
    setup.binary = None;
    let (ops, _) = ops_for(setup, true);
    let e = ops.run(ops::INSTALL_PLAN, &json!({})).unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
}

#[test]
fn a_vault_skill_can_be_added_from_where_it_is_made() {
    let dir = Dir::new("vault");
    let setup = rig(&dir, None);
    let vault = notchling_engine::install::SkillSpec {
        name: "notchling-vault".into(),
        tools: vec![
            notchling_engine::delegate::skill::SkillTool::Claude,
            notchling_engine::delegate::skill::SkillTool::Codex,
        ],
        text: Box::new(|_| "---\nname: notchling-vault\n---\ntext\n".into()),
    };
    let (ops, _) = ops_for(setup, true);
    let ops = ops.with_skill(vault);
    ops.run(ops::INSTALL_APPLY, &json!({"what": ["notchling-vault"]}))
        .unwrap();
    assert!(dir
        .path("home/.claude/skills/notchling-vault/SKILL.md")
        .is_file());
    assert!(dir
        .path("home/.codex/skills/notchling-vault/SKILL.md")
        .is_file());
    assert!(!dir.path("home/.gemini/config/skills").exists());
}

#[test]
fn the_prompt_hooks_wait_as_long_as_the_daemon_holds_a_prompt_and_ten_seconds_more() {
    assert_eq!(
        notchling_engine::install::hooks::PROMPT_TIMEOUT,
        crate::acting_ops::HOOK_WAIT.as_secs() + 10
    );
}

#[test]
fn the_layout_puts_every_tool_where_the_swift_app_looked() {
    use crate::layout::Layout;
    let real = Layout::resolve(None, None, || {
        Some((
            "/Users/me".into(),
            "/Users/me/Library/Application Support/Notchling".into(),
        ))
    })
    .unwrap();
    let setup = InstallSetup::from_layout(&real, Some("/bin/notchling".into()));
    assert_eq!(
        setup.settings,
        PathBuf::from("/Users/me/.claude/settings.json")
    );
    assert_eq!(setup.tools.claude, PathBuf::from("/Users/me/.claude"));
    assert_eq!(setup.tools.codex, real.codex_dir());
    assert_eq!(
        setup.tools.antigravity,
        PathBuf::from("/Users/me/.gemini/config")
    );
    let fixture = Layout::resolve(None, Some(std::ffi::OsStr::new("/r")), || None).unwrap();
    let setup = InstallSetup::from_layout(&fixture, Some("/bin/notchling".into()));
    assert_eq!(setup.settings, PathBuf::from("/r/claude/settings.json"));
    assert_eq!(setup.tools.codex, PathBuf::from("/r/codex"));
    assert_eq!(setup.tools.antigravity, PathBuf::from("/r/gemini/config"));
    let names: Vec<_> = setup
        .skills
        .iter()
        .map(|s| (s.name.as_str(), s.tools.len()))
        .collect();
    assert_eq!(
        names,
        vec![("notchling-delegate", 3), ("notchling-pages", 1)]
    );
    assert!(InstallSetup::from_layout(&fixture, None).skills.is_empty());
}
