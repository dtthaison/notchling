use crate::catalog_ops::CatalogService;
use crate::cost_profile_json as view;
use crate::install_ops::{InstallOps, InstallSetup};
use crate::layout::Layout;
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, INSTALL_OFF};
use notchling_engine::catalog::Tool;
use notchling_engine::cost_profile::guard;
use notchling_engine::cost_profile::model::{habit_range, parse_tool, tool_key, HABIT_KEYS};
use notchling_engine::cost_profile::presets::data;
use notchling_engine::cost_profile::probe::{CommandProbe, NoProbe};
use notchling_engine::cost_profile::{Cell, Choice, Options, Profile, Role, Service, CONSENTS};
use notchling_engine::install::GUARD_HOOKS;
use notchling_engine::settings::Settings;
use notchling_engine::tools::codex::{models as codex_models, plan::find_binary};
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

pub const COST_PROFILE_OPS: [&str; 6] = [
    ops::COST_PROFILE_GET,
    ops::COST_PROFILE_SET,
    ops::COST_SETUP_PLAN,
    ops::COST_SETUP_APPLY,
    ops::COST_SETUP_UNDO,
    ops::COST_SETUP_CHECK,
];

pub struct CostProfileOps {
    service: Service,
    switch: Arc<SwitchFlag>,
    writing: Mutex<()>,
    clock: Box<dyn Fn() -> u64 + Send + Sync>,
    after: Option<Arc<dyn Fn() + Send + Sync>>,
}

fn system_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn bad(message: impl Into<String>) -> ApiError {
    ApiError::bad_request(message)
}

fn failed(message: impl Into<String>) -> ApiError {
    ApiError::new(ErrorCode::Failed, message)
}

impl CostProfileOps {
    pub fn new(service: Service, switch: Arc<SwitchFlag>) -> CostProfileOps {
        CostProfileOps {
            service,
            switch,
            writing: Mutex::new(()),
            clock: Box::new(system_ms),
            after: None,
        }
    }

    pub fn with_after(mut self, after: Arc<dyn Fn() + Send + Sync>) -> CostProfileOps {
        self.after = Some(after);
        self
    }

    pub fn sync_guard(&self) {
        let cap = self.service.profile().habits().parallel_cap;
        guard::sync(&self.service.data_dir, cap);
    }

    fn guard_changed(&self) {
        let cap = self.service.profile().habits().parallel_cap;
        guard::sync(&self.service.data_dir, cap);
        if let Some(after) = &self.after {
            after();
        }
    }

    fn guard_view(&self, mut out: Value) -> Value {
        let dir = &self.service.data_dir;
        out["profile"]["guard"] = json!({
            "level": guard::level(dir),
            "set_up": guard::set_up(dir),
            "on": guard::enabled(dir),
        });
        out
    }

    pub fn with_clock(mut self, clock: Box<dyn Fn() -> u64 + Send + Sync>) -> CostProfileOps {
        self.clock = clock;
        self
    }

    pub fn for_daemon(
        layout: &Layout,
        catalog: Arc<CatalogService>,
        switch: Arc<SwitchFlag>,
    ) -> CostProfileOps {
        let roots = InstallSetup::from_layout(layout, None).tools;
        let home = layout
            .claude_dir
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_default();
        let path = std::env::var("PATH").unwrap_or_default();
        let probe: Arc<dyn notchling_engine::cost_profile::keys::Probe + Send + Sync> =
            match find_binary(&home, &path, |p| p.is_file()) {
                Some(binary) => Arc::new(CommandProbe {
                    binary,
                    home: Some(roots.codex.clone()),
                }),
                None => Arc::new(NoProbe),
            };
        let live_of = catalog.clone();
        let service = Service {
            roots,
            data_dir: layout.data_dir.clone(),
            catalog: catalog.shared_catalog(),
            live: Arc::new(move || live_of.live_models(Tool::Claude)),
            probe,
            codex_snapshot: Some(layout.state_dir().join(codex_models::SNAPSHOT)),
        };
        CostProfileOps::new(service, switch)
    }

