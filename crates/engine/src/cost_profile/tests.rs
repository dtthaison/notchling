use super::action::{Env, Options};
use super::keys::Probe;
use super::manifest::Manifest;
use super::resolve::Available;
use crate::catalog::Catalog;
use crate::install::ToolRoots;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub const MODEL_CHOICE: &str = include_str!("fixtures/model-choice.md");
pub const PSTACK_MODELS: &str = include_str!("fixtures/pstack-models.md");
pub const ORCHESTRATOR: &str = include_str!("fixtures/orchestrator.md");
pub const SETTINGS: &str = "{\n  \"env\": {\"CLAUDE_CODE_SUBAGENT_MODEL\": \"sonnet\"},\n  \"modelSettings\": {\n    \"claude-sonnet-5-5\": {\"autoCompactWindow\": 400000},\n    \"claude-opus-5-5\": {\"effortLevel\": \"medium\"}\n  },\n  \"hooks\": {}\n}\n";

pub struct Rig {
    pub dir: PathBuf,
    pub roots: ToolRoots,
    pub data: PathBuf,
}

impl Rig {
    pub fn new(tag: &str) -> Rig {
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "nl-costprofile-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        let roots = ToolRoots {
            claude: dir.join(".claude"),
            codex: dir.join(".codex"),
            antigravity: dir.join(".gemini/config"),
        };
        let data = dir.join("data");
        std::fs::create_dir_all(&data).expect("data");
        Rig { dir, roots, data }
    }

    pub fn claude(self) -> Rig {
        std::fs::create_dir_all(&self.roots.claude).expect("claude");
        self.write(&self.roots.claude.join("settings.json"), SETTINGS);
        self
    }

    pub fn codex(self) -> Rig {
        std::fs::create_dir_all(&self.roots.codex).expect("codex");
        self
    }

    pub fn agy(self) -> Rig {
        std::fs::create_dir_all(&self.roots.antigravity).expect("agy");
        self
    }

    pub fn write(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("dirs");
        std::fs::write(path, text).expect("write");
    }

    pub fn read(&self, path: &Path) -> String {
        std::fs::read_to_string(path).unwrap_or_default()
    }

    pub fn rule(&self, name: &str) -> PathBuf {
        self.roots.claude.join("rules").join(name)
    }

    pub fn agent(&self, name: &str) -> PathBuf {
        self.roots.claude.join("agents").join(name)
    }

    pub fn settings(&self) -> PathBuf {
        self.roots.claude.join("settings.json")
    }

    pub fn names(&self, dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|r| {
                r.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

pub struct Fake {
    pub reject: Vec<&'static str>,
}

impl Fake {
    pub fn all() -> Fake {
        Fake { reject: Vec::new() }
    }
}

impl Probe for Fake {
    fn accepts(&self, key: &str, _raw: &str) -> bool {
        !self.reject.iter().any(|r| key.ends_with(r))
    }
}

pub fn avail() -> Available {
    Available::from_catalog(&Catalog::built_in(), &[])
}

pub fn env<'a>(
    rig: &'a Rig,
    avail: &'a Available,
    probe: &'a dyn Probe,
    manifest: &'a Manifest,
) -> Env<'a> {
    Env {
        roots: &rig.roots,
        data_dir: &rig.data,
        avail,
        probe,
        manifest,
    }
}

pub fn everything() -> Options {
    Options::consenting(&super::action::CONSENTS)
}

mod model_tests {
    use super::super::model::{Cell, Profile, Role, Stored};
    use super::super::presets::data;
    use super::super::resolve::{resolve, Available, Offer};
    use super::*;
    use crate::catalog::Tool;
    use serde_json::json;

    fn claude(profile: &Profile, role: Role) -> String {
        resolve(profile, role, Tool::Claude, &avail())
            .map(|r| r.text())
            .unwrap_or_default()
    }

    #[test]
    fn the_built_in_presets_parse_and_balanced_is_the_default() {
        let d = data();
        assert_eq!(d.default, "balanced");
        assert_eq!(d.preset_names(), ["saver", "balanced", "quality"]);
        let p = Profile::default();
        assert_eq!(p.preset, "balanced");
        assert_eq!(claude(&p, Role::Read), "haiku medium");
        assert_eq!(claude(&p, Role::Code), "sonnet high");
        assert_eq!(claude(&p, Role::Fix), "sonnet xhigh");
        assert_eq!(claude(&p, Role::Judge), "opus xhigh");
        assert_eq!(claude(&p, Role::Hardest), "fable xhigh");
    }

    #[test]
    fn every_preset_is_at_or_above_every_floor() {
        for name in data().preset_names() {
            let p = Profile::preset(name);
            for (role, floor) in &data().floors {
                let got = resolve(&p, *role, Tool::Claude, &avail()).expect("cell");
                assert!(
                    got.notes.iter().all(|n| n.code != "floor"),
                    "{name} {role:?}"
                );
                let rank = |m: &str| {
                    ["haiku", "sonnet", "opus", "fable"]
                        .iter()
                        .position(|x| *x == m)
                };
                assert!(rank(got.model.as_deref().unwrap_or("")) >= rank(&floor.model));
            }
        }
    }

