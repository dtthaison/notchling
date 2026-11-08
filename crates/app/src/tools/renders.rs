//! The tools' views for `--render` (part U10), over fixtures and made-up data, never a daemon or
//! the real home: the Antigravity permission card peeking out of the notch, the Codex model
//! picker over the chat fixture's `models_cache.json`, and the Second Opinion sheet.

use std::path::Path;

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::tools::codex::models;
use notchling_engine::transcript::model::{DiffKind, DiffLine, SessionFacts};
use serde_json::Value;

use super::codex_picker::CodexPicker;
use super::requests::Pick;
use super::second_opinion::{ModelOption, OpinionTool, SecondOpinionSheet, Subject};
use crate::acting::Acting;
use crate::chat::data::DataRoot as ChatRoot;
use crate::notch::layout::NotchState;
use crate::notch::model::NotchModel;
use crate::notch::renders::Scene;
use crate::notch::requests::HookRequest;
use crate::notch::source::{fixture_clock, DataRoot, Source};
use crate::views::ViewSpec;

pub const AGY_PERMISSION_CARD: &str = "agy-permission-card";
pub const CODEX_MODEL_PICKER: &str = "codex-model-picker";
pub const SECOND_OPINION_SHEET: &str = "second-opinion-sheet";

pub const VIEWS: [ViewSpec; 3] = [
    ViewSpec {
        name: AGY_PERMISSION_CARD,
        size: (560.0, 480.0),
        about: "an Antigravity permission peeking out of the notch, its own options live (fixture data)",
    },
    ViewSpec {
        name: CODEX_MODEL_PICKER,
        size: (360.0, 420.0),
        about: "the Codex model and effort picker over the fixture's models cache",
    },
    ViewSpec {
        name: SECOND_OPINION_SHEET,
        size: (680.0, 560.0),
        about: "the Second Opinion sheet over an edit, acting off: Run disabled (made-up data)",
    },
];

/// What a tools render shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    AgyPermission,
    CodexPicker,
    SecondOpinion,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        AGY_PERMISSION_CARD => Shown::AgyPermission,
        CODEX_MODEL_PICKER => Shown::CodexPicker,
        SECOND_OPINION_SHEET => Shown::SecondOpinion,
        _ => return None,
    })
}

/// The demo Antigravity row the permission is for.
pub const AGY_ROW: &str = "agy:demo-1";

fn agy_prompt() -> Option<Value> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/notch-scenes/agy-permission.json");
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// The notch over the fixture, with an Antigravity session added that waits on a permission, in a
/// peek with acting on, so the card's options are live.
pub fn agy_permission_scene() -> Scene {
    let mut source = Source::keeping_everything(DataRoot::for_render());
    let first = source.load(0);
    let now = fixture_clock(&first.agents);
    let mut loaded = source.load(now);
    loaded.agents.push(Agent {
        id: AGY_ROW.to_owned(),
        name: "Edit hello.txt".to_owned(),
        project: "notchling".to_owned(),
        cwd: "/Users/me/work/notchling".to_owned(),
        group: AgentGroup::Working,
        label: "Working".to_owned(),
        detail: None,
        since_ms: now - 30_000,
        target: AgentTarget::Resume {
            session_id: "demo-1".to_owned(),
            cwd: "/Users/me/work/notchling".to_owned(),
        },
        suggested_reply: None,
        transcript_path: None,
        host: "Antigravity".to_owned(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    });
    if let Some(request) = agy_prompt().as_ref().and_then(HookRequest::from_prompt) {
        loaded.requests.insert(AGY_ROW.to_owned(), request);
    }
    let mut model = NotchModel::default();
    model.now_ms = now;
    model.update(loaded.agents, loaded.snapshots, loaded.requests);
    model.go(NotchState::Peek(AGY_ROW.to_owned()));
    Scene {
        model,
        acting: Acting::On,
        pet: None,
        motion: Default::default(),
        perch: None,
        window: None,
        question: None,
        picks: Vec::new(),
        tab: 0,
    }
}

/// The picker over the chat fixture's models cache, with the last turn on the second model.
pub fn codex_picker() -> CodexPicker {
    let root = ChatRoot::for_render(None);
    let facts = SessionFacts {
        model: Some("gpt-6.1-mini".to_owned()),
        effort: Some("low".to_owned()),
        ..SessionFacts::default()
    };
    CodexPicker::new(models::load(&root.codex_dir), Pick::default(), &facts)
}

/// The sheet over a made-up edit, both tools installed, Codex's models from the fixture.
pub fn second_opinion() -> SecondOpinionSheet {
    let line = |kind, text: &str| DiffLine {
        kind,
        text: text.to_owned(),
        number: None,
    };
    let subject = Subject::Change {
        path: "/Users/me/work/notchling/crates/app/src/notch/model.rs".to_owned(),
        diff: vec![
            line(DiffKind::Hunk, "@@ -210,4 +210,5 @@"),
            line(
                DiffKind::Context,
                "    pub fn peek(&mut self, id: &str, now_ms: i64) {",
            ),
            line(
                DiffKind::Remove,
                "        if self.state != NotchState::Collapsed {",
            ),
            line(
                DiffKind::Add,
                "        let peeking = matches!(self.state, NotchState::Peek(_));",
            ),
            line(
                DiffKind::Add,
                "        if !(self.state == NotchState::Collapsed || peeking) {",
            ),
            line(DiffKind::Context, "            return;"),
        ],
    };
    let mut sheet = SecondOpinionSheet::new(
        subject,
        "/Users/me/work/notchling",
        vec![OpinionTool::Codex, OpinionTool::Agy],
    );
    let root = ChatRoot::for_render(None);
    sheet.codex_models = ModelOption::from_codex(&models::load(&root.codex_dir));
    sheet.agy_models = vec![ModelOption {
        model: "gemini-3-pro".to_owned(),
        name: "gemini-3-pro".to_owned(),
        efforts: vec!["standard".to_owned(), "deep".to_owned()],
    }];
    sheet.pick_model(Some("gpt-6.1-sol"));
    sheet.pick_effort(Some("high"));
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notch::model::Card;

    #[test]
    fn every_view_has_its_data() {
        for spec in VIEWS {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
        }
        assert_eq!(shown("notch-list"), None);
        let scene = agy_permission_scene();
        assert_eq!(scene.model.state, NotchState::Peek(AGY_ROW.to_owned()));
        let Some(Card::Permission(r)) = scene.model.card(AGY_ROW) else {
            panic!("no permission card");
        };
        let agy = r.agy.expect("Antigravity's own options");
        assert_eq!(agy.options.len(), 3);
        assert_eq!(scene.acting, Acting::On);
        let row = scene.model.agent(AGY_ROW).unwrap();
        assert_eq!(
            row.group,
            AgentGroup::NeedsInput,
            "a held prompt makes it wait on you"
        );
        let size = scene.model.size(&scene.model.state);
        assert!(size.h <= f64::from(VIEWS[0].size.1), "{size:?}");

        let picker = codex_picker();
        assert_eq!(picker.models.len(), 2, "the hidden model is left out");
        assert_eq!(picker.label(), "Codex · GPT-6.1 Mini · low");

        let sheet = second_opinion();
        assert_eq!(sheet.model.as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(sheet.efforts(), ["low", "medium", "high", "xhigh"]);
        assert!(sheet.prompt.contains("```diff"));
    }
}