    fn consent(args: &Value) -> Result<Options, ApiError> {
        let mut opts = Options::default();
        match args.get("consent") {
            None | Some(Value::Null) => {}
            Some(Value::Array(items)) => {
                for item in items {
                    let name = item
                        .as_str()
                        .ok_or_else(|| bad("consent is a list of names"))?;
                    if !CONSENTS.contains(&name) {
                        return Err(bad(format!(
                            "no consent named {name:?}; they are {}",
                            CONSENTS.join(", ")
                        )));
                    }
                    opts.consent.insert(name.to_string());
                }
            }
            Some(_) => return Err(bad("consent is a list of names")),
        }
        match args.get("wait") {
            None | Some(Value::Null) => {}
            Some(Value::Bool(on)) => opts.features.wait = *on,
            Some(_) => return Err(bad("wait is true or false")),
        }
        Ok(opts)
    }

    fn tool_arg(args: &Value) -> Result<Tool, ApiError> {
        let name = args
            .get("tool")
            .and_then(Value::as_str)
            .ok_or_else(|| bad("cost.setup.undo takes {tool: claude, codex or agy}"))?;
        let name = if name == "antigravity" { "agy" } else { name };
        parse_tool(name).ok_or_else(|| bad("tool is claude, codex or agy"))
    }

    fn get(&self, caller: &Caller) -> Value {
        let avail = self.service.available();
        self.guard_view(
            json!({"profile": view::profile(&self.service.profile(), &avail, caller.tier)}),
        )
    }

    fn set(&self, args: &Value) -> Result<Value, ApiError> {
        let _one = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        let mut profile = self.service.profile();
        let object = args
            .as_object()
            .ok_or_else(|| bad("cost.profile.set takes an object"))?;
        for key in object.keys() {
            if ![
                "preset", "tools", "picks", "habits", "guards", "guard", "reset",
            ]
            .contains(&key.as_str())
            {
                return Err(bad(format!(
                    "cost.profile.set takes preset, tools, picks, habits, guards, guard or reset, not {key}"
                )));
            }
        }
        if let Some(name) = object.get("preset") {
            let name = name.as_str().ok_or_else(|| bad("preset is a name"))?;
            if !data().presets.contains_key(name) {
                return Err(bad(format!(
                    "no preset named {name:?}; they are {}",
                    data().preset_names().join(", ")
                )));
            }
            profile.preset = name.to_string();
        }
        if object.get("reset") == Some(&Value::Bool(true)) {
            profile.picks.clear();
            profile.habit_edits.clear();
        }
        Self::set_tools(&mut profile, object.get("tools"))?;
        Self::set_picks(&mut profile, object.get("picks"))?;
        Self::set_habits(&mut profile, object.get("habits"))?;
        Self::set_guards(&mut profile, object.get("guards"))?;
        self.service.save_profile(&profile).map_err(failed)?;
        self.set_guard(object.get("guard"))?;
        self.guard_changed();
        Ok(self.get_for_app())
    }

    fn set_guard(&self, value: Option<&Value>) -> Result<(), ApiError> {
        let Some(value) = value else {
            return Ok(());
        };
        let level = value
            .as_str()
            .filter(|l| *l == guard::LOG || *l == guard::OFF)
            .ok_or_else(|| bad("guard is \"log\" or \"off\""))?;
        let mut store =
            Settings::open(&self.service.settings_file()).map_err(|e| failed(e.to_string()))?;
        store
            .set(guard::KEY, Value::from(level))
            .map_err(|e| failed(e.to_string()))
    }

    fn get_for_app(&self) -> Value {
        let avail = self.service.available();
        self.guard_view(
            json!({"profile": view::profile(&self.service.profile(), &avail, Tier::App)}),
        )
    }

