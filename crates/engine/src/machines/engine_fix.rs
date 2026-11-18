use super::admin_secret::failure;
use super::engine::{lock, SetupEngine};
use super::engine_plan::{AdminAnswer, ApprovedStep};
use super::masks::{FixLog, Masks};
use super::registry::{AdminAccess, Machine};
use super::runner::{Channel, OutputSink};
use super::shell;
use super::step::{run, FixContext, FixOutcome, LinkOffer, SetupStep, StepFix, StepId};
use super::steps_tailscale_linux::is_sign_in_link;
use crate::accounts::Secret;
use crate::versions::{selfupdate_log, Tool};
use std::sync::Arc;

fn fail(m: impl Into<String>) -> FixOutcome {
    FixOutcome::fail(m)
}

#[derive(Default)]
pub(super) struct AdminUse {
    pub(super) secret: Option<Secret>,
    pub(super) vault: Option<String>,
}

impl SetupEngine {
    pub(super) fn answer_for(
        &self,
        m: &Machine,
        shown: &ApprovedStep,
        ask_admin: &dyn Fn(&ApprovedStep) -> AdminAnswer,
    ) -> AdminAnswer {
        match m.admin_secret() {
            Some(entry) if m.admin() == AdminAccess::Sudo => AdminAnswer::Vault(entry.to_owned()),
            _ => ask_admin(shown),
        }
    }

    pub fn set_admin_secret(&self, machine: &str, entry: Option<&str>) -> Result<(), String> {
        let _writing = super::registry::write_lock();
        let mut list = self.deps.store.machines();
        let Some(m) = list.iter_mut().find(|m| m.name == machine) else {
            return Err(format!("no machine named {machine}"));
        };
        if m.admin_secret() == entry.filter(|e| !e.is_empty()) {
            return Ok(());
        }
        m.set_admin_secret(entry);
        if self.deps.store.save(&list) {
            self.changed();
            Ok(())
        } else {
            Err("Couldn't save machines.json.".into())
        }
    }

    pub(super) fn admin_use(
        &self,
        m: &Machine,
        needs_admin: bool,
        admin: &AdminAnswer,
    ) -> Result<AdminUse, String> {
        if !(needs_admin && m.admin() == AdminAccess::Sudo) {
            return Ok(AdminUse::default());
        }
        if let Some(typed) = admin.password() {
            return Ok(AdminUse {
                secret: Some(typed.clone()),
                vault: None,
            });
        }
        let AdminAnswer::Vault(shown) = admin else {
            return Ok(AdminUse::default());
        };
        let entry = m
            .admin_secret()
            .ok_or_else(|| format!("{} has no Vault entry for the admin password", m.name))?
            .to_owned();
        if &entry != shown {
            return Err(format!(
                "{}'s Vault entry changed from {shown}; Set up again",
                m.name
            ));
        }
        match self.admin_secrets.read(&entry, &m.name) {
            Ok(secret) if !secret.is_empty() => Ok(AdminUse {
                secret: Some(secret),
                vault: Some(entry),
            }),
            Ok(_) => Err(failure(&entry, "it has no password saved")),
            Err(why) => Err(failure(&entry, &why)),
        }
    }

    fn append_log(&self, machine: &str, lines: Vec<String>) {
        append(&self.state, machine, lines);
    }

    pub(super) fn refusal(
        &self,
        step: &SetupStep,
        m: &Machine,
        admin: &AdminAnswer,
        needs_admin: bool,
        check_needs: bool,
        turning_on: bool,
    ) -> Option<String> {
        if self.running(&m.name).is_some() {
            return Some(format!("another Fix is running on {}", m.name));
        }
        if turning_on {
            if let Some(blocker) = self.turn_on_blocker(&m.name) {
                return Some(format!("waits for {blocker}"));
            }
        }
        if check_needs {
            let results = self.results(&m.name);
            let list = self.step_list(m);
            for need in &step.needs {
                if results.get(need).is_some_and(|r| r.state.is_pass()) {
                    continue;
                }
                let title = list
                    .iter()
                    .find(|s| &s.id == need)
                    .map_or(need.raw().to_string(), |s| s.title.clone());
                return Some(format!("waits for {title}"));
            }
        }
        if needs_admin {
            if let Some(why) = shell::admin_refusal(m) {
                return Some(why.into());
            }
            match m.admin() {
                AdminAccess::RootSsh if !matches!(admin, AdminAnswer::RootConfirmed) => {
                    return Some(format!(
                        "needs your confirmation to run as root on {}",
                        m.name
                    ))
                }
                AdminAccess::Sudo if !admin.gives_password() => {
                    return Some("needs the admin password".into())
                }
                _ => {}
            }
        }
        None
    }

