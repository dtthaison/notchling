//! The "sd worker setup --check" step (Swift `SDWorkerSteps` setup-check part).
//!
//! The check is read-only (sd 1.19.3: it reports missing labels and creates none). Its Fix, on a
//! click, runs `--create-labels`, the one GitHub write sd makes, and the engine checks again after
//! it. An sd older than [`MIN_SD`] runs `--check` and makes labels, so the step holds back on it
//! (see [`super::steps_sd::all`]).

use super::registry::SdWorkerSettings;
use super::shell;
use super::step::{run, CheckResult, FixContext, FixOutcome, FixPlan, Probe, RemoteOutput, SubRow};
use super::steps_sd::{cut_300, first_line_300, is_at_least, MIN_SD, SD_PLUGIN};
use crate::lanes::quote;
use serde_json::Value;

const ROW_CAP: usize = 200;
/// The Running row: the worker's own LaunchAgent. It is listed but never counted as a failure.
const NOT_COUNTED: &str = "com.sd.worker";

/// Stdout, then `@@exit N`, then stderr, so the parse tells the two apart whatever the engine does
/// with `code`. `python -B` keeps the interpreter from writing `.pyc` files next to sd.py: a check
/// changes nothing on the machine. A python or sd.py that isn't a safe absolute path never runs.
pub(super) fn setup_script(job: &SdWorkerSettings, flag: &str) -> String {
    if !shell::is_safe_machine_path(&job.python) || !shell::is_safe_machine_path(&job.sd_path) {
        return "echo; echo '@@exit 2'; echo 'python or sd.py is not an absolute path yet'".into();
    }
    format!(
        "e=$(mktemp); {} -B {} worker setup {flag} --json 2>\"$e\"; c=$?; echo; echo \"@@exit $c\"; head -c 4096 \"$e\"; rm -f \"$e\"",
        quote(&job.python),
        quote(&job.sd_path)
    )
}

pub(super) fn check_probe(job: &SdWorkerSettings) -> Probe {
    Probe::remote(setup_script(job, "--check"), |out, ctx| {
        setup_check_result(out, &|s| ctx.mask(s))
    })
}

fn create_labels_display(job: &SdWorkerSettings) -> String {
    if !shell::is_safe_machine_path(&job.python) || !shell::is_safe_machine_path(&job.sd_path) {
        return "python or sd.py is not an absolute path yet; nothing can run".into();
    }
    format!(
        "{} -B {} worker setup --create-labels --json\nThis makes the issue labels that are missing on GitHub, with the issues token: the one GitHub write sd makes. The check runs again after it.",
        quote(&job.python),
        quote(&job.sd_path)
    )
}

pub(super) fn create_labels_plan(job: &SdWorkerSettings) -> FixPlan {
    let shown = create_labels_display(job);
    let job = job.clone();
    FixPlan::new(shown, false, move |ctx| create_labels(&job, ctx))
}

/// The click-run Fix of the setup check. It runs `--create-labels` only; the engine checks again
/// after it.
fn create_labels(job: &SdWorkerSettings, ctx: &FixContext) -> FixOutcome {
    // the sd on the machine is the one copied from this Mac (Plugins installed compares them), so
    // this Mac's decides
    let Some(sd) = ctx.step.mac.plugins.get(SD_PLUGIN) else {
        return FixOutcome::fail(format!(
            "this Mac has no sd plugin; install sd {MIN_SD} or newer first"
        ));
    };
    if !is_at_least(&sd.version, MIN_SD) {
        return FixOutcome::fail(format!(
            "sd on this Mac is older than {MIN_SD}; update it first"
        ));
    }
    if !shell::is_safe_machine_path(&job.python) || !shell::is_safe_machine_path(&job.sd_path) {
        return FixOutcome::fail("python or sd.py is not an absolute path yet");
    }
    let mask = |s: &str| ctx.mask(s);
    let ran = run::script(ctx, &setup_script(job, "--create-labels"), None, 600);
    if ran.code != 0 {
        let why = first_line_300(
            if ran.err.is_empty() {
                &ran.out
            } else {
                &ran.err
            },
            &mask,
        );
        return FixOutcome::fail(if why.is_empty() {
            format!("the label step exited {}", ran.code)
        } else {
            why
        });
    }
    let result = setup_check_result(
        &RemoteOutput {
            text: ran.out,
            code: 0,
        },
        &mask,
    );
    let labels: Vec<&SubRow> = result
        .rows
        .iter()
        .filter(|r| r.name.starts_with("labels"))
        .collect();
    if let Some(bad) = labels.iter().find(|r| !r.ok) {
        return FixOutcome::fail(format!("{}: {}", bad.name, bad.detail));
    }
    if labels.is_empty() {
        // no label row: the command itself failed, and its state holds the reason
        if result.state.is_fail() {
            return FixOutcome::fail(result.state.text());
        }
        return FixOutcome::fail("setup printed no label row");
    }
    let done = labels
        .iter()
        .map(|r| r.detail.as_str())
        .collect::<Vec<_>>()
        .join("; ");
    if result.state.is_fail() {
        return FixOutcome::fail(format!(
            "The labels are in place ({done}). Fix what the other failing rows name, then Check again."
        ));
    }
    FixOutcome::ok(format!("The labels are in place ({done})"))
}

/// Reads the setup command's output: rows as JSON, then `@@exit N`, then stderr. Every text from
/// the machine is masked, then cut.
pub(super) fn setup_check_result(out: &RemoteOutput, mask: &dyn Fn(&str) -> String) -> CheckResult {
    let text = &out.text;
    let Some(marker) = text.rfind("@@exit ") else {
        return CheckResult::fail("setup --check printed no result");
    };
    let tail = &text[marker + 7..];
    let code: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
    let stderr = tail.split_once('\n').map(|(_, rest)| rest).unwrap_or("");
    let stdout = &text[..marker];
    if code != "0" && code != "4" {
        let why = first_line_300(stderr, mask);
        return CheckResult::fail(if why.is_empty() {
            format!("setup --check exited {code}")
        } else {
            why
        });
    }
    let list = match serde_json::from_str::<Value>(stdout.trim()) {
        Ok(Value::Array(list)) if list.iter().all(Value::is_object) => list,
        _ => return CheckResult::fail("setup --check printed rows this Notchling can't read"),
    };
    let rows: Vec<SubRow> = list
        .iter()
        .take(ROW_CAP)
        .map(|row| {
            let field = |k: &str| row.get(k).and_then(Value::as_str).unwrap_or("");
            SubRow {
                name: cut_300(field("name"), mask),
                ok: row.get("ok").and_then(Value::as_bool).unwrap_or(false),
                detail: cut_300(field("detail"), mask),
            }
        })
        .collect();
    let counted = rows.iter().filter(|r| r.name != NOT_COUNTED).count();
    let failed = rows
        .iter()
        .filter(|r| r.name != NOT_COUNTED && !r.ok)
        .count();
    if failed > 0 {
        return CheckResult::fail(format!("{failed} of {counted} checks failed")).rows(rows);
    }
    if code == "4" && counted == rows.len() {
        return CheckResult::fail("setup --check failed but named no row").rows(rows);
    }
    CheckResult::pass(format!("all {counted} checks passed")).rows(rows)
}
