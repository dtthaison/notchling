use super::{cli_output, Tool, Version};
use crate::tools::codex::plan::FLAGS_USED;
use crate::transcript::strict::{SkipKind, Skips};
use serde::{Deserialize, Serialize};
use std::path::Path;

pub const UNCHECKED: [&str; 3] = ["screens", "dialogs", "turns"];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckPart {
    Version,
    Doctor,
    ExecFlags,
    Transcript,
    SelfTest,
}

impl CheckPart {
    pub fn word(self) -> &'static str {
        match self {
            CheckPart::Version => "version",
            CheckPart::Doctor => "doctor",
            CheckPart::ExecFlags => "exec_flags",
            CheckPart::Transcript => "transcript",
            CheckPart::SelfTest => "self_test",
        }
    }

    pub fn from_word(word: &str) -> Option<CheckPart> {
        [
            CheckPart::Version,
            CheckPart::Doctor,
            CheckPart::ExecFlags,
            CheckPart::Transcript,
            CheckPart::SelfTest,
        ]
        .into_iter()
        .find(|p| p.word() == word)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckTier {
    FilesFlags,
}

impl CheckTier {
    pub fn word(self) -> &'static str {
        match self {
            CheckTier::FilesFlags => "files_flags",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Command {
    pub part: CheckPart,
    pub argv: Vec<String>,
    pub timeout_secs: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckResult {
    #[serde(rename = "passed")]
    Passed,
    #[serde(rename = "partly")]
    PartlyChecked,
    #[serde(rename = "failed")]
    Failed,
    #[serde(rename = "cant_tell")]
    CantTell,
}

impl CheckResult {
    pub fn word(self) -> &'static str {
        match self {
            CheckResult::Passed => "passed",
            CheckResult::PartlyChecked => "partly",
            CheckResult::Failed => "failed",
            CheckResult::CantTell => "cant_tell",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    pub result: CheckResult,
    pub failed_part: Option<CheckPart>,
    pub why: Option<String>,
    pub unknown_count: u32,
    pub unchecked: Vec<&'static str>,
    pub duration_ms: u64,
}

pub type Output = (CheckPart, Option<(i32, String)>);

pub fn commands(tool: Tool, binary: &Path) -> Vec<Command> {
    let bin = binary.to_string_lossy().into_owned();
    let make = |part: CheckPart, rest: &[&str], timeout_secs: u64| {
        let mut argv = vec![bin.clone()];
        argv.extend(rest.iter().map(|s| s.to_string()));
        Command {
            part,
            argv,
            timeout_secs,
        }
    };
    match tool {
        Tool::Notchling => Vec::new(),
        Tool::Claude => vec![
            make(CheckPart::Version, &["--version"], 10),
            make(CheckPart::Doctor, &["doctor"], 30),
        ],
        Tool::Codex => vec![
            make(CheckPart::Version, &["--version"], 10),
            make(CheckPart::ExecFlags, &["exec", "--help"], 10),
        ],
        Tool::Agy => vec![make(CheckPart::Version, &["--version"], 10)],
    }
}

pub(super) fn lists_flag(help: &str, flag: &str) -> bool {
    help.split(|c: char| c.is_whitespace() || c == ',' || c == '=' || c == '<' || c == '[')
        .any(|token| token == flag)
}

fn has_auto_update_line(text: &str) -> bool {
    text.lines().any(|line| {
        let lead = line.trim_start_matches(|c: char| !c.is_ascii_alphanumeric());
        let lower = lead.to_ascii_lowercase();
        lower.starts_with("auto-update")
            || lower.starts_with("auto update")
            || lower.starts_with("autoupdate")
    })
}

fn kind_words(kind: SkipKind) -> &'static str {
    match kind {
        SkipKind::UnknownLine => "an unknown line type",
        SkipKind::UnknownBlock => "an unknown block type",
        SkipKind::UnknownQueueOp => "an unknown queue operation",
        SkipKind::ParseFailed => "a line that doesn't parse",
        SkipKind::UnknownAcpUpdate => "an unknown update",
        SkipKind::UnknownCodexEvent => "an unknown event",
    }
}

enum Verdict {
    Ok,
    Fail(String),
    CantTell,
}

fn find(outputs: &[Output], part: CheckPart) -> Option<&Option<(i32, String)>> {
    outputs.iter().find(|(p, _)| *p == part).map(|(_, o)| o)
}

fn judge_version(tool: Tool, installed: &Version, outputs: &[Output]) -> Verdict {
    let Some(output) = find(outputs, CheckPart::Version) else {
        return Verdict::Ok;
    };
    let Some((code, text)) = output else {
        return Verdict::CantTell;
    };
    let said = cli_output(tool, text);
    if *code != 0 || said.as_ref() != Some(installed) {
        return Verdict::Fail(format!(
            "`{} --version` doesn't say {}",
            tool.command(),
            installed
        ));
    }
    Verdict::Ok
}

fn judge_doctor(outputs: &[Output]) -> Verdict {
    let Some(output) = find(outputs, CheckPart::Doctor) else {
        return Verdict::Ok;
    };
    let Some((code, text)) = output else {
        return Verdict::CantTell;
    };
    if *code != 0 {
        return Verdict::Fail("`claude doctor` exits with an error".to_string());
    }
    if !has_auto_update_line(text) {
        return Verdict::Fail("`claude doctor` no longer prints its auto-update line".to_string());
    }
    Verdict::Ok
}

fn judge_flags(outputs: &[Output]) -> Verdict {
    let Some(output) = find(outputs, CheckPart::ExecFlags) else {
        return Verdict::Ok;
    };
    let Some((code, text)) = output else {
        return Verdict::CantTell;
    };
    if *code != 0 {
        return Verdict::Fail("`codex exec --help` exits with an error".to_string());
    }
    match FLAGS_USED.iter().find(|flag| !lists_flag(text, flag)) {
        Some(flag) => Verdict::Fail(format!("`codex exec` no longer takes `{flag}`")),
        None => Verdict::Ok,
    }
}

fn judge_self_test(outputs: &[Output]) -> Verdict {
    match find(outputs, CheckPart::SelfTest) {
        None => Verdict::Ok,
        Some(None) => Verdict::CantTell,
        Some(Some((0, _))) => Verdict::Ok,
        Some(Some(_)) => Verdict::Fail("Notchling's own check failed".to_string()),
    }
}

fn judge_transcript(installed: &Version, skips: &Skips) -> Verdict {
    match skips.first_kind_in(installed.as_str()) {
        Some(kind) => Verdict::Fail(format!(
            "Transcripts written by {installed} hold {}",
            kind_words(kind)
        )),
        None => Verdict::Ok,
    }
}

pub fn judge(tool: Tool, installed: &Version, outputs: &[Output], skips: &Skips) -> Outcome {
    let steps = [
        (CheckPart::Version, judge_version(tool, installed, outputs)),
        (CheckPart::Doctor, judge_doctor(outputs)),
        (CheckPart::ExecFlags, judge_flags(outputs)),
        (CheckPart::Transcript, judge_transcript(installed, skips)),
        (CheckPart::SelfTest, judge_self_test(outputs)),
    ];
    let mut cant_tell = false;
    let mut failure: Option<(CheckPart, String)> = None;
    for (part, verdict) in steps {
        match verdict {
            Verdict::Ok => {}
            Verdict::CantTell => cant_tell = true,
            Verdict::Fail(why) => {
                if failure.is_none() {
                    failure = Some((part, why));
                }
            }
        }
    }
    let unknown_count = skips.unknown_total();
    let (result, failed_part, why) = match failure {
        Some((part, why)) => (CheckResult::Failed, Some(part), Some(why)),
        None if cant_tell => (CheckResult::CantTell, None, None),
        None => (CheckResult::PartlyChecked, None, None),
    };
    Outcome {
        result,
        failed_part,
        why,
        unknown_count,
        unchecked: UNCHECKED.to_vec(),
        duration_ms: 0,
    }
}
