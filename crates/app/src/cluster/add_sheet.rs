use std::fmt;

use notchling_engine::accounts::Secret;
use notchling_engine::machines::address;
use notchling_engine::machines::addresses::MachineAddress;
use notchling_engine::machines::engine_plan::AdminAnswer;
use notchling_engine::machines::forms::machine_problems;
use notchling_engine::machines::registry::MachineOs;
use notchling_engine::machines::steps_tailscale_linux::is_sign_in_link;
use notchling_locale as t;

use super::add::{Phase, Stop};
use super::add_plan::PlanView;
use super::add_runs::{SignInLink, Snap};
use super::addresses_form::{addresses_of, split_row, RowDraft, RowMeta};
use super::machine_add::Phase as MachinePhase;
use super::not_joined::NotJoined;

#[derive(Default)]
pub struct AddPassword(String);

impl AddPassword {
    pub fn field(&mut self) -> &mut String {
        &mut self.0
    }

    pub fn len(&self) -> usize {
        self.0.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn take(&mut self) -> Option<Secret> {
        let typed = std::mem::take(&mut self.0);
        (!typed.is_empty()).then(|| Secret::new(typed))
    }

    pub fn secret(&self) -> Option<Secret> {
        (!self.0.is_empty()).then(|| Secret::new(self.0.clone()))
    }

    pub fn dots(&self) -> String {
        "•".repeat(self.len())
    }
}

impl fmt::Debug for AddPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("<hidden>")
    }
}

pub fn install(
    password: &mut AddPassword,
    run: impl FnOnce(Option<Secret>) -> Result<(), String>,
) -> Result<(), String> {
    let started = run(password.secret());
    if started.is_ok() {
        password.clear();
    }
    started
}

