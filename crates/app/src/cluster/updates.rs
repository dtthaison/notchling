use std::collections::{BTreeMap, BTreeSet};

use notchling_engine::accounts::Secret;
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

pub(super) fn machine_of<'a>(
    row: &NodeRow,
    rows: &[NodeRow],
    machines: &'a [Machine],
    node_of: &dyn Fn(&str) -> Option<String>,
) -> Option<&'a Machine> {
    if let Some(bound) = machines
        .iter()
        .find(|m| node_of(&m.name).as_deref() == Some(row.id.as_str()))
    {
        return Some(bound);
    }
    let mut named = machines.iter().filter(|m| m.name == row.name);
    let (Some(only), None) = (named.next(), named.next()) else {
        return None;
    };
    let alone = rows.iter().filter(|r| r.name == row.name).count() == 1;
    (alone && node_of(&only.name).is_none()).then_some(only)
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
    let all = model.rows();
    all.iter()
        .filter(|row| row.active && !row.removed && !row.me)
        .map(|row| {
            let name = machine_of(row, &all, machines, node_of)
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
    can_download: bool,
) -> Vec<UpdateRow> {
    let members = members(model, machines, node_of);
    offer(
        this_build,
        &members,
        machines,
        manifest,
        failed_binary,
        can_download,
    )
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
    Idle,
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

pub fn idle_lines(model: &ClusterModel, rows: &[UpdateRow]) -> Vec<Line> {
    model
        .rows()
        .iter()
        .filter(|row| row.active && !row.removed)
        .filter(|row| row.me || !rows.iter().any(|r| r.machine == row.name))
        .map(|row| Line {
            machine: row.name.clone(),
            text: if row.me {
                t::cluster::update_this_mac().to_owned()
            } else {
                t::cluster::update_up_to_date().to_owned()
            },
            kind: Kind::Idle,
        })
        .collect()
}

pub fn selected(
    rows: &[UpdateRow],
    anyway: &BTreeSet<String>,
    deselected: &BTreeSet<String>,
) -> Vec<String> {
    runnable(rows, &anyway.iter().cloned().collect::<Vec<_>>())
        .into_iter()
        .filter(|name| !deselected.contains(name))
        .collect()
}

pub fn run_label(names: &[String]) -> String {
    t::cluster::update_button_run(names.len(), names.first().map_or("", String::as_str))
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

pub fn machines_of(plan: &ShownPlan) -> BTreeMap<String, String> {
    match &plan.kind {
        PlanKind::UpdateNotchling(names) => plan
            .steps
            .iter()
            .zip(names)
            .map(|(step, name)| (step.title.clone(), name.clone()))
            .collect(),
        _ => BTreeMap::new(),
    }
}

pub fn execute(
    engine: &SetupEngine,
    plan: &ShownPlan,
    typed: &BTreeMap<String, Secret>,
) -> FixOutcome {
    if let PlanKind::UpdateNotchling(names) = &plan.kind {
        engine.refresh(Some(names), true);
    }
    let machines = machines_of(plan);
    match engine.confirm(plan.id, AdminAnswer::None) {
        Ok(confirmed) => engine.run_with(confirmed, &|step| {
            let machine = machines.get(&step.title).unwrap_or(&step.title);
            match typed.get(machine) {
                Some(secret) => AdminAnswer::Password(secret.clone()),
                None => AdminAnswer::None,
            }
        }),
        Err(why) => FixOutcome::fail(why),
    }
}
