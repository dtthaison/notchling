use super::hooks::Want;
use super::skills::*;
use super::testutil::*;
use crate::delegate::skill::SkillTool;
use std::fs;
use std::path::PathBuf;

struct Rig {
    dir: Scratch,
    roots: ToolRoots,
}

fn rig() -> Rig {
    let dir = Scratch::new("skills");
    let roots = ToolRoots {
        claude: dir.path("home/.claude"),
        codex: dir.path("home/.codex"),
        antigravity: dir.path("home/.gemini/config"),
    };
    for root in [&roots.claude, &roots.codex, &roots.antigravity] {
        fs::create_dir_all(root).unwrap();
    }
    Rig { dir, roots }
}

fn specs() -> Vec<SkillSpec> {
    let make = |name: &str, tools: Vec<SkillTool>| {
        let n = name.to_string();
        SkillSpec {
            name: name.to_string(),
            tools,
            text: Box::new(move |t| format!("---\nname: {n}\n---\nfor {}\n", tool_name(t))),
        }
    };
    let all = vec![SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity];
    vec![
        make("notchling-vault", all.clone()),
        make("notchling-delegate", all),
        make("notchling-pages", vec![SkillTool::Claude]),
    ]
}

fn everything(want: Want) -> Vec<(String, Want)> {
    ["notchling-vault", "notchling-delegate", "notchling-pages"]
        .iter()
        .map(|n| (n.to_string(), want))
        .collect()
}

fn folder(rig: &Rig, tool: SkillTool, name: &str) -> PathBuf {
    rig.roots.root(tool).join("skills").join(name)
}

