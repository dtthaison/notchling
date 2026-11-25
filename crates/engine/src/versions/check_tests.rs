use super::check::{commands, judge, lists_flag, CheckPart, CheckResult, Output};
use super::{Tool, Version};
use crate::tools::codex::plan::{arguments, Pick, FLAGS_USED, WORKSPACE_WRITE};
use crate::transcript::strict::{SkipKind, Skips};
use std::path::Path;

fn v(s: &str) -> Version {
    Version::parse(s).unwrap()
}

const HELP: &str = "Usage: codex exec [OPTIONS]\n\n  -c, --config <key=value>  Override\n  -m, --model <MODEL>       Model\n  -C, --cd <DIR>            Folder\n      --skip-git-repo-check Skip\n      --json                Print events\n";

const DOCTOR: &str = "Diagnostics\n └ Auto-update: enabled\n";

fn claude_outputs() -> Vec<Output> {
    vec![
        (
            CheckPart::Version,
            Some((0, "2.1.324 (Claude Code)\n".to_string())),
        ),
        (CheckPart::Doctor, Some((0, DOCTOR.to_string()))),
    ]
}

fn codex_outputs(help: &str) -> Vec<Output> {
    vec![
        (
            CheckPart::Version,
            Some((0, "codex-cli 0.166.0\n".to_string())),
        ),
        (CheckPart::ExecFlags, Some((0, help.to_string()))),
    ]
}

#[test]
fn all_parts_passing_is_partly_checked_never_passed() {
    let a = judge(
        Tool::Claude,
        &v("2.1.324"),
        &claude_outputs(),
        &Skips::default(),
    );
    assert_eq!(a.result, CheckResult::PartlyChecked);
    assert_eq!(a.failed_part, None);
    assert_eq!(a.unchecked, vec!["screens", "dialogs", "turns"]);
    let b = judge(
        Tool::Codex,
        &v("0.166.0"),
        &codex_outputs(HELP),
        &Skips::default(),
    );
    assert_eq!(b.result, CheckResult::PartlyChecked);
    let c = judge(
        Tool::Notchling,
        &v("1.0.0"),
        &[(CheckPart::SelfTest, Some((0, String::new())))],
        &Skips::default(),
    );
    assert_eq!(c.result, CheckResult::PartlyChecked);
}

#[test]
fn version_text_mismatch_fails_version() {
    let out = judge(
        Tool::Claude,
        &v("2.1.325"),
        &claude_outputs(),
        &Skips::default(),
    );
    assert_eq!(out.result, CheckResult::Failed);
    assert_eq!(out.failed_part, Some(CheckPart::Version));
}

#[test]
fn doctor_nonzero_fails_doctor() {
    let mut outputs = claude_outputs();
    outputs[1] = (CheckPart::Doctor, Some((1, DOCTOR.to_string())));
    let out = judge(Tool::Claude, &v("2.1.324"), &outputs, &Skips::default());
    assert_eq!(out.failed_part, Some(CheckPart::Doctor));
    outputs[1] = (CheckPart::Doctor, Some((0, "nothing useful\n".to_string())));
    let out = judge(Tool::Claude, &v("2.1.324"), &outputs, &Skips::default());
    assert_eq!(out.failed_part, Some(CheckPart::Doctor));
}

#[test]
fn help_missing_json_fails_exec_flags() {
    let help = HELP.replace("--json", "--jsonl");
    let out = judge(
        Tool::Codex,
        &v("0.166.0"),
        &codex_outputs(&help),
        &Skips::default(),
    );
    assert_eq!(out.result, CheckResult::Failed);
    assert_eq!(out.failed_part, Some(CheckPart::ExecFlags));
    assert!(out.why.unwrap().contains("--json"));
}

