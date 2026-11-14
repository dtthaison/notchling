use super::registry::Machine;
use super::step::{
    Cadence, CheckResult, Detected, Probe, RemoteOutput, SetupStep, StepContext, StepFix, StepId,
    SubRow,
};
use crate::versions::{Method, Tool, ToolState};

pub const TOOL_VERSIONS_SCRIPT: &str = r#"for t in claude codex agy notchling; do
  p=$(command -v "$t" 2>/dev/null)
  echo "$t.path=$p"
  if [ -n "$p" ]; then echo "$t.link=$(readlink "$p" 2>/dev/null)"; fi
done
for t in claude codex notchling; do
  p=$(command -v "$t" 2>/dev/null)
  if [ -n "$p" ]; then echo "$t.version=$("$p" --version 2>/dev/null | head -1)"; fi
done"#;

pub fn tool_versions(_m: &Machine) -> SetupStep {
    SetupStep::new(
        StepId::TOOL_VERSIONS,
        "Tool Versions",
        vec![StepId::REACHABLE],
        Cadence::EveryRefresh,
        Probe::remote(TOOL_VERSIONS_SCRIPT, parse_tool_versions),
        StepFix::HowTo(
            "Notchling only reads this. Matching a worker to the Mac comes later.".into(),
        ),
    )
}

fn row(tool: Tool, state: &ToolState) -> SubRow {
    match state {
        ToolState::NotInstalled => SubRow::new(tool.title(), false, "Not installed"),
        ToolState::Installed {
            version, method, ..
        } => {
            let shown = version
                .as_ref()
                .map_or_else(|| "version unknown".to_string(), |v| v.to_string());
            SubRow::new(tool.title(), true, &format!("{shown} · {}", method.title()))
        }
    }
}

fn parse_tool_versions(out: &RemoteOutput, ctx: &StepContext) -> CheckResult {
    if !out.text.lines().take(64).any(|l| l.contains(".path=")) {
        return CheckResult::cant_tell("couldn't read the tools");
    }
    let mut tools = crate::versions::parse_probe(&out.text, (ctx.now * 1000.0) as i64);
    for state in tools.tools.values_mut() {
        if let ToolState::Installed { method, path, .. } = state {
            if ctx.masks.leaks_into(path) {
                path.clear();
                *method = Method::Unknown;
            }
        }
    }
    let rows: Vec<SubRow> = Tool::ALL
        .into_iter()
        .filter_map(|t| tools.tools.get(&t).map(|s| row(t, s)))
        .collect();
    let found = rows.iter().filter(|r| r.ok).count();
    let text = match found {
        0 => "No agent tools found".to_string(),
        1 => "1 tool found".to_string(),
        n => format!("{n} tools found"),
    };
    CheckResult::pass(text).rows(rows).detected(Detected {
        tools: Some(tools),
        ..Detected::default()
    })
}
