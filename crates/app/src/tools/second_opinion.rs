use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use notchling_engine::delegate::options::Tool;
use notchling_engine::delegate::second_opinion::{build_prompt, title, ModelOption, Subject};
use notchling_engine::delegate::second_opinion_host::ProcessHost;
use notchling_engine::delegate::second_opinion_store::{Request, SecondOpinionStore};

use crate::acting::{Acting, Control};
use crate::chat::data::DataRoot;
use crate::session_source::notchling_binary;

pub const NO_STORE: &str =
    "Second opinions run through this app's notchling delegate, which a fixture root never starts";
pub const NO_TOOLS: &str = "Neither Codex nor Antigravity CLI is installed.";
pub const READ_ONLY_NOTE: &str = "Read-only. It can look at the folder but not change it.";

pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Codex => "Codex",
        Tool::Agy => "Antigravity",
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Editing,
    Running(String),
    Failed(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct SecondOpinionSheet {
    pub subject: Subject,
    pub session: String,
    pub anchor: String,
    pub folder: String,
    pub tools: Vec<Tool>,
    pub tool: Tool,
    pub codex_models: Vec<ModelOption>,
    pub agy_models: Vec<ModelOption>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: String,
    pub stage: Stage,
}

impl SecondOpinionSheet {
    pub fn new(
        subject: Subject,
        session: &str,
        anchor: &str,
        folder: &str,
        tools: Vec<Tool>,
    ) -> SecondOpinionSheet {
        SecondOpinionSheet {
            prompt: build_prompt(&subject, folder),
            subject,
            session: session.to_owned(),
            anchor: anchor.to_owned(),
            folder: folder.to_owned(),
            tool: tools.first().copied().unwrap_or(Tool::Codex),
            tools,
            codex_models: Vec::new(),
            agy_models: Vec::new(),
            model: None,
            effort: None,
            stage: Stage::Editing,
        }
    }

    pub fn subject_title(&self) -> String {
        title(&self.subject)
    }

    pub fn models(&self) -> &[ModelOption] {
        match self.tool {
            Tool::Codex => &self.codex_models,
            Tool::Agy => &self.agy_models,
        }
    }

    pub fn efforts(&self) -> Vec<String> {
        if let Some(m) = self
            .models()
            .iter()
            .find(|m| Some(&m.model) == self.model.as_ref())
        {
            return m.efforts.clone();
        }
        let mut seen: Vec<String> = Vec::new();
        for effort in self.models().iter().flat_map(|m| &m.efforts) {
            if !seen.contains(effort) {
                seen.push(effort.clone());
            }
        }
        seen
    }

    pub fn pick_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.tool = tool;
            self.model = None;
            self.effort = None;
        }
    }

    pub fn pick_model(&mut self, model: Option<&str>) {
        self.model = model.map(str::to_owned);
        if let Some(effort) = &self.effort {
            if !self.efforts().contains(effort) {
                self.effort = None;
            }
        }
    }

    pub fn pick_effort(&mut self, effort: Option<&str>) {
        self.effort = effort.map(str::to_owned);
    }

    pub fn run_control(&self, acting: Acting, has_store: bool) -> Control {
        let gate = acting.control();
        if !gate.enabled {
            return gate;
        }
        if !has_store {
            return Control {
                enabled: false,
                tooltip: Some(NO_STORE),
            };
        }
        let ready = !self.tools.is_empty()
            && !self.prompt.trim().is_empty()
            && !matches!(self.stage, Stage::Running(_));
        Control {
            enabled: ready,
            tooltip: None,
        }
    }

    pub fn request(&self) -> Request {
        Request {
            session: self.session.clone(),
            anchor: self.anchor.clone(),
            subject: self.subject_title(),
            tool: self.tool,
            model: self.model.clone().filter(|m| !m.is_empty()),
            effort: self.effort.clone().filter(|e| !e.is_empty()),
            folder: self.folder.clone(),
            prompt: self.prompt.trim().to_owned(),
        }
    }

    pub fn started(&mut self, result: Result<String, String>) {
        self.stage = match result {
            Ok(id) => Stage::Running(id),
            Err(why) => Stage::Failed(why),
        };
    }

    pub fn footer(&self) -> String {
        match &self.stage {
            Stage::Failed(why) => why.clone(),
            Stage::Running(_) => format!(
                "{} is reviewing {}…",
                tool_name(self.tool),
                self.subject_title()
            ),
            Stage::Editing => READ_ONLY_NOTE.to_owned(),
        }
    }
}

fn now_secs() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0.0, |d| d.as_secs_f64())
}

fn make_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}-{:x}-{:x}",
        (now_secs() * 1000.0) as u64,
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

