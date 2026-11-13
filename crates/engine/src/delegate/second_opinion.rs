use super::agy_pick;
use super::options::Tool;
use crate::tools::codex::models::Model;
use crate::transcript::model::{DiffKind, DiffLine};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub enum Subject {
    Change { path: String, diff: Vec<DiffLine> },
    Message(String),
}

pub const BODY_LIMIT: usize = 24_000;

pub fn build_prompt(subject: &Subject, folder: &str) -> String {
    match subject {
        Subject::Change { path, diff } => {
            let mut lines = vec![
                "Review this change for bugs, regressions and missed edge cases. Be specific and concise; if it looks right, say so.".to_string(),
                String::new(),
                format!("Folder: {folder}"),
            ];
            if !path.is_empty() {
                lines.push(format!("File: {path}"));
            }
            lines.extend([
                String::new(),
                "```diff".into(),
                clip(&unified(diff)),
                "```".into(),
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

pub fn unified(lines: &[DiffLine]) -> String {
    lines
        .iter()
        .map(|line| match line.kind {
            DiffKind::Add => format!("+{}", line.text),
            DiffKind::Remove => format!("-{}", line.text),
            DiffKind::Context => format!(" {}", line.text),
            DiffKind::Hunk => line.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn clip(text: &str) -> String {
    let count = text.chars().count();
    if count <= BODY_LIMIT {
        return text.to_string();
    }
    let kept: String = text.chars().take(BODY_LIMIT).collect();
    format!(
        "{kept}\n… ({} more characters left out)",
        count - BODY_LIMIT
    )
}

pub fn title(subject: &Subject) -> String {
    match subject {
        Subject::Change { path, .. } => {
            let name = path.rsplit('/').next().unwrap_or("");
            if name.is_empty() {
                "Change".into()
            } else {
                name.into()
            }
        }
        Subject::Message(_) => "Reply".into(),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Answer {
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

impl Answer {
    pub fn parse(output: &str) -> Option<Answer> {
        output.lines().rev().find_map(|line| {
            let text = line.trim();
            if !text.starts_with('{') {
                return None;
            }
            let d: Value = serde_json::from_str(text).ok()?;
            d.get("tool")?;
            let string = |k: &str| d.get(k).and_then(Value::as_str).map(String::from);
            Some(Answer {
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Running,
    Done,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecondOpinion {
    pub id: String,
    pub session: String,
    pub anchor: String,
    pub subject: String,
    pub tool: String,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub folder: String,
    pub started: f64,
    pub status: Status,
    pub reply: String,
    pub error: Option<String>,
    pub delegated_session: Option<String>,
    pub seconds: f64,
    pub tokens: Option<u64>,
}

pub const TIMEOUT_SECS: u64 = 900;

impl SecondOpinion {
    pub fn tool_value(&self) -> Tool {
        Tool::from_name(&self.tool).unwrap_or(Tool::Codex)
    }

    pub fn arguments(
        tool: Tool,
        model: Option<&str>,
        effort: Option<&str>,
        folder: &str,
        prompt_file: &str,
        timeout_secs: u64,
    ) -> Vec<String> {
        let mut args = vec!["delegate".to_string(), tool.raw().to_string()];
        for (flag, value) in [("--model", model), ("--effort", effort)] {
            if let Some(v) = value.filter(|v| !v.is_empty()) {
                args.extend([flag.to_string(), v.to_string()]);
            }
        }
        args.extend([
            "--folder".into(),
            folder.into(),
            "--timeout".into(),
            timeout_secs.to_string(),
            "--json".into(),
            "--prompt-file".into(),
            prompt_file.into(),
        ]);
        args
    }

    pub fn finish(&mut self, answer: Option<&Answer>, output: &str, cancelled: bool) {
        if cancelled {
            self.status = Status::Cancelled;
            if let Some(a) = answer {
                if !a.reply.is_empty() {
                    self.reply = a.reply.clone();
                }
            }
            self.delegated_session = answer
                .and_then(|a| a.session.clone())
                .or_else(|| self.delegated_session.take());
            return;
        }
        let Some(a) = answer else {
            self.status = Status::Failed;
            let tail = output.trim();
            self.error = Some(if tail.is_empty() {
                "The tool ended without an answer.".into()
            } else {
                let skip = tail.chars().count().saturating_sub(300);
                tail.chars().skip(skip).collect()
            });
            return;
        };
        self.reply = a.reply.clone();
        self.model = a.model.clone().or_else(|| self.model.take());
        self.effort = a.effort.clone().or_else(|| self.effort.take());
        self.delegated_session = a.session.clone();
        self.seconds = a.seconds;
        self.tokens = a.tokens;
        self.error = a.error.clone();
        self.status = if a.ok && !a.reply.is_empty() {
            Status::Done
        } else {
            Status::Failed
        };
        if a.ok && a.reply.is_empty() {
            self.error = Some("It sent no reply.".into());
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModelOption {
    pub model: String,
    pub name: String,
    pub efforts: Vec<String>,
}

pub fn codex_options(models: &[Model]) -> Vec<ModelOption> {
    models
        .iter()
        .map(|m| ModelOption {
            model: m.slug.clone(),
            name: m.name.clone(),
            efforts: m.efforts.clone(),
        })
        .collect()
}

pub fn agy_options(listing: &str) -> Vec<ModelOption> {
    agy_pick::models(listing)
        .into_iter()
        .map(|(model, efforts)| ModelOption {
            name: model.clone(),
            model,
            efforts,
        })
        .collect()
}

#[cfg(test)]
#[path = "second_opinion_tests.rs"]
mod tests;