    pub(super) fn run_fix(
        &self,
        machine: &str,
        id: &StepId,
        admin: &AdminAnswer,
        expected: &str,
    ) -> FixOutcome {
        let Some(m) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        let step = match self.find_step(&m, id) {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let Some(plan) = step.plan().cloned() else {
            return fail(format!("{} has no Fix", step.title));
        };
        if plan.display != expected {
            return fail(format!("{} needs a look; Set up again", step.title));
        }
        if let Some(why) = self.refusal(&step, &m, admin, plan.needs_admin, true, false) {
            return fail(why);
        }
        let used = match self.admin_use(&m, plan.needs_admin, admin) {
            Ok(used) => used,
            Err(why) => return fail(why),
        };
        let outcome = self.perform_with(id.clone(), &step.title, &m, used, |ctx| (plan.run)(ctx));
        self.refresh(Some(&[machine.to_string()]), true);
        outcome
    }

    pub(super) fn run_replace(&self, machine: &str, id: &StepId, expected: &str) -> FixOutcome {
        let Some(m) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        let step = match self.find_step(&m, id) {
            Ok(s) => s,
            Err(e) => return fail(e),
        };
        let Some(plan) = step.replace.clone() else {
            return fail(format!("{machine} has nothing to replace in {}", id.raw()));
        };
        if !(self.catalog.offers_replace)(self.results(machine).get(id)) || plan.display != expected
        {
            return fail(format!(
                "{} needs a look; Check again",
                step.replace_title()
            ));
        }
        if let Some(why) = self.refusal(&step, &m, &AdminAnswer::None, false, true, false) {
            return fail(why);
        }
        let outcome = self.perform(id.clone(), &step.replace_title(), &m, None, |ctx| {
            (plan.run)(ctx)
        });
        self.refresh(Some(&[machine.to_string()]), true);
        outcome
    }

    pub(super) fn perform(
        &self,
        id: StepId,
        title: &str,
        m: &Machine,
        password: Option<Secret>,
        body: impl FnOnce(&FixContext) -> FixOutcome,
    ) -> FixOutcome {
        let used = AdminUse {
            secret: password,
            vault: None,
        };
        self.perform_with(id, title, m, used, body)
    }

    pub(super) fn perform_with(
        &self,
        id: StepId,
        title: &str,
        m: &Machine,
        used: AdminUse,
        body: impl FnOnce(&FixContext) -> FixOutcome,
    ) -> FixOutcome {
        let self_update = id.self_update_tool();
        {
            let mut s = lock(&self.state);
            if s.running.contains_key(&m.name) {
                return fail(format!("another Fix is running on {}", m.name));
            }
            s.running.insert(m.name.clone(), id);
        }
        let AdminUse {
            secret: password,
            vault,
        } = used;
        let masks: Masks = self.masks(m, password.as_ref().map(Secret::expose));
        self.append_log(&m.name, vec![format!("▸ {title}")]);
        self.changed();
        let log = Arc::new(FixLog::new(masks.clone()));
        let state = self.state.clone();
        let name = m.name.clone();
        let sink_log = log.clone();
        let sink: Arc<OutputSink> = Arc::new(move |c: Channel, t: &str| {
            sink_log.add(c, t);
            append(&state, &name, sink_log.take());
        });
        let links = self.link_offer(&m.name);
        let ctx = FixContext {
            step: self.context(m, &masks),
            admin_password: password,
            log: sink,
            masks: masks.clone(),
            links: Some(links),
        };
        let raw = body(&ctx);
        drop(ctx);
        lock(&self.state).sign_in.remove(&m.name);
        log.end();
        self.append_log(&m.name, log.take());
        let message = masks.apply(&raw.message);
        let mut lines: Vec<String> = message.split('\n').map(str::to_string).collect();
        if let Some(first) = lines.first_mut() {
            *first = format!("{} {first}", if raw.ok { "✓" } else { "✗" });
        }
        self.append_log(&m.name, lines);
        {
            let mut s = lock(&self.state);
            s.running.remove(&m.name);
            *s.fixes_ended.entry(m.name.clone()).or_default() += 1;
        }
        self.audit(&m.name, title, raw.ok, vault.as_deref());
        if let Some(tool) = self_update {
            self.log_self_update(&m.name, tool, title, raw.ok, &message);
        }
        self.changed();
        FixOutcome {
            ok: raw.ok,
            message,
        }
    }

    fn link_offer(&self, machine: &str) -> LinkOffer {
        let state = self.state.clone();
        let name = machine.to_string();
        let notify = self
            .on_link
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        Arc::new(move |url: &str| {
            if !is_sign_in_link(url) {
                return;
            }
            {
                let mut s = lock(&state);
                s.sign_in.insert(name.clone(), url.to_string());
                s.revision += 1;
            }
            if let Some(notify) = &notify {
                notify();
            }
        })
    }

    pub(super) fn set_up_all_steps(&self, m: &Machine) -> Vec<ApprovedStep> {
        let results = self.results(&m.name);
        self.step_list(m)
            .into_iter()
            .filter_map(|step| {
                let plan = step.plan()?;
                let state = &results.get(&step.id)?.state;
                let waiting = if state.is_fail() {
                    false
                } else if state.is_waiting() {
                    true
                } else {
                    return None;
                };
                Some(ApprovedStep {
                    id: step.id.clone(),
                    title: step.title.clone(),
                    display: plan.display.clone(),
                    waiting,
                    needs_admin: plan.needs_admin,
                })
            })
            .collect()
    }

    pub(super) fn set_up_all(
        &self,
        machine: &str,
        approved: &[ApprovedStep],
        ask_admin: &dyn Fn(&ApprovedStep) -> AdminAnswer,
    ) -> FixOutcome {
        let Some(first) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        if self.running(machine).is_some() {
            return fail(format!("another Fix is running on {machine}"));
        }
        let mut fixed = 0;
        for id in self.step_list(&first).into_iter().map(|s| s.id) {
            let Some(m) = self.machine(machine) else {
                break;
            };
            let Ok(step) = self.find_step(&m, &id) else {
                continue;
            };
            let Some(plan) = step.plan().cloned() else {
                continue;
            };
            if !self
                .results(machine)
                .get(&id)
                .is_some_and(|r| r.state.is_fail())
            {
                continue;
            }
            let Some(shown) = approved
                .iter()
                .find(|a| a.id == id && a.display == plan.display)
            else {
                return fail(format!("{} needs a look; Set up again", step.title));
            };
            let mut answer = AdminAnswer::None;
            if plan.needs_admin {
                if let Some(why) = shell::admin_refusal(&m) {
                    return fail(format!("{}: {why}", step.title));
                }
                answer = self.answer_for(&m, shown, ask_admin);
                if matches!(answer, AdminAnswer::None) {
                    return fail(format!(
                        "Set up all stopped before {}: cancelled",
                        step.title
                    ));
                }
            }
            let outcome = self.run_fix(machine, &id, &answer, &plan.display);
            drop(answer);
            if !outcome.ok {
                return fail(format!("{}: {}", step.title, outcome.message));
            }
            fixed += 1;
        }
        let done = match fixed {
            0 => "nothing to set up".to_string(),
            1 => "1 step set up".to_string(),
            n => format!("{n} steps set up"),
        };
        let Some(last) = self.machine(machine) else {
            return FixOutcome::ok(done);
        };
        let results = self.results(machine);
        let remaining: Vec<String> = self
            .step_list(&last)
            .into_iter()
            .filter(|s| {
                s.id != StepId::RUNNING && !results.get(&s.id).is_some_and(|r| r.state.is_pass())
            })
            .map(|s| s.title)
            .collect();
        if remaining.is_empty() {
            return FixOutcome::ok(done);
        }
        let mut shown = remaining
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        if remaining.len() > 4 {
            shown.push_str(&format!(" and {} more", remaining.len() - 4));
        }
        fail(format!("{done}; still not ✓: {shown}"))
    }

    pub fn turn_on_blocker(&self, machine: &str) -> Option<String> {
        let m = self.machine(machine)?;
        let results = self.results(machine);
        for step in self.step_list(&m) {
            if step.id == StepId::RUNNING {
                return None;
            }
            if step.id.is_node() {
                continue;
            }
            if !results.get(&step.id).is_some_and(|r| r.state.is_pass()) {
                return Some(step.title);
            }
        }
        None
    }

    pub(super) fn toggle_running(
        &self,
        machine: &str,
        on: bool,
        admin: &AdminAnswer,
        expected: &str,
    ) -> FixOutcome {
        let Some(m) = self.machine(machine) else {
            return fail(format!("no machine named {machine}"));
        };
        let Ok(step) = self.find_step(&m, &StepId::RUNNING) else {
            return fail(format!("{machine} has no Running switch"));
        };
        let StepFix::Toggle { needs_admin } = step.fix else {
            return fail(format!("{machine} has no Running switch"));
        };
        if let Some(why) = self.refusal(&step, &m, admin, needs_admin, false, on) {
            return fail(why);
        }
        let title = if on {
            "Turn Running on"
        } else {
            "Turn Running off"
        };
        let script = (self.catalog.toggle_script)(m.is_linux(), on);
        if script != expected {
            return fail(format!("{title} needs a look; Set up again"));
        }
        let used = match self.admin_use(&m, needs_admin, admin) {
            Ok(used) => used,
            Err(why) => return fail(why),
        };
        let outcome = self.perform_with(StepId::RUNNING, title, &m, used, |ctx| {
            let r = if needs_admin {
                run::admin(ctx, &script, 900)
            } else {
                run::script(ctx, &script, None, 600)
            };
            if r.code == 0 {
                return FixOutcome::ok(if on {
                    "Running is on"
                } else {
                    "Running is off"
                });
            }
            let text = if r.err.is_empty() { &r.out } else { &r.err }
                .trim()
                .to_string();
            fail(if text.is_empty() {
                format!("exited {}", r.code)
            } else {
                ctx.mask(&text)
            })
        });
        self.refresh(Some(&[machine.to_string()]), false);
        outcome
    }

    pub fn forget(&self, machine: &str) {
        {
            let mut s = lock(&self.state);
            s.results.remove(machine);
            s.logs.remove(machine);
            s.last_slow.remove(machine);
            s.facts.remove(machine);
            s.tools_known.remove(machine);
            s.sign_in.remove(machine);
            s.saved_state.remove(machine);
            s.expiry_sent.remove(machine);
            s.shown.retain(|_, r| r.plan.machine != machine);
            for usages in s.offers.values_mut() {
                usages.retain(|u| u.machine != machine);
            }
            s.offers.retain(|_, u| !u.is_empty());
        }
        self.save_offers();
        self.changed();
    }
}

pub(super) fn append(
    state: &std::sync::Mutex<super::engine::State>,
    machine: &str,
    lines: Vec<String>,
) {
    if lines.is_empty() {
        return;
    }
    let mut s = lock(state);
    let log = s.logs.entry(machine.to_string()).or_default();
    log.extend(lines);
    if log.len() > super::engine::LOG_LIMIT {
        let cut = log.len() - super::engine::LOG_LIMIT;
        log.drain(..cut);
    }
    s.revision += 1;
}

impl SetupEngine {
    fn log_self_update(&self, machine: &str, tool: Tool, title: &str, ok: bool, message: &str) {
        let Some(data) = &self.deps.offers_data else {
            return;
        };
        let action = if title.starts_with("Self-Update On") {
            "on"
        } else {
            "off"
        };
        let at_ms = ((self.deps.clock)() * 1000.0) as i64;
        let _ = selfupdate_log::append(data, at_ms, tool, machine, action, ok, message);
    }

    fn audit(&self, machine: &str, title: &str, ok: bool, vault: Option<&str>) {
        let Some(path) = &self.deps.audit else {
            return;
        };
        let mut line = serde_json::json!({
            "time": (self.deps.clock)(),
            "op": "machines.fix",
            "machine": machine,
            "fix": title,
            "ok": ok,
        });
        if let (Some(entry), Some(map)) = (vault, line.as_object_mut()) {
            map.insert("vault".into(), serde_json::json!([entry]));
        }
        if let Some(dir) = path.parent() {
            let _ = notchling_platform::files::ensure_private_dir(dir);
        }
        let mut options = std::fs::OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        }
        if let Ok(mut f) = options.open(path) {
            use std::io::Write;
            let _ = writeln!(f, "{line}");
        }
    }
}
