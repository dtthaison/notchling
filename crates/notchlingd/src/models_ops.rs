use crate::catalog_ops::CatalogService;
use crate::install_ops::InstallOps;
use crate::layout::Layout;
use crate::op_group::OpGroup;
use notchling_engine::catalog::roles::Roles;
use notchling_engine::catalog::{LiveModel, Tool};
use notchling_engine::delegate::deps::Capture;
use notchling_engine::delegate::models_skill;
use notchling_engine::delegate::models_update::{
    agy_live, agy_live_state, codex_live, codex_live_state, codex_snapshot, diff, suggestions,
    unpriced, Report, ToolReport,
};
use notchling_engine::delegate::real::{self, Folders};
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

pub const MODELS_OPS: [&str; 2] = [ops::MODELS_CHECK, ops::MODELS_UPDATE];

#[derive(Clone)]
pub struct Sources {
    pub codex: Option<PathBuf>,
    pub agy: Option<String>,
    pub capture: Capture,
}

impl Sources {
    pub fn from_folders(folders: &Folders) -> Sources {
        let deps = real::deps(folders, Arc::new(AtomicBool::new(false)));
        Sources {
            codex: deps.codex_binary,
            agy: deps.agy_binary,
            capture: deps.capture,
        }
    }
}

pub struct ModelsOps {
    catalog: Arc<CatalogService>,
    install: Option<Arc<InstallOps>>,
    sources: Sources,
    working: Mutex<()>,
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

impl ModelsOps {
    pub fn new(
        catalog: Arc<CatalogService>,
        install: Option<Arc<InstallOps>>,
        sources: Sources,
    ) -> ModelsOps {
        ModelsOps {
            catalog,
            install,
            sources,
            working: Mutex::new(()),
        }
    }

    pub fn for_daemon(
        layout: &Layout,
        catalog: Arc<CatalogService>,
        install: Arc<InstallOps>,
    ) -> ModelsOps {
        if layout.fixture {
            let sources = Sources {
                codex: None,
                agy: None,
                capture: Arc::new(|_, _| None),
            };
            return ModelsOps::new(catalog, Some(install), sources);
        }
        let real_home = notchling_platform::paths::home().unwrap_or_else(|| layout.home());
        let folders = crate::delegate_cli::folders(
            layout,
            &real_home,
            std::env::var("PATH").unwrap_or_default(),
            std::env::var_os("CODEX_HOME").as_deref(),
        );
        ModelsOps::new(catalog, Some(install), Sources::from_folders(&folders))
    }

    fn read_codex(&self) -> Result<Vec<u8>, String> {
        let binary = self
            .sources
            .codex
            .as_deref()
            .ok_or_else(|| "Codex isn't installed here".to_string())?;
        (self.sources.capture)(binary, &["debug", "models"])
            .map(String::into_bytes)
            .ok_or_else(|| "couldn't run codex debug models".to_string())
    }

    fn read_agy(&self) -> Result<String, String> {
        let binary = self
            .sources
            .agy
            .as_deref()
            .ok_or_else(|| "Antigravity CLI isn't installed here".to_string())?;
        (self.sources.capture)(Path::new(binary), &["models"])
            .ok_or_else(|| "couldn't run agy models".to_string())
    }

    fn tool_report(
        &self,
        tool: Tool,
        source: &str,
        before: &[LiveModel],
        fetched: Result<&[LiveModel], String>,
    ) -> ToolReport {
        let catalog = self.catalog.catalog();
        match fetched {
            Ok(after) => ToolReport {
                tool: tool.name().into(),
                source: source.into(),
                read: true,
                note: None,
                total: after.len(),
                diff: diff(before, after),
                unpriced: unpriced(catalog, tool, after),
                written: false,
            },
            Err(why) => ToolReport {
                tool: tool.name().into(),
                source: source.into(),
                read: false,
                note: Some(format!("{why}; the lists Notchling has were kept")),
                total: before.len(),
                diff: Default::default(),
                unpriced: unpriced(catalog, tool, before),
                written: false,
            },
        }
    }

    fn write_codex(&self, raw: &[u8]) -> bool {
        let (Some(file), Some(text)) = (self.catalog.codex_snapshot_path(), codex_snapshot(raw))
        else {
            return false;
        };
        if let Some(dir) = file.parent() {
            if std::fs::create_dir_all(dir).is_err() {
                return false;
            }
        }
        notchling_platform::files::write_private_no_follow(file, text.as_bytes()).is_ok()
    }

