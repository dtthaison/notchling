use std::collections::{BTreeMap, BTreeSet};

use notchling_engine::machines::dist_manifest::DistManifest;
use notchling_engine::machines::engine::SetupEngine;
use notchling_engine::machines::engine_plan::{AdminAnswer, PlanKind, ShownPlan};
use notchling_engine::machines::node_updates::{
    offer, runnable, MemberBuild, UpdateRow, UpdateState,
};
use notchling_engine::machines::registry::Machine;
use notchling_engine::machines::step::FixOutcome;
use notchling_locale as t;

use super::model::{ClusterModel, NodeRow};
use crate::machines::runs::AdminAsks;

fn machine_of<'a>(
    row: &NodeRow,
    machines: &'a [Machine],
    node_of: &dyn Fn(&str) -> Option<String>,
) -> Option<&'a Machine> {
    machines
        .iter()
        .find(|m| node_of(&m.name).as_deref() == Some(row.id.as_str()))
        .or_else(|| machines.iter().find(|m| m.name == row.name))
}

pub fn is_moving(model: &ClusterModel, row: &NodeRow) -> bool {
    let target = model
        .move_status()
        .filter(|m| m.running())
        .map(|m| m.to.as_str());
    let in_move = model.move_running() && (row.primary || target == Some(row.id.as_str()));
    let in_promote = model.promote_running() && (row.primary || row.me);
    in_move || in_promote
}

pub fn members(
    model: &ClusterModel,
    machines: &[Machine],
    node_of: &dyn Fn(&str) -> Option<String>,
) -> Vec<MemberBuild> {
    model
        .rows()
        .iter()
        .filter(|row| row.active && !row.removed && !row.me)
        .map(|row| {
            let name = machine_of(row, machines, node_of)
                .map(|m| m.name.clone())
                .unwrap_or_else(|| row.name.clone());
            let (os, arch) = row
                .versions
                .as_ref()
                .map(|v| (v.os.clone(), v.arch.clone()))
                .unwrap_or_default();
            MemberBuild {
                node: row.id.clone(),
                name,
                os,
                arch,
                build: row.build.clone(),
                running_shell_jobs: model.running_shell_jobs(&row.id),
                moving: is_moving(model, row),
            }
        })
        .collect()
}

pub fn rows_for(
    this_build: Option<&str>,
    model: &ClusterModel,
    machines: &[Machine],
    node_of: &dyn Fn(&str) -> Option<String>,
    manifest: Option<&DistManifest>,
    failed_binary: &BTreeSet<String>,
) -> Vec<UpdateRow> {
    let members = members(model, machines, node_of);
    offer(this_build, &members, machines, manifest, failed_binary)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NotNow(Option<Option<String>>);

impl NotNow {
    pub fn hide(&mut self, this_build: Option<&str>) {
        self.0 = Some(this_build.map(str::to_owned));
    }

    pub fn hides(&self, this_build: Option<&str>) -> bool {
        self.0
            .as_ref()
            .is_some_and(|seen| seen.as_deref() == this_build)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub text: String,
    pub button: String,
    pub count: usize,
}

pub fn banner(rows: &[UpdateRow]) -> Option<Banner> {
    if rows.is_empty() {
        return None;
    }
    let names: Vec<&str> = rows.iter().map(|r| r.machine.as_str()).collect();
    Some(Banner {
        text: t::cluster::update_banner(rows.len(), &names.join(", ")),
        button: t::cluster::update_button_open(rows.len()),
        count: rows.len(),
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ready,
    Skipped,
    Busy,
    Chosen,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub machine: String,
    pub text: String,
    pub kind: Kind,
}

impl Line {
    pub fn offers_anyway(&self) -> bool {
        matches!(self.kind, Kind::Busy)
    }
}

pub fn lines(rows: &[UpdateRow], anyway: &BTreeSet<String>) -> Vec<Line> {
    rows.iter()
        .map(|row| {
            let (text, kind) = match &row.state {
                UpdateState::Ready => (t::cluster::update_ready().to_owned(), Kind::Ready),
                UpdateState::NeedsBuild(why) => (why.clone(), Kind::Skipped),
                UpdateState::Busy(why) if anyway.contains(&row.machine) => {
                    (t::cluster::update_anyway_chosen(why), Kind::Chosen)
                }
                UpdateState::Busy(why) => (why.clone(), Kind::Busy),
                UpdateState::NotInMachines => (
                    t::cluster::update_not_in_machines(&row.machine),
                    Kind::Skipped,
                ),
            };
            Line {
                machine: row.machine.clone(),
                text,
                kind,
            }
        })
        .collect()
}

pub fn run_button(rows: &[UpdateRow], anyway: &BTreeSet<String>) -> (String, bool) {
    let count = runnable(rows, &anyway.iter().cloned().collect::<Vec<_>>()).len();
    (t::cluster::update_button_run(count), count > 0)
}

pub fn plan(
    engine: &SetupEngine,
    rows: &[UpdateRow],
    anyway: &BTreeSet<String>,
) -> Result<ShownPlan, String> {
    let names = runnable(rows, &anyway.iter().cloned().collect::<Vec<_>>());
    if names.is_empty() {
        return Err(t::cluster::update_nothing().to_owned());
    }
    engine.plan("", PlanKind::UpdateNotchling(names))
}

pub fn commands(plan: &ShownPlan) -> BTreeMap<String, String> {
    plan.steps
        .iter()
        .map(|s| (s.title.clone(), s.display.clone()))
        .collect()
}

pub fn execute(engine: &SetupEngine, plan: &ShownPlan, asks: &AdminAsks) -> FixOutcome {
    if let PlanKind::UpdateNotchling(names) = &plan.kind {
        engine.refresh(Some(names), true);
    }
    match engine.confirm(plan.id, AdminAnswer::None) {
        Ok(confirmed) => engine.run_with(confirmed, &|step| asks.ask(&step.title, step)),
        Err(why) => FixOutcome::fail(why),
    }
}
