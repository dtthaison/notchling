//! E13 in the daemon (Milestone 6): installing the hook entries into Claude Code's
//! `settings.json` and the `notchling-*` skills into Claude Code's, Codex's and Antigravity's
//! skills folders. Built and switched off, like `acting`: the `install` switch is off at every
//! start, and until the maintainer turns it on the Swift app owns both (decision 0011).
//!
//! | Op | Tier | Args | Reply |
//! | --- | --- | --- | --- |
//! | `install.plan` | app | `{action?: "install" \| "remove", what?, with_swift_hooks?}` | the envelope, `applied: false`; reads only, works with the switch off |
//! | `install.apply` | app | `{what?, with_swift_hooks?}` | the envelope |
//! | `install.remove` | app | `{what?}` | the envelope |
//!
//! `what` names the parts to act on; left out it is all of them. The names are `prompt_hooks`
//! (`PreToolUse` `AskUserQuestion` and `PermissionRequest` `*`), `page_hooks` (`PreToolUse`,
//! `PostToolUse` and `PostToolUseFailure` matching `Artifact`) and the skills' folder names
//! (`notchling-delegate`, `notchling-pages`, and `notchling-vault` once the vault supplies it).
//!
//! The envelope:
//!
//! ```json
//! {"on": false, "applied": false,
//!  "message": "install is off: the Swift app owns hooks and skills until the maintainer turns this on",
//!  "backup": "/…/settings.json.notchling-backup-1794150245123",
//!  "plan": {"action": "install", "changed": true, "blocked": null,
//!    "hooks": {"file": "/…/settings.json", "state": "present", "read_only": false,
//!      "refused": null, "changed": true,
//!      "changes": ["add PreToolUse (AskUserQuestion): '/…/notchling' hook (timeout 300 s)"],
//!      "notes": ["the Swift app's hook is in PermissionRequest (*); it was not changed"],
//!      "present": [{"event": "PermissionRequest", "matcher": "*", "origin": "swift",
//!                   "command": "'/…/Notchling' --hook", "managed": false}]},
//!    "skills": [{"name": "notchling-pages", "tool": "claude", "path": "/…/skills/notchling-pages",
//!                "action": "write", "reason": null, "error": null}]}}
//! ```
//!
//! - `message` is there while the switch is off (and `applied` is false, and nothing was written).
//! - `state` is `missing`, `empty`, `present`, or `refused` (with `refused` saying why: broken
//!   JSON, not an object, a dangling link, …). Planning a refused file is not an error.
//! - `blocked` is the reason an `install.apply` would be refused: the Swift app's `--hook` is
//!   still in the file, and two hooks must not both answer. `with_swift_hooks: true` goes ahead.
//!   Retirement rewrites the Swift entries (decision 0011), not this.
//! - `present` lists every `notchling hook` and Swift `--hook` entry in the file; `managed` is
//!   true for the ones in our places. Other hooks are never listed or changed.
//! - A failure (a refused file, a file that changed meanwhile, a write error) is code `failed`
//!   with a message; the file is then as it was. Nothing from the file reaches a message.

use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, INSTALL_OFF};
use notchling_engine::delegate::skill::{content as delegate_text, SkillTool};
use notchling_engine::install::{
    self, Action, FileError, FilePlan, Found, SkillPlan, SkillSpec, ToolRoots, Want, Wanted,
};
use notchling_engine::pages::skill::content as pages_text;
use notchling_engine::tools::codex::models;
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec};
use notchling_platform::paths::Os;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub const INSTALL_OPS: [&str; 3] = [ops::INSTALL_PLAN, ops::INSTALL_APPLY, ops::INSTALL_REMOVE];

/// Where everything is and what is installed.
pub struct InstallSetup {
    /// `~/.claude/settings.json`.
    pub settings: PathBuf,
    pub tools: ToolRoots,
    /// This program; the hook runs `<binary> hook`. `None` when the OS would not say.
    pub binary: Option<PathBuf>,
    pub os: Os,
    pub skills: Vec<SkillSpec>,
}

