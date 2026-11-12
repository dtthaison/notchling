//! `notchling delegate` and `notchling delegate-models` as functions of their streams (Swift
//! `DelegateCLI.run` and `models`): parse, refuse a third level of nesting, run the tool, copy
//! its media to `--out`, print, and answer the exit code (0 done, 1 failed, 124 timed out, 2 for a
//! command line that can't be run).
//!
//! The command needs no daemon and is not behind the `acting` switch: it runs a tool only
//! because the agent that called it asked, and it never touches a session it didn't start.

use super::deps::Deps;
use super::options::{self, Tool};
use super::outcome::{render, to_json, Outcome};
use super::{agy_pick, agy_run, codex_run, media};
use crate::tools::codex::models;
use std::io::Write;
use std::path::{Path, PathBuf};

/// Where the command runs.
pub struct Context<'a> {
    /// Relative paths are read against this.
    pub cwd: &'a Path,
    /// What `~` stands for.
    pub home: Option<&'a Path>,
    /// The value of `NOTCHLING_DELEGATE_DEPTH` in this process, if set.
    pub depth: Option<&'a str>,
}

/// Runs a job; `args` are what follows `delegate`. Prints the outcome on `out` and returns the
/// exit code. Anything wrong with the command line goes to `err` with exit 2.
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
    let mut outcome = match options.tool {
        Tool::Codex => codex_run::run(&options, &deps),
        Tool::Agy => agy_run::run(&options, &deps, err),
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
    finish(&outcome, parsed.json, out)
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

/// `notchling delegate-models [codex|agy]`: each tool's models and efforts. Exit 0.
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

/// `notchling delegate-models [codex|agy] --json`: the same listing as one JSON value, camelCase
/// like `delegate --json`: `{"codex": {"models": [{"slug", "name", "summary", "efforts",
/// "defaultEffort"?, "contextWindow"?}]}, "agy": {"available": bool, "models": [{"model",
/// "efforts"}]}}`; only the tool asked for when one is named. A tool with nothing to list has
/// `"models": []` (agy also `"available": false` when `agy models` can't run). Exit 0.
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
