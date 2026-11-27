use super::engine::SetupEngine;
use super::engine_plan::{AdminAnswer, AdminNeed, ApprovedStep, PlanId, PlanKind, ShownPlan};
use super::shell;
use super::step::{FixOutcome, StepFix, StepId};
use std::collections::BTreeSet;

fn fail(m: impl Into<String>) -> FixOutcome {
    FixOutcome::fail(m)
}

impl SetupEngine {
    pub fn binary_failures(&self) -> BTreeSet<String> {
        self.current_machines()
            .into_iter()
            .filter(|m| {
                self.results(&m.name)
                    .get(&StepId::NODE_BINARY)
                    .is_some_and(|r| r.state.is_fail())
            })
            .map(|m| m.name)
            .collect()
    }

    pub(super) fn update_notchling_plan(&self, names: &[String]) -> Result<ShownPlan, String> {
        if names.is_empty() {
            return Err("no machine to update".into());
        }
        let mut seen = BTreeSet::new();
        let mut steps = Vec::new();
        for name in names {
            if !seen.insert(name.clone()) {
                return Err(format!("{name} is in the update twice"));
            }
            let m = self
                .machine(name)
                .ok_or(format!("no machine named {name}"))?;
            let step = self.find_step(&m, &StepId::NODE_BINARY)?;
            let StepFix::Run(plan) = &step.fix else {
                return Err(format!("{name} has no Notchling Fix to run"));
            };
            if plan.needs_admin {
                if let Some(why) = shell::admin_refusal(&m) {
                    return Err(format!("{name}: {why}"));
                }
            }
            steps.push(ApprovedStep {
                id: StepId::NODE_BINARY,
                title: format!("Update Notchling on {name}"),
                display: plan.display.clone(),
                waiting: false,
                needs_admin: plan.needs_admin,
            });
        }
        Ok(ShownPlan {
            id: PlanId(0),
            machine: String::new(),
            kind: PlanKind::UpdateNotchling(names.to_vec()),
            title: format!("Update Notchling on {} machine(s)", steps.len()),
            steps,
            admin: AdminNeed::None,
        })
    }

    fn failing_after_update(&self, name: &str) -> Option<String> {
        let m = self.machine(name)?;
        let results = self.results(name);
        [StepId::NODE_BINARY, StepId::NODE_SERVICE]
            .iter()
            .find_map(|id| {
                let step = self.find_step(&m, id).ok()?;
                match results.get(id) {
                    Some(r) if r.state.is_pass() => None,
                    Some(r) => Some(format!("{}: {}", step.title, r.state.text())),
                    None => Some(format!("{}: not checked after the update", step.title)),
                }
            })
    }

    pub(super) fn update_notchling(
        &self,
        names: &[String],
        approved: &[ApprovedStep],
        ask_admin: &dyn Fn(&ApprovedStep) -> AdminAnswer,
    ) -> FixOutcome {
        if names.len() != approved.len() {
            return fail("the update needs a look; Set up again");
        }
        let mut done = 0;
        for (name, shown) in names.iter().zip(approved) {
            let mut answer = AdminAnswer::None;
            if shown.needs_admin {
                answer = match self.machine(name) {
                    Some(m) => self.answer_for(&m, shown, ask_admin),
                    None => ask_admin(shown),
                };
                if matches!(answer, AdminAnswer::None) {
                    return fail(format!(
                        "The update stopped before {name}: cancelled; {done} machine(s) updated"
                    ));
                }
            }
            let outcome = self.run_fix(name, &StepId::NODE_BINARY, &answer, &shown.display);
            drop(answer);
            if !outcome.ok {
                return fail(format!(
                    "{name}: {}; {done} machine(s) updated before it",
                    outcome.message
                ));
            }
            if let Some(line) = self.failing_after_update(name) {
                return fail(format!(
                    "{name}: {line}; {done} machine(s) updated before it"
                ));
            }
            done += 1;
        }
        FixOutcome::ok(format!("Notchling updated on {done} machine(s)"))
    }
}