impl InstallSetup {
    /// From the daemon's layout. A fixture root keeps every tool's folder inside the root, so a
    /// test daemon never reaches the real ones.
    pub fn from_layout(layout: &Layout, binary: Option<PathBuf>) -> InstallSetup {
        let antigravity = layout
            .antigravity_dir()
            .parent()
            .map(|gemini| gemini.join("config"))
            .unwrap_or_else(|| layout.claude_dir.join("gemini-config"));
        let tools = ToolRoots {
            claude: layout.claude_dir.clone(),
            codex: layout.codex_dir(),
            antigravity,
        };
        let skills = binary
            .as_deref()
            .map(|b| default_skills(b, &tools.codex))
            .unwrap_or_default();
        InstallSetup {
            settings: layout.claude_dir.join("settings.json"),
            tools,
            binary,
            os: Os::current(),
            skills,
        }
    }
}

/// The delegate skill (all three tools) and the pages skill (Claude Code's: the Artifact tool is
/// its own). The vault's comes from the vault and is added with [`InstallOps::with_skill`].
pub fn default_skills(binary: &Path, codex_home: &Path) -> Vec<SkillSpec> {
    let for_delegate = binary.to_string_lossy().into_owned();
    let for_pages = for_delegate.clone();
    let codex_home = codex_home.to_path_buf();
    vec![
        SkillSpec {
            name: notchling_engine::delegate::skill::NAME.to_string(),
            tools: vec![SkillTool::Claude, SkillTool::Codex, SkillTool::Antigravity],
            text: Box::new(move |tool| {
                let model = models::load(&codex_home).first().map(|m| m.slug.clone());
                delegate_text(&for_delegate, tool, model.as_deref())
            }),
        },
        SkillSpec {
            name: notchling_engine::pages::skill::NAME.to_string(),
            tools: vec![SkillTool::Claude],
            text: Box::new(move |_| pages_text(&for_pages)),
        },
    ]
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Direction {
    Install,
    Remove,
}

impl Direction {
    fn want(self) -> Want {
        match self {
            Direction::Install => Want::Install,
            Direction::Remove => Want::Remove,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Direction::Install => "install",
            Direction::Remove => "remove",
        }
    }
}

/// What a call was asked to act on.
struct Request {
    direction: Direction,
    prompt: Want,
    pages: Want,
    skills: Vec<(String, Want)>,
    with_swift_hooks: bool,
}

pub struct InstallOps {
    setup: InstallSetup,
    switch: Arc<SwitchFlag>,
    /// Milliseconds since 1970, for the backup's name.
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
    /// One change at a time: two would each back up and then overwrite the other.
    writing: Mutex<()>,
}

fn system_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

impl InstallOps {
    /// `switch` is the daemon's `install` switch.
    pub fn new(setup: InstallSetup, switch: Arc<SwitchFlag>) -> InstallOps {
        InstallOps {
            setup,
            switch,
            clock: Box::new(system_ms),
            writing: Mutex::new(()),
        }
    }

    /// Another skill to install (the vault's, from where it is made).
    pub fn with_skill(mut self, skill: SkillSpec) -> InstallOps {
        self.setup.skills.push(skill);
        self
    }

    /// A fixed clock (tests).
    pub fn with_clock(mut self, clock: Box<dyn Fn() -> u64 + Send + Sync>) -> InstallOps {
        self.clock = clock;
        self
    }

    /// Takes the writing lock, so a test can have a call wait at it (tests).
    #[cfg(test)]
    pub(crate) fn hold_writes(&self) -> std::sync::MutexGuard<'_, ()> {
        self.writing.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// The three ops without a caller (the group's `call` is this): tests drive it directly.
    pub fn run(&self, op: &str, args: &Value) -> Result<Value, ApiError> {
        let request = self.request(op, args)?;
        if op == ops::INSTALL_PLAN {
            let plan = self.plan(&request)?;
            return Ok(envelope(self.switch.is_on(), false, None, &plan, None));
        }
        // The switch and the file are read only once this call has the lock: a call that waited
        // for another change must not write on what the switch said before the wait.
        let _one_at_a_time = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let plan = self.plan(&request)?;
        if !self.switch.is_on() {
            return Ok(envelope(false, false, Some(INSTALL_OFF), &plan, None));
        }
        if let Some(why) = &plan.blocked {
            return Err(failed(why.clone()));
        }
        self.apply(&request, plan)
    }

    fn request(&self, op: &str, args: &Value) -> Result<Request, ApiError> {
        let bad = |m: &str| ApiError::bad_request(m.to_string());
        let direction = match op {
            ops::INSTALL_REMOVE => Direction::Remove,
            ops::INSTALL_PLAN => match args.get("action").and_then(Value::as_str) {
                None | Some("install") => Direction::Install,
                Some("remove") => Direction::Remove,
                Some(_) => return Err(bad("action is \"install\" or \"remove\"")),
            },
            _ => Direction::Install,
        };
        let mut names: Vec<String> = vec![install::PROMPT_HOOKS.into(), install::PAGE_HOOKS.into()];
        names.extend(self.setup.skills.iter().map(|s| s.name.clone()));
        let chosen: Vec<String> = match args.get("what") {
            None | Some(Value::Null) => names.clone(),
            Some(Value::Array(items)) => {
                let mut chosen = Vec::new();
                for item in items {
                    let name = item
                        .as_str()
                        .ok_or_else(|| bad("what is a list of names"))?;
                    if !names.iter().any(|n| n == name) {
                        return Err(ApiError::bad_request(format!(
                            "no part named {name:?}; the parts are {}",
                            names.join(", ")
                        )));
                    }
                    chosen.push(name.to_string());
                }
                chosen
            }
            Some(_) => return Err(bad("what is a list of names")),
        };
        let want = |name: &str| {
            if chosen.iter().any(|c| c == name) {
                direction.want()
            } else {
                Want::Leave
            }
        };
        let flag = args.get("with_swift_hooks");
        if flag.is_some_and(|f| !f.is_boolean() && !f.is_null()) {
            return Err(bad("with_swift_hooks is true or false"));
        }
        Ok(Request {
            direction,
            prompt: want(install::PROMPT_HOOKS),
            pages: want(install::PAGE_HOOKS),
            skills: self
                .setup
                .skills
                .iter()
                .map(|s| (s.name.clone(), want(&s.name)))
                .collect(),
            with_swift_hooks: flag.and_then(Value::as_bool).unwrap_or(false),
        })
    }

    fn wanted(&self, request: &Request) -> Result<Wanted, ApiError> {
        let binary = self
            .setup
            .binary
            .as_deref()
            .ok_or_else(|| failed("can't find the path of this program, so no hook command"))?;
        Ok(Wanted {
            command: install::hook_command(binary, self.setup.os),
            prompt: request.prompt,
            pages: request.pages,
        })
    }

    /// Everything the request would do, from reading only.
    fn plan(&self, request: &Request) -> Result<Plan, ApiError> {
        let wanted = self.wanted(request)?;
        let hooks = install::plan(&self.setup.settings, &wanted);
        let blocked = match &hooks {
            Ok(p) if request.direction == Direction::Install && !request.with_swift_hooks => {
                let asked = request.prompt != Want::Leave || request.pages != Want::Leave;
                let swift = p
                    .hooks
                    .found
                    .iter()
                    .any(|f| f.origin == install::Origin::Swift);
                (asked && swift).then(|| {
                    "the Swift app's hook is still in settings.json; two hooks would both answer \
                     (retire the Swift app's first, or pass with_swift_hooks)"
                        .to_string()
                })
            }
            _ => None,
        };
        let skills = install::plan_skills(&self.setup.tools, &self.setup.skills, &request.skills);
        Ok(Plan {
            direction: request.direction,
            settings: self.setup.settings.clone(),
            hooks,
            skills,
            blocked,
        })
    }

    fn apply(&self, request: &Request, plan: Plan) -> Result<Value, ApiError> {
        let wanted = self.wanted(request)?;
        let mut backup = None;
        let mut plan = plan;
        if request.prompt != Want::Leave || request.pages != Want::Leave {
            match install::apply(&self.setup.settings, &wanted, (self.clock)()) {
                Ok(done) => {
                    backup = done.backup.map(|p| p.to_string_lossy().into_owned());
                    plan.hooks = Ok(done.plan);
                }
                Err(e) => return Err(failed(e.to_string())),
            }
        }
        let results = install::apply_skills(&self.setup.tools, &self.setup.skills, &request.skills);
        let errors: Vec<String> = results
            .iter()
            .filter_map(|r| {
                r.error.as_ref().map(|e| {
                    format!(
                        "{} in {}: {e}",
                        r.plan.name,
                        install::skills::tool_name(r.plan.tool)
                    )
                })
            })
            .collect();
        plan.skills = results.iter().map(|r| r.plan.clone()).collect();
        let mut value = envelope(true, true, None, &plan, backup);
        if let Some(list) = value["plan"]["skills"].as_array_mut() {
            for (item, result) in list.iter_mut().zip(&results) {
                item["error"] = json!(result.error);
            }
        }
        if errors.is_empty() {
            Ok(value)
        } else {
            Err(failed(format!(
                "the hooks were handled but some skills were not: {}",
                errors.join("; ")
            )))
        }
    }
}

impl OpGroup for InstallOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        INSTALL_OPS.contains(&op).then_some(OpSpec::APP)
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.run(op, args)
    }
}

