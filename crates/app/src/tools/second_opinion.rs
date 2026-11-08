//! The Second Opinion sheet, pure (`SecondOpinionForm`): ask Codex or Antigravity CLI to review an
//! edit or a reply. The prompt is built from what is being reviewed (the engine's
//! `delegate::second_opinion`), the tool, model and effort are picked, and Run hands a
//! [`Request`] to the engine's [`SecondOpinionStore`], which runs this binary's `notchling
//! delegate` through a `ProcessHost`.
//!
//! Starting one launches `codex` or `agy`, so Run is an acting control: disabled with the off
//! message while acting isn't on, like every other control that runs a tool.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use notchling_engine::delegate::options::Tool;
use notchling_engine::delegate::second_opinion::{build_prompt, title, ModelOption, Subject};
use notchling_engine::delegate::second_opinion_host::ProcessHost;
use notchling_engine::delegate::second_opinion_store::{file_in, Request, SecondOpinionStore};

use crate::acting::{Acting, Control};
use crate::chat::data::DataRoot;

/// Why Run is off while there is no store (a render, a fixture root).
pub const NO_STORE: &str =
    "Second opinions run through this app's notchling delegate, which a fixture root never starts";
pub const NO_TOOLS: &str = "Neither Codex nor Antigravity CLI is installed.";
pub const READ_ONLY_NOTE: &str = "Read-only. It can look at the folder but not change it.";

/// A tool's name in the sheet.
pub fn tool_name(tool: Tool) -> &'static str {
    match tool {
        Tool::Codex => "Codex",
        Tool::Agy => "Antigravity",
    }
}

/// Where the sheet is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Editing,
    /// Started; the opinion's id.
    Running(String),
    Failed(String),
}

/// The sheet's state.
#[derive(Clone, Debug, PartialEq)]
pub struct SecondOpinionSheet {
    pub subject: Subject,
    /// The Chat row the opinion is asked in, and the message or step the card sits under.
    pub session: String,
    pub anchor: String,
    pub folder: String,
    /// The tools installed, in order; empty shows [`NO_TOOLS`].
    pub tools: Vec<Tool>,
    pub tool: Tool,
    pub codex_models: Vec<ModelOption>,
    pub agy_models: Vec<ModelOption>,
    /// `None` is the tool's default (the flag is left off).
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

    /// What is reviewed, for the card ("Reply", a file name).
    pub fn subject_title(&self) -> String {
        title(&self.subject)
    }

    /// The picked tool's models.
    pub fn models(&self) -> &[ModelOption] {
        match self.tool {
            Tool::Codex => &self.codex_models,
            Tool::Agy => &self.agy_models,
        }
    }

    /// The picked model's efforts; with the default model, every effort any model offers, each
    /// once.
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

    /// Another tool: its own models, so the pick starts over.
    pub fn pick_tool(&mut self, tool: Tool) {
        if self.tool != tool {
            self.tool = tool;
            self.model = None;
            self.effort = None;
        }
    }

    /// `None` is the default model. An effort the new model doesn't offer is dropped.
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

    /// Run's gate: acting on (it launches a tool), a store to hand it to, a tool installed, a
    /// prompt and nothing running.
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

    /// What Run hands to the store.
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

    /// The line at the bottom: the failure, or the read-only note.
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

/// An id for a new opinion: the time and a counter, unique within this run and across runs.
fn make_id() -> String {
    static N: AtomicU64 = AtomicU64::new(0);
    format!(
        "{:x}-{:x}-{:x}",
        (now_secs() * 1000.0) as u64,
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    )
}

/// The running app's store: over the app's own data folder (`second-opinions.json`, never the
/// Swift app's) and this binary as the delegate command. `None` for a fixture root, which never
/// starts a tool, and when the binary can't be named.
pub fn store_for_run(root: Option<&DataRoot>, exe: Option<&Path>) -> Option<SecondOpinionStore> {
    let root = root.filter(|r| r.fixture.is_none())?;
    let program: PathBuf = exe?.to_path_buf();
    Some(SecondOpinionStore::new(
        Some(file_in(&root.data_dir)),
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

    #[test]
    fn a_fixture_root_gets_no_store_and_ids_differ() {
        let fixture = DataRoot::for_render(None);
        assert!(store_for_run(Some(&fixture), Some(Path::new("/bin/true"))).is_none());
        assert!(store_for_run(None, Some(Path::new("/bin/true"))).is_none());
        assert_ne!(make_id(), make_id());
        // A real root over a scratch folder: the store opens over our own file, writing nothing yet.
        let scratch = crate::testutil::Scratch::new("opinions");
        let root = DataRoot {
            claude_dir: scratch.path().join("claude"),
            data_dir: scratch.path().join("data"),
            codex_dir: scratch.path().join("codex"),
            antigravity_dir: scratch.path().join("agy"),
            home: scratch.path().to_path_buf(),
            fixture: None,
        };
        let store = store_for_run(Some(&root), Some(Path::new("/bin/true"))).expect("a store");
        assert!(store.opinions().is_empty());
        assert!(!scratch.path().join("data/second-opinions.json").exists());
    }
}
