use super::agy_trust::{self, Trusted};
use super::deps::Deps;
use super::options::{self, Tool};
use super::outcome::{render, to_json, Outcome};
use super::record::{Delegation, Recorder};
use super::{agy_pick, agy_run, codex_run, media};
use crate::tools::codex::models;
use crate::transcript::util::now_secs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Arc;

pub struct Context<'a> {
    pub cwd: &'a Path,
    pub home: Option<&'a Path>,
    pub depth: Option<&'a str>,
    pub record: Option<RecordTo<'a>>,
}

pub struct RecordTo<'a> {
    pub dir: &'a Path,
    pub parent_session: &'a str,
    pub parent_cwd: &'a str,
    pub pid: u32,
}

pub fn run(
    args: &[String],
    ctx: &Context<'_>,
    mut deps: Deps,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> i32 {
    let parsed = match options::parse(args, ctx.cwd, ctx.home) {
        Ok(p) => p,
        Err(e) => return usage_error(err, &e.message),
    };
    deps.depth = match options::enter_depth(ctx.depth) {
        Ok(depth) => depth,
        Err(message) => return usage_error(err, &message),
    };
    let options = parsed.options;
    if !Path::new(&options.folder).exists() {
        return usage_error(err, &format!("No folder at {}.", options.folder));
    }
    let recorder = ctx.record.as_ref().map(|to| {
        let record = Delegation::new(
            to.parent_session,
            to.parent_cwd,
            &options,
            now_secs(),
            to.pid,
        );
        Arc::new(Recorder::start(to.dir, record, now_secs()))
    });
    deps.recorder = recorder.clone();
    let stop = deps.stop.clone();
    let mut outcome = match options.tool {
        Tool::Codex => codex_run::run(&options, &deps),
        Tool::Agy => {
            trust_folder(&options.folder, &deps, err);
            agy_run::run(&options, &deps, err)
        }
    };
    if let (Some(dir), false) = (&options.out, outcome.media.is_empty()) {
        let files: Vec<PathBuf> = outcome.media.iter().map(PathBuf::from).collect();
        match media::copy_to(&files, Path::new(dir)) {
            Ok(copied) => {
                outcome.copied = copied
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned())
                    .collect();
            }
            Err(e) => {
                outcome.error = outcome
                    .error
                    .or_else(|| Some(format!("Couldn't copy to {dir}: {e}")));
            }
        }
    }
    if let Some(recorder) = &recorder {
        recorder.finish(&outcome, stop.load(Ordering::SeqCst), now_secs());
    }
    finish(&outcome, parsed.json, out)
}

fn trust_folder(folder: &str, deps: &Deps, err: &mut dyn Write) {
    let Some(files) = &deps.agy_trust else {
        return;
    };
    if let Trusted::Added(_) = agy_trust::trust(files, Path::new(folder)) {
        let _ = writeln!(
            err,
            "Added {folder} to Antigravity CLI's trusted workspaces (Claude Code trusts it)."
        );
    }
}

fn finish(outcome: &Outcome, json: bool, out: &mut dyn Write) -> i32 {
    let text = if json {
        to_json(outcome)
    } else {
        render(outcome)
    };
    let _ = writeln!(out, "{text}");
    outcome.exit_code()
}

fn usage_error(err: &mut dyn Write, message: &str) -> i32 {
    let _ = writeln!(err, "{message}");
    2
}

pub fn models_listing(args: &[String], deps: &Deps, out: &mut dyn Write) -> i32 {
    let only = args.first().and_then(|a| Tool::from_name(a));
    let mut lines: Vec<String> = Vec::new();
    if only != Some(Tool::Agy) {
        lines.push("codex (--model <slug> --effort <level>):".into());
        let known = models::load(&deps.codex_home);
        if known.is_empty() {
            lines.push("  (Codex hasn't cached its model list yet; run codex once)".into());
        }
        for m in known {
            let default = m
                .default_effort
                .map(|d| format!(" (default {d})"))
                .unwrap_or_default();
            lines.push(format!(
                "  {}  efforts: {}{default}",
                m.slug,
                m.efforts.join(", ")
            ));
        }
    }
    if only != Some(Tool::Codex) {
        lines.push("agy (--model <model> --effort low|medium|high):".into());
        let listing = deps
            .agy_binary
            .as_deref()
            .and_then(|agy| (deps.capture)(Path::new(agy), &["models"]));
        match listing {
            Some(listing) => {
                for (model, levels) in agy_pick::models(&listing) {
                    let efforts = if levels.is_empty() {
                        String::new()
                    } else {
                        format!("  efforts: {}", levels.join(", "))
                    };
                    lines.push(format!("  {model}{efforts}"));
                }
            }
            None => lines.push("  (couldn't run agy models)".into()),
        }
    }
    let _ = writeln!(out, "{}", lines.join("\n"));
    0
}

pub fn models_listing_json(args: &[String], deps: &Deps, out: &mut dyn Write) -> i32 {
    let only = args.first().and_then(|a| Tool::from_name(a));
    let mut root = serde_json::Map::new();
    if only != Some(Tool::Agy) {
        let models: Vec<serde_json::Value> = models::load(&deps.codex_home)
            .into_iter()
            .map(|m| {
                let mut v = serde_json::json!({
                    "slug": m.slug,
                    "name": m.name,
                    "summary": m.summary,
                    "efforts": m.efforts,
                });
                if let Some(d) = m.default_effort {
                    v["defaultEffort"] = d.into();
                }
                if let Some(w) = m.context_window {
                    v["contextWindow"] = w.into();
                }
                v
            })
            .collect();
        root.insert("codex".into(), serde_json::json!({ "models": models }));
    }
    if only != Some(Tool::Codex) {
        let listing = deps
            .agy_binary
            .as_deref()
            .and_then(|agy| (deps.capture)(Path::new(agy), &["models"]));
        let available = listing.is_some();
        let models: Vec<serde_json::Value> = listing
            .map(|l| {
                agy_pick::models(&l)
                    .into_iter()
                    .map(|(model, efforts)| serde_json::json!({"model": model, "efforts": efforts}))
                    .collect()
            })
            .unwrap_or_default();
        root.insert(
            "agy".into(),
            serde_json::json!({ "available": available, "models": models }),
        );
    }
    let value = serde_json::Value::Object(root);
    let _ = writeln!(
        out,
        "{}",
        serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
    );
    0
}

#[cfg(test)]
#[path = "command_tests.rs"]
mod tests;