struct Plan {
    direction: Direction,
    settings: PathBuf,
    hooks: Result<FilePlan, FileError>,
    skills: Vec<SkillPlan>,
    blocked: Option<String>,
}

fn found_json(f: &Found) -> Value {
    json!({
        "event": f.event,
        "matcher": f.matcher,
        "origin": f.origin.as_str(),
        "command": f.command,
        "managed": f.managed,
    })
}

fn hooks_json(path: &Path, hooks: &Result<FilePlan, FileError>) -> Value {
    let file = path.to_string_lossy();
    match hooks {
        Ok(p) => json!({
            "file": file,
            "state": p.state.as_str(),
            "read_only": p.read_only,
            "refused": null,
            "changed": p.hooks.changed,
            "changes": p.hooks.changes,
            "notes": p.hooks.notes,
            "present": p.hooks.found.iter().map(found_json).collect::<Vec<_>>(),
        }),
        // A file that cannot be edited is a fact about the file, not a failed call.
        Err(e) => json!({
            "file": file,
            "state": "refused",
            "read_only": false,
            "refused": e.to_string(),
            "changed": false,
            "changes": [],
            "notes": [],
            "present": [],
        }),
    }
}

fn skill_json(p: &SkillPlan) -> Value {
    json!({
        "name": p.name,
        "tool": install::skills::tool_name(p.tool),
        "path": p.dir.to_string_lossy(),
        "action": p.action.as_str(),
        "reason": p.reason,
        "error": null,
    })
}

fn envelope(
    on: bool,
    applied: bool,
    message: Option<&str>,
    plan: &Plan,
    backup: Option<String>,
) -> Value {
    let hooks_changed = plan.hooks.as_ref().is_ok_and(|p| p.hooks.changed);
    let skills_changed = plan
        .skills
        .iter()
        .any(|s| matches!(s.action, Action::Write | Action::Update | Action::Remove));
    let mut value = json!({
        "on": on,
        "applied": applied,
        "plan": {
            "action": plan.direction.name(),
            "changed": hooks_changed || skills_changed,
            "blocked": plan.blocked,
            "hooks": hooks_json(&plan.settings, &plan.hooks),
            "skills": plan.skills.iter().map(skill_json).collect::<Vec<_>>(),
        },
    });
    if let Some(message) = message {
        value["message"] = json!(message);
    }
    if let Some(backup) = backup {
        value["backup"] = json!(backup);
    }
    value
}