    fn run(&self, apply: bool, skills: bool) -> Report {
        let _one = self.working.lock().unwrap_or_else(|e| e.into_inner());
        let mut report = Report {
            applied: apply,
            ..Report::default()
        };

        let claude_before = self.catalog.live_models(Tool::Claude);
        let claude = self.catalog.read_claude_now();
        let mut part = self.tool_report(
            Tool::Claude,
            "Anthropic's model list",
            &claude_before,
            claude.as_deref().map_err(String::clone),
        );
        if let (true, Ok(models)) = (apply, &claude) {
            self.catalog.store_live(Tool::Claude, models);
            part.written = !part.diff.is_empty();
        }
        report.tools.push(part);

        let codex_before = self.catalog.live_models(Tool::Codex);
        let codex_raw = self.read_codex();
        let codex_after =
            codex_raw
                .as_deref()
                .map_err(String::clone)
                .and_then(|raw| match codex_live(raw) {
                    after if after.is_empty() => {
                        Err("codex debug models listed nothing".to_string())
                    }
                    after => Ok(after),
                });
        let mut part = self.tool_report(
            Tool::Codex,
            "codex debug models",
            &codex_before,
            codex_after.as_deref().map_err(String::clone),
        );
        if let (true, Ok(raw), Ok(_)) = (apply, &codex_raw, &codex_after) {
            if !part.diff.is_empty() && self.write_codex(raw) {
                self.catalog.announce_changed(Tool::Codex);
                part.written = true;
            }
        }
        report.tools.push(part);

        let agy_before = self.catalog.live_models(Tool::Antigravity);
        let listing = self.read_agy();
        let agy_after = listing.as_deref().map_err(String::clone).and_then(|l| {
            match agy_live(l, &agy_before) {
                after if after.is_empty() => Err("agy models listed nothing".to_string()),
                after => Ok(after),
            }
        });
        let mut part = self.tool_report(
            Tool::Antigravity,
            "agy models",
            &agy_before,
            agy_after.as_deref().map_err(String::clone),
        );
        if let (true, Ok(models)) = (apply, &agy_after) {
            self.catalog.store_live(Tool::Antigravity, models);
            part.written = !part.diff.is_empty();
        }
        report.tools.push(part);

        let roles = Roles::load(self.catalog.override_path());
        let codex_live_view = codex_live_state(
            self.catalog.codex_dir(),
            codex_raw
                .as_ref()
                .ok()
                .filter(|_| codex_after.is_ok())
                .map(Vec::as_slice),
        );
        let agy_live_view = agy_live_state(
            listing.as_deref().ok().filter(|_| agy_after.is_ok()),
            &agy_before,
        );
        report.suggestions = suggestions(&roles, &codex_live_view, &agy_live_view);

        if skills {
            report.skills = self.skills(apply);
        }
        report
    }

    fn skills(&self, apply: bool) -> Vec<String> {
        let Some(install) = &self.install else {
            return Vec::new();
        };
        let op = if apply {
            ops::INSTALL_APPLY
        } else {
            ops::INSTALL_PLAN
        };
        let names = [notchling_engine::delegate::skill::NAME, models_skill::NAME];
        let reply = match install.run(op, &json!({ "what": names })) {
            Ok(reply) => reply,
            Err(e) => return vec![format!("not rewritten: {}", e.message)],
        };
        if apply && reply.get("on").and_then(Value::as_bool) == Some(false) {
            return vec![
                "not rewritten: the Install Hooks and Skills switch is off in Notchling".into(),
            ];
        }
        reply["plan"]["skills"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|s| {
                let name = s.get("name")?.as_str()?;
                let tool = s.get("tool")?.as_str()?;
                let action = s.get("action")?.as_str()?;
                let shown = match (apply, action) {
                    (false, "update" | "write") => format!("would {action}"),
                    (_, "unchanged") => "up to date".to_string(),
                    (_, "skip") => format!(
                        "left alone ({})",
                        s.get("reason").and_then(Value::as_str).unwrap_or("skipped")
                    ),
                    (true, "update") => "updated".to_string(),
                    (true, "write") => "written".to_string(),
                    (_, other) => other.to_string(),
                };
                Some(format!("{name} ({tool}): {shown}"))
            })
            .collect()
    }
}

impl OpGroup for ModelsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        MODELS_OPS.contains(&op).then_some(OpSpec::AGENT)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        let skills = match args.get("skills") {
            None | Some(Value::Null) => true,
            Some(Value::Bool(b)) => *b,
            Some(_) => return Err(ApiError::bad_request("skills is true or false")),
        };
        match op {
            ops::MODELS_CHECK => Ok(self.run(false, skills).to_json()),
            ops::MODELS_UPDATE => Ok(self.run(true, skills).to_json()),
            other => Err(failed(format!("{other} has no handler"))),
        }
    }
}