#[test]
fn unknown_block_in_new_version_fails_transcript() {
    let mut skips = Skips::default();
    skips.note(Some("2.1.324"), SkipKind::UnknownBlock);
    let out = judge(Tool::Claude, &v("2.1.324"), &claude_outputs(), &skips);
    assert_eq!(out.result, CheckResult::Failed);
    assert_eq!(out.failed_part, Some(CheckPart::Transcript));
    assert!(out.why.unwrap().contains("unknown block"));
    assert_eq!(out.unknown_count, 1);
}

#[test]
fn unknown_block_in_old_version_only_counts() {
    let mut skips = Skips::default();
    skips.note(Some("2.1.323"), SkipKind::UnknownBlock);
    skips.note(Some("2.1.323"), SkipKind::UnknownLine);
    let out = judge(Tool::Claude, &v("2.1.324"), &claude_outputs(), &skips);
    assert_eq!(out.result, CheckResult::PartlyChecked);
    assert_eq!(out.unknown_count, 2);
}

#[test]
fn timeout_is_cant_tell() {
    let mut outputs = claude_outputs();
    outputs[1] = (CheckPart::Doctor, None);
    let out = judge(Tool::Claude, &v("2.1.324"), &outputs, &Skips::default());
    assert_eq!(out.result, CheckResult::CantTell);
    assert_eq!(out.failed_part, None);
}

#[test]
fn a_definite_failure_wins_over_a_timeout() {
    let mut outputs = claude_outputs();
    outputs[0] = (CheckPart::Version, Some((0, "9.9.9 (Claude Code)".into())));
    outputs[1] = (CheckPart::Doctor, None);
    let out = judge(Tool::Claude, &v("2.1.324"), &outputs, &Skips::default());
    assert_eq!(out.result, CheckResult::Failed);
}

#[test]
fn flags_used_covers_every_turn_kind() {
    let picks = [
        Pick::default(),
        Pick {
            model: Some("gpt-x".into()),
            effort: Some("high".into()),
        },
    ];
    for pick in &picks {
        for session in [None, Some("abc")] {
            for network in [false, true] {
                for prompt in ["hi", "-dash"] {
                    let args = arguments(prompt, session, "/p", pick, WORKSPACE_WRITE, network);
                    for arg in args.iter().skip(1) {
                        if arg == prompt || arg == "--" || !arg.starts_with('-') {
                            continue;
                        }
                        assert!(FLAGS_USED.contains(&arg.as_str()), "{arg}");
                    }
                }
            }
        }
    }
}

#[test]
fn outcome_why_never_quotes_output() {
    let marker = "PLANTED-MARKER-OUTPUT";
    let outputs = vec![
        (
            CheckPart::Version,
            Some((0, format!("{marker}\n2.1.324 (Claude Code)"))),
        ),
        (CheckPart::Doctor, Some((2, marker.to_string()))),
    ];
    let out = judge(Tool::Claude, &v("2.1.324"), &outputs, &Skips::default());
    let text = format!("{out:?}");
    assert!(!text.contains(marker), "{text}");
    let out = judge(
        Tool::Codex,
        &v("0.166.0"),
        &codex_outputs(marker),
        &Skips::default(),
    );
    assert!(!format!("{out:?}").contains(marker));
}

#[test]
fn commands_are_one_shot_per_tool() {
    let bin = Path::new("/x/claude");
    let argvs =
        |tool| -> Vec<Vec<String>> { commands(tool, bin).into_iter().map(|c| c.argv).collect() };
    assert_eq!(
        argvs(Tool::Claude),
        vec![
            vec!["/x/claude".to_string(), "--version".to_string()],
            vec!["/x/claude".to_string(), "doctor".to_string()],
        ]
    );
    assert_eq!(argvs(Tool::Codex)[1], vec!["/x/claude", "exec", "--help"]);
    assert!(commands(Tool::Notchling, bin).is_empty());
    assert_eq!(commands(Tool::Agy, bin).len(), 1);
}

#[test]
fn flags_are_matched_as_whole_tokens() {
    assert!(lists_flag("-c, --config", "-c"));
    assert!(!lists_flag("--cd <DIR>", "-c"));
    assert!(!lists_flag("--jsonl", "--json"));
}