    fn set_tools(profile: &mut Profile, value: Option<&Value>) -> Result<(), ApiError> {
        let Some(value) = value else {
            return Ok(());
        };
        let map = value
            .as_object()
            .ok_or_else(|| bad("tools is {claude?, codex?, agy?}: true or false each"))?;
        for (name, on) in map {
            let tool = parse_tool(name).ok_or_else(|| bad(format!("no tool named {name:?}")))?;
            let on = on.as_bool().ok_or_else(|| bad("a tool is true or false"))?;
            profile.tools.insert(tool, on);
        }
        Ok(())
    }

    fn set_picks(profile: &mut Profile, value: Option<&Value>) -> Result<(), ApiError> {
        let Some(value) = value else {
            return Ok(());
        };
        let map = value
            .as_object()
            .ok_or_else(|| bad("picks is {\"role.tool\": \"model effort\" or null}"))?;
        for (name, pick) in map {
            let (role, tool) = name
                .split_once('.')
                .and_then(|(r, t)| Some((Role::parse(r)?, parse_tool(t)?)))
                .ok_or_else(|| bad(format!("{name:?} is not role.tool, such as code.claude")))?;
            match pick {
                Value::Null => {
                    profile.picks.remove(&(role, tool));
                }
                Value::String(text) => {
                    let cell = Cell::parse(tool, text).ok_or_else(|| {
                        bad(format!(
                            "{text:?} is not \"model effort\" for {}",
                            tool_key(tool)
                        ))
                    })?;
                    profile.picks.insert((role, tool), cell);
                }
                _ => return Err(bad("a pick is \"model effort\" or null")),
            }
        }
        Ok(())
    }

    fn set_habits(profile: &mut Profile, value: Option<&Value>) -> Result<(), ApiError> {
        let Some(value) = value else {
            return Ok(());
        };
        let map = value
            .as_object()
            .ok_or_else(|| bad("habits is {name: number or null}"))?;
        for (name, n) in map {
            if !HABIT_KEYS.contains(&name.as_str()) {
                return Err(bad(format!(
                    "no habit named {name:?}; they are {}",
                    HABIT_KEYS.join(", ")
                )));
            }
            match n {
                Value::Null => {
                    profile.habit_edits.remove(name);
                }
                _ => {
                    let n = n
                        .as_u64()
                        .ok_or_else(|| bad(format!("{name} is a whole number")))?;
                    let (low, high) = habit_range(name);
                    if n < low || n > high {
                        return Err(bad(format!("{name} takes a number from {low} to {high}")));
                    }
                    profile.habit_edits.insert(name.clone(), n);
                }
            }
        }
        Ok(())
    }

    fn set_guards(profile: &mut Profile, value: Option<&Value>) -> Result<(), ApiError> {
        let Some(value) = value else {
            return Ok(());
        };
        let map = value
            .as_object()
            .ok_or_else(|| bad("guards is {name: level or null}"))?;
        for (name, level) in map {
            let plain = |s: &str, max: usize| {
                !s.is_empty()
                    && s.len() <= max
                    && s.chars()
                        .all(|c| c.is_ascii_lowercase() || c == '_' || c == '-')
            };
            if !plain(name, 32) {
                return Err(bad("a guard name is lowercase letters, - and _"));
            }
            match level {
                Value::Null => {
                    profile.guards.remove(name);
                }
                Value::String(l) if plain(l, 16) => {
                    profile.guards.insert(name.clone(), l.clone());
                }
                _ => return Err(bad("a guard level is a short lowercase word or null")),
            }
        }
        Ok(())
    }

    fn plan(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let opts = Self::consent(args)?;
        let plan = self.service.plan(&opts).map_err(failed)?;
        let avail = self.service.available();
        Ok(json!({"on": self.switch.is_on(), "plan": view::plan(&plan, &avail, caller.tier)}))
    }

    fn off(&self) -> Option<Value> {
        (!self.switch.is_on())
            .then(|| json!({"on": false, "applied": false, "message": INSTALL_OFF}))
    }

