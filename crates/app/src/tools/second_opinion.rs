//! The Second Opinion sheet, pure (`SecondOpinionForm`, `SecondOpinionPrompt`): ask Codex or
//! Antigravity CLI to review an edit or a reply. The prompt is built from what is being reviewed,
//! the tool, model and effort are picked, and Run hands an [`OpinionRequest`] to an
//! [`OpinionRunner`].
//!
//! The runner is a trait here because the engine's `SecondOpinion` and `Delegate` (Task 5, on
//! `m5-delegate`) aren't merged yet: the sheet is complete against this trait, and nothing
//! implements it in the app until then.
//!
//! TODO(m5-delegate): implement [`OpinionRunner`] over `notchling delegate <tool> --model --effort
//! --folder --timeout --json -- <prompt>` (the arguments are [`arguments`]), reading its `--json`
//! answer with [`OpinionResult::parse`], and keep the runs in a store like `SecondOpinionStore`.

use notchling_engine::tools::codex::models::Model;
use notchling_engine::transcript::model::{DiffKind, DiffLine};
use serde_json::Value;

use crate::acting::{Acting, Control};

/// What goes into the prompt, so a pasted log or a huge diff can't make it unreasonably long.
pub const BODY_LIMIT: usize = 24_000;
/// Why Run is off while nothing runs opinions yet.
pub const NO_RUNNER: &str = "Second opinions run through notchling delegate, which isn't wired yet";
pub const NO_TOOLS: &str = "Neither Codex nor Antigravity CLI is installed.";
pub const READ_ONLY_NOTE: &str = "Read-only. It can look at the folder but not change it.";
/// How long a review may run, in seconds (`SecondOpinion.arguments`' default).
pub const TIMEOUT_SECS: u64 = 900;

/// What is being reviewed.
#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    /// An Edit or Write step: the file and its diff.
    Change { path: String, diff: Vec<DiffLine> },
    /// An assistant message.
    Message(String),
}

impl Subject {
    /// A short name for the card: the file's name, or "Reply".
    pub fn title(&self) -> String {
        match self {
            Subject::Change { path, .. } => {
                let name = path.rsplit('/').next().unwrap_or("");
                if name.is_empty() {
                    "Change".to_owned()
                } else {
                    name.to_owned()
                }
            }
            Subject::Message(_) => "Reply".to_owned(),
        }
    }
}

/// The diff as unified text: `+`, `-` or a space in front of each line, hunk headers as they are.
pub fn unified(lines: &[DiffLine]) -> String {
    lines
        .iter()
        .map(|l| match l.kind {
            DiffKind::Add => format!("+{}", l.text),
            DiffKind::Remove => format!("-{}", l.text),
            DiffKind::Context => format!(" {}", l.text),
            DiffKind::Hunk => l.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// At most [`BODY_LIMIT`] characters, saying how many were left out.
pub fn clip(text: &str) -> String {
    let count = text.chars().count();
    if count <= BODY_LIMIT {
        return text.to_owned();
    }
    let kept: String = text.chars().take(BODY_LIMIT).collect();
    format!(
        "{kept}\n… ({} more characters left out)",
        count - BODY_LIMIT
    )
}

/// The prompt for a subject (`SecondOpinionPrompt.build`).
pub fn build_prompt(subject: &Subject, folder: &str) -> String {
    match subject {
        Subject::Change { path, diff } => {
            let mut lines = vec![
                "Review this change for bugs, regressions and missed edge cases. Be specific and concise; if it looks right, say so.".to_owned(),
                String::new(),
                format!("Folder: {folder}"),
            ];
            if !path.is_empty() {
                lines.push(format!("File: {path}"));
            }
            lines.extend([
                String::new(),
                "```diff".to_owned(),
                clip(&unified(diff)),
                "```".to_owned(),
            ]);
            lines.join("\n")
        }
        Subject::Message(text) => [
            "Review this answer from another AI agent for mistakes, gaps and claims that aren't supported. Be specific and concise; check against the files if it refers to them.",
            "",
            &format!("Folder: {folder}"),
            "",
            "Message:",
            "",
            &clip(text.trim()),
        ]
        .join("\n"),
    }
}

/// The tool asked.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpinionTool {
    Codex,
    Agy,
}

impl OpinionTool {
    pub fn name(self) -> &'static str {
        match self {
            OpinionTool::Codex => "Codex",
            OpinionTool::Agy => "Antigravity",
        }
    }

    /// `notchling delegate`'s word for it.
    pub fn arg(self) -> &'static str {
        match self {
            OpinionTool::Codex => "codex",
            OpinionTool::Agy => "agy",
        }
    }
}