pub fn may_open(busy: bool, sheet_open: bool) -> bool {
    !busy && !sheet_open
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Add,
    CheckAgain,
    Install,
    OpenSignIn,
    FromTailscale,
    Close,
    Cancel,
    Done,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Action {
    pub button: Button,
    pub enabled: bool,
}

impl Action {
    fn on(button: Button) -> Action {
        Action {
            button,
            enabled: true,
        }
    }

    fn when(button: Button, enabled: bool) -> Action {
        Action { button, enabled }
    }
}

pub fn sign_in_target(snap: &Snap) -> Option<&str> {
    snap.sign_in
        .as_ref()
        .map(SignInLink::url)
        .filter(|url| is_sign_in_link(url))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Draft {
    pub address: String,
    pub name: String,
    pub first: RowMeta,
    pub more: Vec<RowDraft>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Parsed {
    pub target: String,
    pub name: String,
    pub addresses: Vec<MachineAddress>,
}

fn without_port(typed: &str) -> String {
    let Ok(row) = split_row(typed) else {
        return typed.to_owned();
    };
    if row.port.is_none() {
        return typed.trim().to_owned();
    }
    let host = if row.host.contains(':') {
        format!("[{}]", row.host)
    } else {
        row.host
    };
    match row.user {
        Some(user) => format!("{user}@{host}"),
        None => host,
    }
}

pub fn derived_name(typed: &str, user: &str) -> String {
    address::parse(&without_port(typed), user, &[])
        .map(|at| at.name)
        .unwrap_or_default()
}

pub fn parse_draft(draft: &Draft, user: &str, existing: &[String]) -> Result<Parsed, String> {
    let at = address::parse(&without_port(&draft.address), user, &[])?;
    let typed = draft.name.trim();
    let name = if typed.is_empty() {
        at.name.clone()
    } else {
        typed.to_owned()
    };
    if let Some(first) = machine_problems(&name, &at.host, &at.user, existing).first() {
        return Err(first.clone());
    }
    let plain = draft.more.is_empty()
        && draft.first.is_plain()
        && split_row(&draft.address).is_ok_and(|r| r.port.is_none());
    let addresses = if plain {
        Vec::new()
    } else {
        let mut rows = vec![RowDraft {
            text: draft.address.clone(),
            meta: draft.first.clone(),
        }];
        rows.extend(
            draft
                .more
                .iter()
                .filter(|r| !r.text.trim().is_empty())
                .cloned(),
        );
        addresses_of(&rows, &name, &at.user)?
    };
    Ok(Parsed {
        target: format!("{}@{}", at.user, at.host),
        name,
        addresses,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub struct AddView {
    pub title: String,
    pub form: bool,
    pub target: Option<String>,
    pub error: Option<String>,
    pub busy: Option<String>,
    pub plan: Option<PlanView>,
    pub plan_intro: Option<String>,
    pub step: Option<String>,
    pub step_title: Option<String>,
    pub log: Vec<String>,
    pub stop: Option<Stop>,
    pub done: Option<String>,
    pub note: Option<String>,
    pub actions: Vec<Action>,
}

impl AddView {
    fn empty(title: String) -> AddView {
        AddView {
            title,
            form: false,
            target: None,
            error: None,
            busy: None,
            plan: None,
            plan_intro: None,
            step: None,
            step_title: None,
            log: Vec::new(),
            stop: None,
            done: None,
            note: None,
            actions: Vec::new(),
        }
    }

    pub fn allows(&self, button: Button) -> bool {
        self.actions.iter().any(|a| a.button == button && a.enabled)
    }

    pub fn can_install(&self) -> bool {
        self.allows(Button::Install)
    }
}

pub fn view(
    snap: Option<&Snap>,
    draft: &Draft,
    password_len: usize,
    user: &str,
    existing: &[String],
) -> AddView {
    let Some(snap) = snap else {
        return machine_view(&MachinePhase::Form, draft, user, existing);
    };
    let mut view = AddView::empty(t::cluster::add_title_named(&snap.name));
    let target = format!("{}@{}", snap.user, snap.host);
    match &snap.phase {
        Phase::Address => {
            view.form = true;
            view.actions = vec![Action::on(Button::Cancel)];
        }
        Phase::Checking => {
            view.busy = Some(t::cluster::add_checking(&target));
            view.actions = vec![Action::on(Button::Close)];
        }
        Phase::Plan => {
            view.plan_intro = Some(t::cluster::add_plan_intro(&snap.name));
            let ready = snap.plan.as_ref().is_some_and(|p| {
                p.blocked.is_none()
                    && (!p.needs_password
                        || AdminAnswer::ready(password_len, p.admin_entry.as_deref()))
            });
            view.plan = snap.plan.clone();
            view.actions = vec![
                Action::on(Button::Cancel),
                Action::on(Button::CheckAgain),
                Action::when(Button::Install, ready),
            ];
        }
        Phase::Running { step, of, title } => {
            view.step = Some(t::cluster::add_running(&snap.name, *step, *of));
            view.step_title = Some(title.clone());
            view.log = snap.log.clone();
            view.note = Some(t::cluster::add_running_note().to_owned());
            view.actions = vec![Action::on(Button::Close)];
            if sign_in_target(snap).is_some() {
                view.actions.insert(0, Action::on(Button::OpenSignIn));
            }
        }
        Phase::Joining { .. } => {
            view.busy = Some(t::cluster::add_joining(&snap.name));
            view.log = snap.log.clone();
            view.note = Some(t::cluster::add_running_note().to_owned());
            view.actions = vec![Action::on(Button::Close)];
        }
        Phase::Done => {
            view.done = Some(t::cluster::add_done(&snap.name));
            view.actions = vec![Action::on(Button::Done)];
        }
        Phase::Stopped(stop) => {
            view.stop = Some(stop.clone());
            view.log = snap.log.clone();
            view.actions = vec![Action::on(Button::Close)];
            if stop.check_again {
                view.actions.insert(0, Action::on(Button::CheckAgain));
            }
        }
    }
    view
}

pub fn refused_view() -> AddView {
    let mut view = AddView::empty(t::cluster::add_title().to_owned());
    view.actions = vec![Action::on(Button::Close)];
    view
}

pub fn machine_view(
    phase: &MachinePhase,
    draft: &Draft,
    user: &str,
    existing: &[String],
) -> AddView {
    let mut view = AddView::empty(t::cluster::add_title().to_owned());
    let parsed = parse_draft(draft, user, existing);
    match phase {
        MachinePhase::Form => {
            view.form = true;
            match &parsed {
                Ok(ok) => view.target = Some(t::cluster::add_connects_as(&ok.target)),
                Err(why) if !draft.address.trim().is_empty() => view.error = Some(why.clone()),
                Err(_) => {}
            }
            view.actions = vec![
                Action::on(Button::Cancel),
                Action::on(Button::FromTailscale),
                Action::when(Button::Add, parsed.is_ok()),
            ];
        }
        MachinePhase::Checking => {
            let target = parsed.map(|p| p.target).unwrap_or_default();
            view.busy = Some(t::cluster::add_checking(&target));
            view.actions = vec![Action::on(Button::Close)];
        }
        MachinePhase::Saved { name, note } => {
            view.done = Some(t::cluster::add_saved(name));
            view.note = note.clone();
            view.actions = vec![Action::on(Button::Done)];
        }
        MachinePhase::Stopped(stop) => {
            view.stop = Some(stop.clone());
            view.actions = vec![Action::on(Button::Close)];
            if stop.check_again {
                view.actions.insert(0, Action::on(Button::CheckAgain));
            }
        }
    }
    view
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Brief {
    pub name: String,
    pub text: String,
    pub problem: bool,
    pub dismiss: bool,
}

pub fn brief(name: &str, phase: &Phase) -> Option<Brief> {
    let (text, problem, dismiss) = match phase {
        Phase::Address => return None,
        Phase::Checking => (t::cluster::add_row_checking(name), false, false),
        Phase::Plan => (t::cluster::add_row_waiting(name), false, true),
        Phase::Running { step, of, .. } => {
            (t::cluster::add_running(name, *step, *of), false, false)
        }
        Phase::Joining { .. } => (t::cluster::add_joining(name), false, false),
        Phase::Done => (t::cluster::add_done(name), false, true),
        Phase::Stopped(stop) => (t::cluster::add_row_stopped(name, &stop.reason), true, true),
    };
    Some(Brief {
        name: name.to_owned(),
        text,
        problem,
        dismiss,
    })
}

pub fn not_joined_line(machine: &NotJoined) -> String {
    let target = format!("{}@{}", machine.user, machine.host);
    match machine.os {
        Some(MachineOs::MacOs) => t::cluster::not_joined_line(&target, t::cluster::os_macos()),
        Some(MachineOs::Linux) => t::cluster::not_joined_line(&target, t::cluster::os_linux()),
        None => t::cluster::not_joined_line_unknown(&target),
    }
}