    fn apply(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        let opts = Self::consent(args)?;
        let _one = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(off) = self.off() {
            let mut out = off;
            let plan = self.service.plan(&opts).map_err(failed)?;
            out["plan"] = view::plan(&plan, &self.service.available(), caller.tier);
            return Ok(out);
        }
        let (plan, applied, saved) = self.service.apply(&opts, (self.clock)()).map_err(failed)?;
        self.guard_changed();
        let mut out = json!({
            "on": true,
            "applied": applied.results.iter().any(|r| r.done),
            "failed": applied.failed(),
            "results": applied.results.iter().map(view::done).collect::<Vec<_>>(),
            "plan": view::plan(&plan, &self.service.available(), caller.tier),
        });
        if let Some(why) = saved {
            out["saveError"] = json!(why);
        }
        if applied.failed() {
            let first = applied.results.iter().find_map(|r| r.error.clone());
            return Err(failed(format!(
                "Set Up stopped part way: {}",
                first.unwrap_or_default()
            ))
            .with_data(out));
        }
        Ok(out)
    }

    fn undo(&self, args: &Value) -> Result<Value, ApiError> {
        let tool = Self::tool_arg(args)?;
        let _one = self.writing.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(off) = self.off() {
            return Ok(off);
        }
        let results = self.service.undo(tool, (self.clock)()).map_err(failed)?;
        self.guard_changed();
        Ok(json!({
            "on": true,
            "tool": tool_key(tool),
            "results": results.iter().map(view::undone).collect::<Vec<_>>(),
            "conflicts": results.iter().filter(|r| r.outcome.as_str() == "conflict").count(),
        }))
    }

    fn check(&self, caller: &Caller, args: &Value) -> Result<Value, ApiError> {
        if let Some(pick) = args.get("resolve").filter(|v| !v.is_null()) {
            if caller.tier != Tier::App {
                return Err(ApiError::new(
                    ErrorCode::Forbidden,
                    "only the Notchling app may keep or restore a file",
                ));
            }
            let id = pick
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| bad("resolve is {id, choice: keep or restore}"))?;
            let choice = match pick.get("choice").and_then(Value::as_str) {
                Some("keep") => Choice::Keep,
                Some("restore") => Choice::Restore,
                _ => return Err(bad("choice is keep or restore")),
            };
            let _one = self.writing.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(off) = self.off() {
                return Ok(off);
            }
            self.service
                .resolve(id, choice, (self.clock)())
                .map_err(failed)?;
        }
        let rows = self.service.check().map_err(failed)?;
        let drift = rows
            .iter()
            .any(|r| !matches!(r.state, "in_step" | "skipped"));
        Ok(json!({
            "on": self.switch.is_on(),
            "rows": rows.iter().map(view::row).collect::<Vec<_>>(),
            "drift": drift,
        }))
    }
}

impl OpGroup for CostProfileOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::COST_PROFILE_GET | ops::COST_SETUP_PLAN | ops::COST_SETUP_CHECK => {
                Some(OpSpec::AGENT)
            }
            _ if COST_PROFILE_OPS.contains(&op) => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::COST_PROFILE_GET => Ok(self.get(caller)),
            ops::COST_PROFILE_SET => self.set(args),
            ops::COST_SETUP_PLAN => self.plan(caller, args),
            ops::COST_SETUP_APPLY => self.apply(caller, args),
            ops::COST_SETUP_UNDO => self.undo(args),
            ops::COST_SETUP_CHECK => self.check(caller, args),
            _ => Err(ApiError::new(
                ErrorCode::UnknownOp,
                format!("no op named {op}"),
            )),
        }
    }
}

pub fn wired(
    layout: &Layout,
    catalog: &Arc<CatalogService>,
    switches: &crate::switches::Switches,
    install: &Arc<InstallOps>,
) -> Arc<CostProfileOps> {
    let install = install.clone();
    let after: Arc<dyn Fn() + Send + Sync> = Arc::new(move || {
        let _ = install.run(ops::INSTALL_APPLY, &json!({"what": [GUARD_HOOKS]}));
    });
    let ops = CostProfileOps::for_daemon(layout, catalog.clone(), switches.install.clone())
        .with_after(after);
    ops.sync_guard();
    Arc::new(ops)
}