/// A model a tool offers, for the pickers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelOption {
    pub model: String,
    pub name: String,
    pub efforts: Vec<String>,
}

impl ModelOption {
    /// Codex's cached list as options.
    pub fn from_codex(models: &[Model]) -> Vec<ModelOption> {
        models
            .iter()
            .map(|m| ModelOption {
                model: m.slug.clone(),
                name: m.name.clone(),
                efforts: m.efforts.clone(),
            })
            .collect()
    }
}

/// What Run asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OpinionRequest {
    pub tool: OpinionTool,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub folder: String,
    pub subject: String,
    pub prompt: String,
}

/// Runs opinions. Nothing in the app implements it yet (see the module doc).
pub trait OpinionRunner: Send + Sync {
    /// Starts a run; the id to cancel it by.
    fn start(&self, request: &OpinionRequest) -> Result<String, String>;
    fn cancel(&self, id: &str);
}

/// The arguments for `notchling delegate`: read-only (no `--write`), with the prompt last after
/// `--`.
pub fn arguments(request: &OpinionRequest, timeout_secs: u64) -> Vec<String> {
    let mut args = vec!["delegate".to_owned(), request.tool.arg().to_owned()];
    if let Some(model) = request.model.as_deref().filter(|m| !m.is_empty()) {
        args.extend(["--model".to_owned(), model.to_owned()]);
    }
    if let Some(effort) = request.effort.as_deref().filter(|e| !e.is_empty()) {
        args.extend(["--effort".to_owned(), effort.to_owned()]);
    }
    args.extend([
        "--folder".to_owned(),
        request.folder.clone(),
        "--timeout".to_owned(),
        timeout_secs.to_string(),
        "--json".to_owned(),
        "--".to_owned(),
        request.prompt.clone(),
    ]);
    args
}

/// What `notchling delegate … --json` prints, read back (`SecondOpinionResult`).
#[derive(Clone, Debug, PartialEq)]
pub struct OpinionResult {
    pub ok: bool,
    pub reply: String,
    pub session: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub error: Option<String>,
    pub seconds: f64,
    pub tokens: Option<u64>,
    pub timed_out: bool,
}

impl OpinionResult {
    /// The last JSON object in `output` with a `tool` field (the command prints one line; anything
    /// before it is noise).
    pub fn parse(output: &str) -> Option<OpinionResult> {
        output.lines().rev().find_map(|line| {
            let text = line.trim();
            if !text.starts_with('{') {
                return None;
            }
            let d: Value = serde_json::from_str(text).ok()?;
            d.get("tool")?;
            let string = |key: &str| d.get(key).and_then(Value::as_str).map(str::to_owned);
            Some(OpinionResult {
                ok: d.get("ok").and_then(Value::as_bool).unwrap_or(false),
                reply: string("reply").unwrap_or_default(),
                session: string("session"),
                model: string("model"),
                effort: string("effort"),
                error: string("error"),
                seconds: d.get("seconds").and_then(Value::as_f64).unwrap_or(0.0),
                tokens: d.get("tokens").and_then(Value::as_u64),
                timed_out: d.get("timedOut").and_then(Value::as_bool).unwrap_or(false),
            })
        })
    }
}