#[test]
fn skills_are_written_to_each_tool_that_is_there_and_nowhere_else() {
    let rig = rig();
    let results = apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    assert_eq!(results.len(), 7);
    assert!(results
        .iter()
        .all(|r| r.error.is_none() && r.plan.action == Action::Write));
    for (tool, name, text) in [
        (SkillTool::Claude, "notchling-vault", "for claude"),
        (SkillTool::Codex, "notchling-vault", "for codex"),
        (
            SkillTool::Antigravity,
            "notchling-delegate",
            "for antigravity",
        ),
        (SkillTool::Claude, "notchling-pages", "for claude"),
    ] {
        let dir = folder(&rig, tool, name);
        assert!(fs::read_to_string(dir.join("SKILL.md"))
            .unwrap()
            .contains(text));
        assert!(dir.join(MARK).is_file());
    }
    assert!(!folder(&rig, SkillTool::Codex, "notchling-pages").exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mark = folder(&rig, SkillTool::Claude, "notchling-vault").join(MARK);
        assert_eq!(
            fs::metadata(mark).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}

#[test]
fn a_tool_that_is_not_installed_is_skipped_and_its_folder_is_not_made() {
    let rig = rig();
    fs::remove_dir_all(&rig.roots.codex).unwrap();
    let plans = plan_skills(&rig.roots, &specs(), &everything(Want::Install));
    let codex: Vec<_> = plans
        .iter()
        .filter(|p| p.tool == SkillTool::Codex)
        .collect();
    assert_eq!(codex.len(), 2);
    assert!(codex.iter().all(|p| p.action == Action::Skip));
    assert!(codex[0]
        .reason
        .as_deref()
        .unwrap()
        .contains("not installed"));
    apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    assert!(!rig.roots.codex.exists());
    let gone = plan_skills(&rig.roots, &specs(), &everything(Want::Remove));
    assert!(gone
        .iter()
        .filter(|p| p.tool == SkillTool::Codex)
        .all(|p| p.action == Action::Absent));
}

#[test]
fn the_plan_reads_and_writes_nothing() {
    let rig = rig();
    plan_skills(&rig.roots, &specs(), &everything(Want::Install));
    assert!(!rig.roots.claude.join("skills").exists());
}

#[test]
fn unchanged_text_is_left_and_changed_text_is_updated() {
    let rig = rig();
    apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    let again = plan_skills(&rig.roots, &specs(), &everything(Want::Install));
    assert!(again.iter().all(|p| p.action == Action::Unchanged));
    let file = folder(&rig, SkillTool::Claude, "notchling-vault").join("SKILL.md");
    fs::write(&file, "old text").unwrap();
    let again = plan_skills(&rig.roots, &specs(), &everything(Want::Install));
    assert_eq!(
        again.iter().filter(|p| p.action == Action::Update).count(),
        1
    );
    apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    assert!(fs::read_to_string(&file).unwrap().contains("for claude"));
}

#[test]
fn a_folder_we_did_not_make_is_never_written_over_or_removed() {
    let rig = rig();
    let mine = folder(&rig, SkillTool::Claude, "notchling-vault");
    fs::create_dir_all(&mine).unwrap();
    fs::write(mine.join("SKILL.md"), "my own notes").unwrap();
    let swift = folder(&rig, SkillTool::Codex, "notchling-delegate");
    fs::create_dir_all(&swift).unwrap();
    fs::write(
        swift.join("SKILL.md"),
        "---\nname: notchling-delegate\n---\nswift text\n",
    )
    .unwrap();

    for want in [Want::Install, Want::Remove] {
        let plans = plan_skills(&rig.roots, &specs(), &everything(want));
        let skipped: Vec<_> = plans.iter().filter(|p| p.action == Action::Skip).collect();
        assert_eq!(skipped.len(), 2, "{want:?}");
        assert!(skipped.iter().any(|p| p
            .reason
            .as_deref()
            .unwrap()
            .contains("Swift app wrote it")));
        assert!(skipped.iter().any(|p| p
            .reason
            .as_deref()
            .unwrap()
            .contains("not one Notchling made")));
        let results = apply_skills(&rig.roots, &specs(), &everything(want));
        assert!(results.iter().all(|r| r.error.is_none()));
        assert_eq!(
            fs::read_to_string(mine.join("SKILL.md")).unwrap(),
            "my own notes"
        );
        assert!(fs::read_to_string(swift.join("SKILL.md"))
            .unwrap()
            .contains("swift text"));
        assert!(!mine.join(MARK).exists() && !swift.join(MARK).exists());
    }
}

#[cfg(unix)]
#[test]
fn a_link_where_the_folder_should_be_is_refused_and_its_target_untouched() {
    use std::os::unix::fs::symlink;
    let rig = rig();
    let victim = rig.dir.path("victim");
    fs::create_dir(&victim).unwrap();
    fs::write(victim.join("keep"), "x").unwrap();
    let skills = rig.roots.claude.join("skills");
    fs::create_dir_all(&skills).unwrap();
    symlink(&victim, skills.join("notchling-vault")).unwrap();
    for want in [Want::Install, Want::Remove] {
        let results = apply_skills(&rig.roots, &specs(), &everything(want));
        let vault = results
            .iter()
            .find(|r| r.plan.tool == SkillTool::Claude && r.plan.name == "notchling-vault")
            .unwrap();
        assert_eq!(vault.plan.action, Action::Skip);
        assert!(vault.plan.reason.as_deref().unwrap().contains("link"));
        assert_eq!(fs::read_dir(&victim).unwrap().count(), 1);
    }
    let rig = self::rig();
    let elsewhere = rig.dir.path("shared-skills");
    fs::create_dir(&elsewhere).unwrap();
    symlink(&elsewhere, rig.roots.claude.join("skills")).unwrap();
    apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    assert!(elsewhere.join("notchling-vault/SKILL.md").is_file());
}

#[test]
fn removing_deletes_what_we_wrote_and_nothing_the_user_added() {
    let rig = rig();
    apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    let vault = folder(&rig, SkillTool::Claude, "notchling-vault");
    fs::write(vault.join("notes.md"), "mine").unwrap();
    let results = apply_skills(&rig.roots, &specs(), &everything(Want::Remove));
    assert!(results
        .iter()
        .all(|r| r.error.is_none() && r.plan.action == Action::Remove));
    assert!(vault.join("notes.md").is_file());
    assert!(!vault.join("SKILL.md").exists() && !vault.join(MARK).exists());
    assert!(!folder(&rig, SkillTool::Codex, "notchling-vault").exists());
    assert!(!folder(&rig, SkillTool::Claude, "notchling-pages").exists());
    assert!(rig.roots.claude.join("skills").is_dir());
    let again = plan_skills(&rig.roots, &specs(), &everything(Want::Remove));
    assert!(again
        .iter()
        .filter(|p| p.tool == SkillTool::Codex)
        .all(|p| p.action == Action::Absent));
}

#[test]
fn leave_and_unnamed_skills_are_not_planned() {
    let rig = rig();
    let wants = vec![
        ("notchling-vault".to_string(), Want::Leave),
        ("notchling-pages".to_string(), Want::Install),
    ];
    let plans = plan_skills(&rig.roots, &specs(), &wants);
    assert_eq!(plans.len(), 1);
    assert_eq!(plans[0].name, "notchling-pages");
}

#[test]
fn a_skill_whose_file_is_not_a_plain_file_is_an_error_for_that_skill_only() {
    let rig = rig();
    apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    let file = folder(&rig, SkillTool::Claude, "notchling-vault").join("SKILL.md");
    fs::remove_file(&file).unwrap();
    fs::create_dir(&file).unwrap();
    let results = apply_skills(&rig.roots, &specs(), &everything(Want::Install));
    let bad: Vec<_> = results.iter().filter(|r| r.error.is_some()).collect();
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].plan.tool, SkillTool::Claude);
}