pub fn store_for_run(root: Option<&DataRoot>, exe: Option<&Path>) -> Option<SecondOpinionStore> {
    let root = root.filter(|r| r.fixture.is_none())?;
    let program: PathBuf = notchling_binary(exe?)?;
    Some(SecondOpinionStore::new(
        Some(root.data_dir.clone()),
        std::env::temp_dir(),
        Arc::new(ProcessHost::new(program)),
        Box::new(now_secs),
        Box::new(make_id),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::model::{DiffKind, DiffLine};

    fn sheet() -> SecondOpinionSheet {
        let mut s = SecondOpinionSheet::new(
            Subject::Message("hi".into()),
            "codex:s1",
            "item-3",
            "/w",
            vec![Tool::Codex, Tool::Agy],
        );
        s.codex_models = vec![
            ModelOption {
                model: "sol".into(),
                name: "Sol".into(),
                efforts: vec!["low".into(), "high".into()],
            },
            ModelOption {
                model: "mini".into(),
                name: "Mini".into(),
                efforts: vec!["low".into(), "medium".into()],
            },
        ];
        s.agy_models = vec![ModelOption {
            model: "g3".into(),
            name: "g3".into(),
            efforts: vec!["standard".into()],
        }];
        s
    }

    #[test]
    fn the_prompt_is_the_engines_and_names_the_subject() {
        let s = SecondOpinionSheet::new(
            Subject::Change {
                path: "/w/app/src/main.rs".into(),
                diff: vec![DiffLine {
                    kind: DiffKind::Add,
                    text: "new();".into(),
                    number: None,
                }],
            },
            "s1",
            "a1",
            "/w/app",
            vec![Tool::Codex],
        );
        assert_eq!(s.prompt, build_prompt(&s.subject, "/w/app"));
        assert!(s.prompt.contains("+new();"));
        assert_eq!(s.subject_title(), "main.rs");
        assert_eq!(sheet().subject_title(), "Reply");
    }

    #[test]
    fn picks_follow_the_tool_and_the_model() {
        let mut s = sheet();
        assert_eq!(s.tool, Tool::Codex);
        assert_eq!(s.efforts(), ["low", "high", "medium"], "every effort, once");
        s.pick_model(Some("sol"));
        s.pick_effort(Some("high"));
        assert_eq!(s.efforts(), ["low", "high"]);
        s.pick_model(Some("mini"));
        assert_eq!(s.effort, None, "mini has no high");
        s.pick_effort(Some("medium"));
        s.pick_tool(Tool::Agy);
        assert_eq!((s.model.as_deref(), s.effort.as_deref()), (None, None));
        assert_eq!(s.efforts(), ["standard"]);
        s.pick_tool(Tool::Agy);
        s.pick_model(Some("g3"));
        s.pick_effort(Some(""));
        let request = s.request();
        assert_eq!(
            (request.tool, request.model.as_deref()),
            (Tool::Agy, Some("g3"))
        );
        assert_eq!(request.effort, None, "an empty pick is the default");
        assert_eq!(
            (request.session.as_str(), request.anchor.as_str()),
            ("codex:s1", "item-3")
        );
        assert_eq!(request.subject, "Reply");
        assert_eq!(request.prompt, s.prompt.trim());
    }

    #[test]
    fn run_is_gated_by_acting_then_the_store_then_the_sheet() {
        let mut s = sheet();
        assert_eq!(
            s.run_control(Acting::Off, true).tooltip,
            Some(crate::acting::OFF_MESSAGE)
        );
        assert_eq!(s.run_control(Acting::On, false).tooltip, Some(NO_STORE));
        assert!(s.run_control(Acting::On, true).enabled);
        s.prompt = "  ".into();
        assert!(!s.run_control(Acting::On, true).enabled);
        s.prompt = "go".into();
        s.started(Ok("run-1".into()));
        assert!(!s.run_control(Acting::On, true).enabled);
        assert_eq!(s.footer(), "Codex is reviewing Reply…");
        s.started(Err("no codex".into()));
        assert_eq!(s.footer(), "no codex");
        let none = SecondOpinionSheet::new(Subject::Message("x".into()), "s", "a", "/w", vec![]);
        assert!(!none.run_control(Acting::On, true).enabled);
        assert_eq!(none.footer(), READ_ONLY_NOTE);
        assert_eq!(tool_name(Tool::Agy), "Antigravity");
    }

    fn real_root(scratch: &crate::testutil::Scratch) -> DataRoot {
        DataRoot {
            claude_dir: scratch.path().join("claude"),
            data_dir: scratch.path().join("data"),
            codex_dir: scratch.path().join("codex"),
            antigravity_dir: scratch.path().join("agy"),
            home: scratch.path().to_path_buf(),
            fixture: None,
        }
    }

    #[test]
    fn a_fixture_root_gets_no_store_and_ids_differ() {
        let fixture = DataRoot::for_render(None);
        assert!(store_for_run(Some(&fixture), Some(Path::new("/bin/true"))).is_none());
        assert!(store_for_run(None, Some(Path::new("/bin/true"))).is_none());
        assert_ne!(make_id(), make_id());
        let scratch = crate::testutil::Scratch::new("opinions");
        let root = real_root(&scratch);
        let store = store_for_run(Some(&root), Some(Path::new("/bin/true"))).expect("a store");
        assert!(store.opinions().is_empty());
        assert!(!notchling_engine::store::exists(&root.data_dir));
    }

    #[test]
    fn the_apps_opinions_are_not_the_swift_apps_file() {
        use notchling_engine::delegate::second_opinion_store::{old_file, DOC};
        let scratch = crate::testutil::Scratch::new("opinions-path");
        let root = real_root(&scratch);
        assert_eq!(DOC, "app/second-opinions");
        let old = old_file(&root.data_dir);
        assert_eq!(old, scratch.path().join("data/app/second-opinions.json"));
        assert_ne!(
            old,
            root.data_dir.join("second-opinions.json"),
            "the Swift app's file is in data/"
        );
        assert!(
            !old.starts_with(root.data_dir.join("daemon")),
            "nor the daemon's folder"
        );
    }

    #[test]
    fn an_opinion_is_saved_in_the_apps_own_document() {
        use notchling_engine::delegate::second_opinion_store::DOC;
        let scratch = crate::testutil::Scratch::new("opinions-saved");
        let root = real_root(&scratch);
        let app = scratch.path().join("nowhere/notchling-app");
        let store = store_for_run(Some(&root), Some(&app)).expect("a store");
        store.start(&request_in(&scratch));
        assert!(notchling_engine::store::doc(&root.data_dir, DOC)
            .unwrap()
            .is_some());
        assert!(!scratch.path().join("data/second-opinions.json").exists());
    }

    #[test]
    fn the_swift_apps_opinions_file_is_left_alone_and_the_old_app_file_moves_in() {
        use notchling_engine::delegate::second_opinion_store::{old_file, DOC};
        let scratch = crate::testutil::Scratch::new("opinions-swift");
        let root = real_root(&scratch);
        let swift = scratch.path().join("data/second-opinions.json");
        std::fs::create_dir_all(swift.parent().unwrap()).unwrap();
        let theirs = r#"[{"id":"swift","not":"ours"}]"#;
        std::fs::write(&swift, theirs).unwrap();
        let old = old_file(&root.data_dir);
        std::fs::create_dir_all(old.parent().unwrap()).unwrap();
        std::fs::write(&old, "[]").unwrap();
        let app = scratch.path().join("nowhere/notchling-app");
        let store = store_for_run(Some(&root), Some(&app)).expect("a store");
        assert!(store.opinions().is_empty());
        assert!(!old.exists(), "the old app file moved in");
        assert_eq!(
            notchling_engine::store::doc(&root.data_dir, DOC)
                .unwrap()
                .as_deref(),
            Some("[]")
        );
        store.start(&request_in(&scratch));
        assert_eq!(std::fs::read_to_string(&swift).unwrap(), theirs);
    }

    fn request_in(scratch: &crate::testutil::Scratch) -> Request {
        let mut request = SecondOpinionSheet::new(
            Subject::Message("hi".into()),
            "codex:s1",
            "a1",
            &scratch.path().to_string_lossy(),
            vec![Tool::Codex],
        )
        .request();
        request.prompt = "look".into();
        request
    }

    #[test]
    fn the_delegate_command_is_the_notchling_binary_beside_the_app() {
        let app = Path::new("/Applications/Notchling.app/Contents/MacOS/notchling-app");
        assert_eq!(
            notchling_binary(app),
            Some(PathBuf::from(format!(
                "/Applications/Notchling.app/Contents/MacOS/notchling{}",
                std::env::consts::EXE_SUFFIX
            )))
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_second_opinion_runs_the_binary_beside_the_app_not_the_app() {
        use notchling_engine::delegate::second_opinion::Status;
        use std::os::unix::fs::PermissionsExt;
        let scratch = crate::testutil::Scratch::new("opinions-run");
        let root = real_root(&scratch);
        let bin = scratch.path().join("bin");
        std::fs::create_dir_all(&bin).expect("a folder");
        for name in ["notchling", "notchling-app"] {
            let script = bin.join(name);
            std::fs::write(&script, "#!/bin/sh\necho \"$1\" > \"$0.arg\"\n").expect("a script");
            std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
        }
        let store = store_for_run(Some(&root), Some(&bin.join("notchling-app"))).expect("a store");
        store.start(&request_in(&scratch));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while store.opinions().iter().any(|o| o.status == Status::Running)
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert_eq!(
            std::fs::read_to_string(bin.join("notchling.arg"))
                .as_deref()
                .ok(),
            Some("delegate\n"),
            "the sibling got `delegate`"
        );
        assert!(
            !bin.join("notchling-app.arg").exists(),
            "the app itself was not run"
        );
    }
}