/// Where the sheet is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Editing,
    /// Started; the id to cancel by.
    Running(String),
    Failed(String),
}

/// The sheet's state.
#[derive(Clone, Debug, PartialEq)]
pub struct SecondOpinionSheet {
    pub subject: Subject,
    pub folder: String,
    /// The tools installed, in order; empty shows [`NO_TOOLS`].
    pub tools: Vec<OpinionTool>,
    pub tool: OpinionTool,
    pub codex_models: Vec<ModelOption>,
    pub agy_models: Vec<ModelOption>,
    /// `None` is the tool's default (the flag is left off).
    pub model: Option<String>,
    pub effort: Option<String>,
    pub prompt: String,
    pub stage: Stage,
}

impl SecondOpinionSheet {
    pub fn new(subject: Subject, folder: &str, tools: Vec<OpinionTool>) -> SecondOpinionSheet {
        SecondOpinionSheet {
            prompt: build_prompt(&subject, folder),
            subject,
            folder: folder.to_owned(),
            tool: tools.first().copied().unwrap_or(OpinionTool::Codex),
            tools,
            codex_models: Vec::new(),
            agy_models: Vec::new(),
            model: None,
            effort: None,
            stage: Stage::Editing,
        }
    }

    /// The picked tool's models.
    pub fn models(&self) -> &[ModelOption] {
        match self.tool {
            OpinionTool::Codex => &self.codex_models,
            OpinionTool::Agy => &self.agy_models,
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
    pub fn pick_tool(&mut self, tool: OpinionTool) {
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

    /// Run's gate: acting on, a runner to hand it to, a tool installed, a prompt and nothing
    /// running.
    pub fn run_control(&self, acting: Acting, has_runner: bool) -> Control {
        let gate = acting.control();
        if !gate.enabled {
            return gate;
        }
        if !has_runner {
            return Control {
                enabled: false,
                tooltip: Some(NO_RUNNER),
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

    /// What Run sends.
    pub fn request(&self) -> OpinionRequest {
        OpinionRequest {
            tool: self.tool,
            model: self.model.clone().filter(|m| !m.is_empty()),
            effort: self.effort.clone().filter(|e| !e.is_empty()),
            folder: self.folder.clone(),
            subject: self.subject.title(),
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
                self.tool.name(),
                self.subject.title()
            ),
            Stage::Editing => READ_ONLY_NOTE.to_owned(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diff() -> Vec<DiffLine> {
        let line = |kind, text: &str| DiffLine {
            kind,
            text: text.to_owned(),
            number: None,
        };
        vec![
            line(DiffKind::Hunk, "@@ -1,2 +1,2 @@"),
            line(DiffKind::Context, "fn main() {"),
            line(DiffKind::Remove, "    old();"),
            line(DiffKind::Add, "    new();"),
        ]
    }

    #[test]
    fn the_prompt_carries_the_folder_the_file_and_a_unified_diff() {
        let subject = Subject::Change {
            path: "/w/app/src/main.rs".into(),
            diff: diff(),
        };
        let prompt = build_prompt(&subject, "/w/app");
        assert!(prompt.starts_with("Review this change"));
        assert!(prompt.contains("Folder: /w/app\nFile: /w/app/src/main.rs\n"));
        assert!(prompt
            .contains("```diff\n@@ -1,2 +1,2 @@\n fn main() {\n-    old();\n+    new();\n```"));
        assert_eq!(subject.title(), "main.rs");
        assert_eq!(
            Subject::Change {
                path: String::new(),
                diff: vec![]
            }
            .title(),
            "Change"
        );
        let message = build_prompt(&Subject::Message("  It works.  ".into()), "/w");
        assert!(message.ends_with("Message:\n\nIt works."));
        assert_eq!(Subject::Message(String::new()).title(), "Reply");
        let long = "x".repeat(BODY_LIMIT + 5);
        let clipped = clip(&long);
        assert!(clipped.ends_with("… (5 more characters left out)"));
        assert_eq!(clip("short"), "short");
    }

    fn sheet() -> SecondOpinionSheet {
        let mut s = SecondOpinionSheet::new(
            Subject::Message("hi".into()),
            "/w",
            vec![OpinionTool::Codex, OpinionTool::Agy],
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
    fn picks_follow_the_tool_and_the_model() {
        let mut s = sheet();
        assert_eq!(s.tool, OpinionTool::Codex);
        assert_eq!(s.efforts(), ["low", "high", "medium"], "every effort, once");
        s.pick_model(Some("sol"));
        s.pick_effort(Some("high"));
        assert_eq!(s.efforts(), ["low", "high"]);
        s.pick_model(Some("mini"));
        assert_eq!(s.effort, None, "mini has no high");
        s.pick_effort(Some("medium"));
        s.pick_tool(OpinionTool::Agy);
        assert_eq!((s.model.as_deref(), s.effort.as_deref()), (None, None));
        assert_eq!(s.efforts(), ["standard"]);
        s.pick_tool(OpinionTool::Agy);
        let request = s.request();
        assert_eq!(request.tool, OpinionTool::Agy);
        assert_eq!(request.subject, "Reply");
        assert_eq!(
            arguments(&request, 900),
            [
                "delegate",
                "agy",
                "--folder",
                "/w",
                "--timeout",
                "900",
                "--json",
                "--",
                request.prompt.as_str()
            ]
        );
        s.pick_model(Some("g3"));
        s.pick_effort(Some("standard"));
        let args = arguments(&s.request(), 60);
        assert_eq!(&args[2..6], ["--model", "g3", "--effort", "standard"]);
    }

    #[test]
    fn run_is_gated_by_acting_then_the_runner_then_the_sheet() {
        let mut s = sheet();
        assert_eq!(
            s.run_control(Acting::Off, true).tooltip,
            Some(crate::acting::OFF_MESSAGE)
        );
        assert_eq!(s.run_control(Acting::On, false).tooltip, Some(NO_RUNNER));
        assert!(s.run_control(Acting::On, true).enabled);
        s.prompt = "  ".into();
        assert!(!s.run_control(Acting::On, true).enabled);
        s.prompt = "go".into();
        s.started(Ok("run-1".into()));
        assert!(!s.run_control(Acting::On, true).enabled);
        assert_eq!(s.footer(), "Codex is reviewing Reply…");
        s.started(Err("no codex".into()));
        assert_eq!(s.footer(), "no codex");
        let none = SecondOpinionSheet::new(Subject::Message("x".into()), "/w", vec![]);
        assert!(!none.run_control(Acting::On, true).enabled);
        assert_eq!(none.footer(), READ_ONLY_NOTE);
    }

    #[test]
    fn the_delegate_answer_is_the_last_json_line_with_a_tool() {
        let output = "warming up\n{\"not\": \"this\"}\n{\"tool\":\"codex\",\"ok\":true,\"reply\":\"Looks right.\",\"session\":\"s9\",\"seconds\":12.5,\"tokens\":800}\n";
        let r = OpinionResult::parse(output).unwrap();
        assert!(r.ok && !r.timed_out);
        assert_eq!(
            (r.reply.as_str(), r.session.as_deref()),
            ("Looks right.", Some("s9"))
        );
        assert_eq!((r.seconds, r.tokens), (12.5, Some(800)));
        assert_eq!(OpinionResult::parse("nothing"), None);
        let options = ModelOption::from_codex(&[Model {
            slug: "sol".into(),
            name: "Sol".into(),
            summary: String::new(),
            efforts: vec!["low".into()],
            default_effort: None,
        }]);
        assert_eq!(options[0].name, "Sol");
    }
}
