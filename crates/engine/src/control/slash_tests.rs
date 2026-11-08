//! Ports of `MenuCommandTests` and `EnabledPluginsTests`, plus matching and scanning.

use super::*;
use crate::sessions::testutil::TempDir;
use std::collections::HashMap;

// Port of MenuCommandTests.testMenuCommands.
#[test]
fn menu_commands() {
    assert!(opens_menu("/plugin"));
    assert!(opens_menu("  /MCP "));
    assert!(opens_menu("/plugin install pstack@cursor-plugins"));
    assert!(opens_menu("/effort"));
    assert_eq!(route("/plugin"), Route::Terminal);
}

// Port of MenuCommandTests.testCommandsWithOutputOrAValueStayInChat.
#[test]
fn commands_with_output_or_a_value_stay_in_chat() {
    assert!(!opens_menu("/effort high"));
    assert!(!opens_menu("/model claude-opus-5-5"));
    assert!(!opens_menu("/context"));
    assert!(!opens_menu("/pstack:poteto-mode fix the scroll"));
    assert!(!opens_menu("/plugins"));
    assert!(!opens_menu("please run /plugin"));
    assert!(!opens_menu("/"));
    assert_eq!(route("/context"), Route::Session);
    assert_eq!(route("hello"), Route::Session);
}

#[test]
fn built_ins_mark_the_menu_ones() {
    let all = built_ins();
    let plugin = all.iter().find(|c| c.name == "plugin").expect("plugin");
    assert_eq!(plugin.description, "Manage plugins · Opens a menu");
    let bare = all.iter().find(|c| c.name == "memory").expect("memory");
    assert_eq!(bare.description, "Edit memory files · Opens a menu");
    let clear = all.iter().find(|c| c.name == "clear").expect("clear");
    assert_eq!(clear.description, "Clear the conversation and start fresh");
    // Every menu command is a built-in, so the marker reaches it.
    for name in MENU_COMMANDS {
        assert!(all.iter().any(|c| c.name == name), "{name}");
    }
}

fn setup(tag: &str) -> (TempDir, String) {
    let tmp = TempDir::new(tag);
    let project = tmp.path("proj").to_string_lossy().into_owned();
    let project_json = serde_json::to_string(&project).expect("json");
    tmp.write(
        "home/.claude/plugins/installed_plugins.json",
        &format!(
            r#"{{"plugins":{{
              "sd@m":[{{"scope":"user","installPath":"/cache/sd"}}],
              "off@m":[{{"scope":"user","installPath":"/cache/off"}}],
              "pstack@m":[
                {{"scope":"project","projectPath":"/elsewhere","installPath":"/cache/pstack-old"}},
                {{"scope":"project","projectPath":{project_json},"installPath":"/cache/pstack"}}]
            }}}}"#
        ),
    );
    tmp.write(
        "home/.claude/settings.json",
        r#"{"enabledPlugins":{"sd@m":true,"off@m":true}}"#,
    );
    std::fs::create_dir_all(tmp.path("proj/.claude")).expect("mkdir");
    (tmp, project)
}

fn names(tmp: &TempDir, cwd: &str) -> HashMap<String, String> {
    enabled_plugins(&tmp.path("home"), cwd)
        .into_iter()
        .collect()
}

// Port of EnabledPluginsTests.testUserSettingsOnly.
#[test]
fn user_settings_only() {
    let (tmp, _) = setup("plug-user");
    let found = names(&tmp, "");
    assert_eq!(found.len(), 2);
    assert_eq!(found["sd"], "/cache/sd");
    assert_eq!(found["off"], "/cache/off");
}

// Port of EnabledPluginsTests.testProjectScopedPluginShowsInItsFolder.
#[test]
fn project_scoped_plugin_shows_in_its_folder() {
    let (tmp, project) = setup("plug-project");
    tmp.write(
        "proj/.claude/settings.json",
        r#"{"enabledPlugins":{"pstack@m":true}}"#,
    );
    let found = names(&tmp, &project);
    assert_eq!(
        found["pstack"], "/cache/pstack",
        "picks the install made for this folder"
    );
    assert_eq!(found["sd"], "/cache/sd", "user plugins stay");
}

// Port of EnabledPluginsTests.testLocalSettingsOverrideProjectAndUser.
#[test]
fn local_settings_override_project_and_user() {
    let (tmp, project) = setup("plug-local");
    tmp.write(
        "proj/.claude/settings.json",
        r#"{"enabledPlugins":{"pstack@m":true}}"#,
    );
    tmp.write(
        "proj/.claude/settings.local.json",
        r#"{"enabledPlugins":{"pstack@m":false,"off@m":false}}"#,
    );
    let found = names(&tmp, &project);
    assert!(!found.contains_key("pstack"));
    assert!(!found.contains_key("off"));
    assert_eq!(found["sd"], "/cache/sd");
}

#[test]
fn files_become_commands_with_a_plugin_prefix_and_no_override_of_built_ins() {
    let (tmp, project) = setup("slash-all");
    tmp.write(
        "home/.claude/commands/ship.md",
        "---\ndescription: \"Ship it. Then relax.\"\nargument-hint: [branch]\n---\nbody\n",
    );
    // A user command named like a built-in never replaces it.
    tmp.write(
        "home/.claude/commands/clear.md",
        "---\ndescription: mine\n---\n",
    );
    tmp.write(
        "proj/.claude/skills/audit/SKILL.md",
        "---\nname: deep-audit\ndescription: Audits.\n---\n",
    );
    tmp.write(
        "cache/sd/skills/approve/SKILL.md",
        "---\ndescription: Approve a stage\n---\n",
    );
    let cache = tmp.path("cache/sd").to_string_lossy().into_owned();
    tmp.write(
        "home/.claude/plugins/installed_plugins.json",
        &format!(
            r#"{{"plugins":{{"sd@m":[{{"installPath":{}}}]}}}}"#,
            serde_json::to_string(&cache).expect("json")
        ),
    );
    let found = all(&tmp.path("home"), &project);
    let get = |n: &str| found.iter().find(|c| c.name == n);
    let ship = get("ship").expect("ship");
    assert_eq!(ship.description, "Ship it.");
    assert_eq!(ship.argument_hint.as_deref(), Some("[branch]"));
    assert_eq!(
        get("deep-audit").expect("skill by its name").description,
        "Audits."
    );
    assert_eq!(
        get("sd:approve").expect("plugin skill").description,
        "Approve a stage"
    );
    assert_eq!(
        get("clear").expect("clear").description,
        "Clear the conversation and start fresh"
    );
    assert_eq!(found.iter().filter(|c| c.name == "clear").count(), 1);
}

#[test]
fn matching_puts_prefixes_first_and_honours_the_plugin_part() {
    let command = |n: &str| SlashCommand {
        name: n.to_string(),
        description: String::new(),
        argument_hint: None,
    };
    let commands = [
        command("compact"),
        command("sd:approve"),
        command("approve-all"),
        command("clear"),
    ];
    let names = |typed: &str, limit: usize| -> Vec<String> {
        matches(typed, &commands, limit)
            .into_iter()
            .map(|c| c.name)
            .collect()
    };
    assert_eq!(names("app", 6), ["sd:approve", "approve-all"]);
    assert_eq!(names("c", 6), ["compact", "clear"]);
    assert_eq!(names("", 2), ["compact", "sd:approve"]);
    assert_eq!(names("c", 1), ["compact"]);
    assert_eq!(names("ear", 6), ["clear"]);
}