    #[test]
    fn a_pick_below_a_floor_is_raised_and_says_so() {
        let mut p = Profile::default();
        p.picks
            .insert((Role::Judge, Tool::Claude), Cell::new("haiku", "low"));
        p.picks
            .insert((Role::Hardest, Tool::Claude), Cell::new("opus", "medium"));
        p.picks
            .insert((Role::Fix, Tool::Claude), Cell::new("sonnet", "low"));
        assert_eq!(claude(&p, Role::Judge), "opus high");
        assert_eq!(claude(&p, Role::Hardest), "opus xhigh");
        assert_eq!(claude(&p, Role::Fix), "sonnet high");
        let r = resolve(&p, Role::Judge, Tool::Claude, &avail()).expect("cell");
        assert!(r.own);
        assert!(r.notes.iter().any(|n| n.code == "floor"));
    }

    #[test]
    fn a_stronger_model_at_lower_effort_passes_the_floor() {
        let mut p = Profile::default();
        p.picks
            .insert((Role::Judge, Tool::Claude), Cell::new("fable", "low"));
        assert_eq!(claude(&p, Role::Judge), "fable low");
    }

    #[test]
    fn a_family_the_catalog_lacks_falls_to_the_nearest_one_and_never_below_a_floor() {
        let mut have = avail();
        have.claude.as_mut().expect("claude").remove("fable");
        let p = Profile::default();
        let r = resolve(&p, Role::Hardest, Tool::Claude, &have).expect("cell");
        assert_eq!(r.text(), "opus xhigh");
        assert!(r.notes.iter().any(|n| n.code == "model-missing"));
        have.claude.as_mut().expect("claude").remove("haiku");
        let r = resolve(&p, Role::Read, Tool::Claude, &have).expect("cell");
        assert_eq!(r.model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn codex_picks_follow_the_live_list_and_clamp_the_effort() {
        let offers = |ids: &[(&str, &[&str])]| {
            Some(
                ids.iter()
                    .map(|(id, e)| Offer {
                        id: id.to_string(),
                        efforts: e.iter().map(|s| s.to_string()).collect(),
                    })
                    .collect::<Vec<_>>(),
            )
        };
        let p = Profile::default();
        let mut have = avail();
        have.codex = offers(&[
            ("gpt-6.2-sol", &["low", "medium"]),
            ("gpt-6-astra", &["low"]),
        ]);
        let r = resolve(&p, Role::Code, Tool::Codex, &have).expect("cell");
        assert_eq!(r.model.as_deref(), Some("gpt-6.2-sol"));
        assert_eq!(r.effort, "medium");
        let r = resolve(&p, Role::Fix, Tool::Codex, &have).expect("cell");
        assert_eq!(r.effort, "medium");
        assert!(r.notes.iter().any(|n| n.code == "effort-clamped"));
        have.codex = offers(&[("other-model", &[])]);
        let r = resolve(&p, Role::Code, Tool::Codex, &have).expect("cell");
        assert_eq!(r.model.as_deref(), Some("other-model"));
        have.codex = None;
        let r = resolve(&p, Role::Code, Tool::Codex, &have).expect("cell");
        assert_eq!(r.model.as_deref(), Some("gpt-6-sol"));
        assert!(r.notes.iter().any(|n| n.code == "unchecked"));
    }

    #[test]
    fn a_tool_that_is_off_has_no_picks() {
        let mut p = Profile::default();
        p.tools.insert(Tool::Codex, false);
        assert!(resolve(&p, Role::Code, Tool::Codex, &Available::default()).is_none());
    }

    #[test]
    fn image_has_no_model_and_the_claude_column_has_no_image_cell() {
        let p = Profile::default();
        let r = resolve(&p, Role::Image, Tool::Codex, &Available::default()).expect("cell");
        assert_eq!(r.model, None);
        assert_eq!(r.effort, "low");
        assert!(resolve(&p, Role::Image, Tool::Claude, &avail()).is_none());
    }

    #[test]
    fn the_profile_round_trips_through_its_settings_values() {
        let mut p = Profile::preset("quality");
        p.picks
            .insert((Role::Code, Tool::Codex), Cell::new("gpt-6-sol", "low"));
        p.habit_edits.insert("parallelCap".into(), 3);
        p.tools.insert(Tool::Antigravity, false);
        p.guards.insert("poll".into(), "block".into());
        assert_eq!(Profile::from_stored(&p.to_stored()), p);
        assert_eq!(p.habits().parallel_cap, 3);
        assert_eq!(p.habits().max_turns, 200);
    }

    #[test]
    fn a_hand_made_value_is_checked_not_trusted() {
        let stored = Stored {
            preset: Some("nonsense".into()),
            picks: Some(
                "code.claude=gpt-9 high; code.codex=ok-model medium; bogus.claude=opus high; junk"
                    .into(),
            ),
            tools: vec![],
            app: Some(
                json!({"habits": {"parallelCap": 9999, "maxTurns": 1}, "guards": {"poll": "x".repeat(40)}}),
            ),
        };
        let p = Profile::from_stored(&stored);
        assert_eq!(p.preset, "balanced");
        assert!(!p.picks.contains_key(&(Role::Code, Tool::Claude)));
        assert!(p.picks.contains_key(&(Role::Code, Tool::Codex)));
        assert_eq!(p.habits().parallel_cap, 12);
        assert_eq!(p.habits().max_turns, 20);
        assert!(p.guards.is_empty());
        assert_eq!(Profile::from_stored(&Stored::default()), Profile::default());
    }
}

mod diff_and_keys_tests {
    use super::super::diff::unified;
    use super::super::keys::{claude_keys, undo_json_keys};
    use super::super::model::Profile;
    use super::*;

    #[test]
    fn a_unified_diff_marks_only_the_changed_lines() {
        let d = unified("f", "a\nb\nc\nd\ne\nf\ng\n", "a\nb\nc\nX\ne\nf\ng\n");
        assert!(d.contains("-d\n+X\n"), "{d}");
        assert!(d.contains("@@ -2,5 +2,5 @@"), "{d}");
        assert_eq!(unified("f", "same\n", "same\n"), "");
        assert!(unified("f", "", "new\n").contains("+new"));
    }

    #[test]
    fn claude_keys_take_their_model_ids_from_the_model_list_not_from_code() {
        let have = avail();
        let plan = claude_keys(SETTINGS, &Profile::default(), &have, &[]).expect("plan");
        let sonnet = have.claude_id("sonnet").expect("sonnet").to_string();
        let opus = have.claude_id("opus").expect("opus").to_string();
        assert!(
            plan.text.contains("\"bashOutputMaxChars\": 12000"),
            "{}",
            plan.text
        );
        let value: serde_json::Value = serde_json::from_str(&plan.text).expect("json");
        assert_eq!(value["modelSettings"][&sonnet]["autoCompactWindow"], 250000);
        assert_eq!(value["modelSettings"][&opus]["autoCompactWindow"], 300000);
        assert_eq!(value["modelSettings"][&opus]["effortLevel"], "medium");
        assert_eq!(value["env"]["CLAUDE_CODE_SUBAGENT_MODEL"], "sonnet");
        let before = plan
            .recs
            .iter()
            .find(|r| r.path.get(1) == Some(&sonnet))
            .expect("rec");
        assert_eq!(before.before.as_deref(), Some("400000"));
        let none = plan
            .recs
            .iter()
            .find(|r| r.path[0] == "bashOutputMaxChars")
            .expect("rec");
        assert_eq!(none.before, None);
    }

    #[test]
    fn a_new_model_in_the_list_gets_its_own_key() {
        let mut have = avail();
        have.claude
            .as_mut()
            .expect("claude")
            .insert("sonnet".into(), "claude-sonnet-9-9".into());
        let plan = claude_keys("{}", &Profile::default(), &have, &[]).expect("plan");
        assert!(plan.text.contains("claude-sonnet-9-9"), "{}", plan.text);
        assert!(!plan.text.contains("claude-sonnet-5-5"));
    }

    #[test]
    fn undoing_the_keys_restores_the_file_byte_for_byte() {
        let plan = claude_keys(SETTINGS, &Profile::default(), &avail(), &[]).expect("plan");
        let (text, conflicts) = undo_json_keys(&plan.text, &plan.recs).expect("undo");
        assert!(conflicts.is_empty());
        let a: serde_json::Value = serde_json::from_str(&text).expect("json");
        let b: serde_json::Value = serde_json::from_str(SETTINGS).expect("json");
        assert_eq!(a, b);
        let empty = claude_keys("{}\n", &Profile::default(), &avail(), &[]).expect("plan");
        let (text, _) = undo_json_keys(&empty.text, &empty.recs).expect("undo");
        let value: serde_json::Value = serde_json::from_str(&text).expect("json");
        assert_eq!(value, serde_json::json!({}));
    }

    #[test]
    fn a_key_changed_since_is_left_alone_and_reported() {
        let have = avail();
        let plan = claude_keys(SETTINGS, &Profile::default(), &have, &[]).expect("plan");
        let changed = plan.text.replace("12000", "13000");
        let (text, conflicts) = undo_json_keys(&changed, &plan.recs).expect("undo");
        assert_eq!(conflicts.len(), 1);
        assert_eq!(conflicts[0].path, ["bashOutputMaxChars"]);
        assert!(text.contains("13000"));
        let again = claude_keys(&changed, &Profile::default(), &have, &plan.recs).expect("plan");
        assert!(again.skipped.iter().any(|s| s.why == "changed-since"));
        assert!(again.text.contains("13000"));
    }

    #[test]
    fn a_settings_file_that_is_not_an_object_is_refused() {
        assert!(claude_keys("[1,2]", &Profile::default(), &avail(), &[]).is_err());
    }
}
