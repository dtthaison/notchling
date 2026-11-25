use std::collections::HashSet;

use notchling_engine::accounts::Secret;
use notchling_locale as t;
use serde_json::{json, Value};

pub const MACHINES_APPLY: &str = "machines.apply";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Machine,
    Update,
    Install,
    Cost,
    Model,
}

impl Family {
    pub fn of(view: &Value) -> Option<Family> {
        match view.get("kind").and_then(Value::as_str)? {
            "machine" => Some(Family::Machine),
            "update" => Some(Family::Update),
            "install" => Some(Family::Install),
            "cost_profile" => Some(Family::Cost),
            "model" => Some(Family::Model),
            _ => None,
        }
    }

    pub fn has_parts(self) -> bool {
        matches!(self, Family::Install | Family::Cost)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Field {
    pub name: String,
    pub before: String,
    pub after: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Part {
    pub id: String,
    pub path: String,
    pub tool: String,
    pub change: String,
    pub added: i64,
    pub removed: i64,
    pub before: Option<String>,
    pub after: Option<String>,
    pub notes: Vec<String>,
    pub fields: Vec<Field>,
    pub also: Vec<String>,
    pub kept: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct FamilyApply {
    pub op: String,
    pub args: Value,
    pub family: Family,
    pub admin: bool,
    pub admin_entry: Option<String>,
    pub refused: bool,
}

#[derive(Clone, PartialEq, Eq)]
pub enum Admin {
    Password(Secret),
    Vault(String),
}

impl std::fmt::Debug for Admin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Admin::Password(_) => f.write_str("Admin::Password(..)"),
            Admin::Vault(name) => write!(f, "Admin::Vault({name})"),
        }
    }
}

fn text(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn shown(value: Option<&Value>) -> String {
    match value {
        Some(Value::String(s)) => s.clone(),
        None | Some(Value::Null) => t::consent::card_unset().to_owned(),
        Some(other) => other.to_string(),
    }
}

fn field_of(value: &Value) -> Option<Field> {
    Some(Field {
        name: text(value, "field")?,
        before: shown(value.get("before")),
        after: shown(value.get("after")),
    })
}

pub fn parts_of(view: &Value, unticked: &HashSet<String>) -> Vec<Part> {
    view.get("parts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|part| {
            let id = text(part, "id")?;
            Some(Part {
                kept: !unticked.contains(&id),
                path: text(part, "path").unwrap_or_default(),
                tool: text(part, "tool").unwrap_or_default(),
                change: text(part, "change").unwrap_or_default(),
                added: part.get("added").and_then(Value::as_i64).unwrap_or(0),
                removed: part.get("removed").and_then(Value::as_i64).unwrap_or(0),
                before: text(part, "before"),
                after: text(part, "after"),
                notes: part
                    .get("notes")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|n| n.as_str().map(str::to_owned))
                    .collect(),
                fields: part
                    .get("fields")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(field_of)
                    .collect(),
                also: part
                    .get("also")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|other| text(other, "path"))
                    .collect(),
                id,
            })
        })
        .collect()
}

pub fn family_apply(view: &Value) -> Option<FamilyApply> {
    let family = Family::of(view)?;
    let change = view.get("change")?;
    let apply = change.get("apply")?;
    let args = apply.get("args").filter(|a| a.is_object())?.clone();
    let need = change.get("admin").and_then(Value::as_str);
    Some(FamilyApply {
        op: text(apply, "op")?,
        args,
        family,
        admin: need == Some("password") && family == Family::Machine,
        admin_entry: text(change, "adminEntry").filter(|_| family == Family::Machine),
        refused: need == Some("refused"),
    })
}

pub fn kept_ids(parts: &[Part]) -> Value {
    Value::Array(
        parts
            .iter()
            .filter(|p| p.kept)
            .map(|p| Value::String(p.id.clone()))
            .collect(),
    )
}

pub fn kept_count(parts: &[Part]) -> usize {
    parts.iter().filter(|p| p.kept).count()
}

impl FamilyApply {
    pub fn request(&self, id: &str, parts: &[Part], admin: Option<&Admin>) -> Value {
        let mut out = self.args.clone();
        let Some(map) = out.as_object_mut() else {
            return out;
        };
        map.insert("consentId".into(), Value::String(id.to_owned()));
        if self.family.has_parts() {
            map.insert("parts".into(), kept_ids(parts));
        }
        if self.op == MACHINES_APPLY {
            match admin {
                Some(Admin::Password(secret)) => {
                    map.insert("admin".into(), json!({"password": secret.expose()}));
                }
                Some(Admin::Vault(name)) => {
                    map.insert("admin".into(), json!({"vault": name}));
                }
                None => {}
            }
        }
        out
    }
}

pub fn toggled(unticked: &mut HashSet<String>, id: &str) {
    if !unticked.remove(id) {
        unticked.insert(id.to_owned());
    }
}

fn names(args: &Value, key: &str) -> Option<String> {
    match args.get(key)? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Array(list) => {
            let joined: Vec<&str> = list.iter().filter_map(Value::as_str).collect();
            (!joined.is_empty()).then(|| joined.join(", "))
        }
        _ => None,
    }
}

fn arg(args: &Value, key: &str) -> Option<String> {
    text(args, key)
}

fn count(args: &Value) -> Option<u64> {
    args.get("count").and_then(Value::as_u64)
}

fn cost_tool(key: &str) -> &'static str {
    super::consent_card::tool_name(if key == "agy" { "antigravity" } else { key })
}

fn need_word(need: &str) -> &'static str {
    match need {
        "password" => t::consent::machine_need_password(),
        "root" => t::consent::machine_need_root(),
        "refused" => t::consent::machine_need_refused(),
        _ => t::consent::machine_need_none(),
    }
}

fn drops_line(args: &Value) -> Option<String> {
    let mut out = t::consent::network_drops(args.get("count").and_then(Value::as_u64)?);
    for group in args
        .get("groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        if let (Some(organization), Some(machines)) =
            (arg(group, "organization"), names(group, "machines"))
        {
            out = format!(
                "{out}. {}",
                t::consent::network_drops_in(&organization, &machines)
            );
        }
    }
    Some(out)
}

fn machine_line(args: &Value) -> Option<String> {
    let mut out = t::consent::machine_named(
        &arg(args, "machine")?,
        &arg(args, "user")?,
        &arg(args, "host")?,
        &arg(args, "organization")?,
    );
    if let Some(addresses) = names(args, "addresses") {
        out = format!("{out}. {}", t::consent::machine_address_list(&addresses));
    }
    if let Some(key) = arg(args, "key") {
        out = format!("{out}. {}", t::consent::machine_key_file(&key));
    }
    Some(out)
}

pub fn fact_line(line: &str, args: &Value) -> Option<String> {
    Some(match line {
        "consent-machine" => machine_line(args)?,
        "consent-machine-add" => t::consent::machine_add(&arg(args, "machine")?),
        "consent-machine-import" => t::consent::machine_import(&names(args, "machines")?),
        "consent-machine-addresses" => {
            t::consent::machine_addresses(&names(args, "before")?, &names(args, "after")?)
        }
        "consent-machine-remove" => t::consent::machine_remove(&arg(args, "machine")?),
        "consent-machine-step" => {
            t::consent::machine_step(&arg(args, "title")?, &arg(args, "display")?)
        }
        "consent-machine-admin" => t::consent::machine_admin(need_word(&arg(args, "need")?)),
        "consent-network-switch" => {
            t::consent::network_switch(&arg(args, "from")?, &arg(args, "to")?)
        }
        "consent-network-drops" => drops_line(args)?,
        "consent-network-cluster" => t::consent::network_cluster(
            args.get("members").and_then(Value::as_u64)?,
            &arg(args, "failover")?,
        ),
        "consent-network-phone" => t::consent::network_phone().to_owned(),
        "consent-update-tool" => t::consent::update_tool(
            &arg(args, "tool")?,
            &arg(args, "method")?,
            &arg(args, "from")?,
            &arg(args, "to")?,
        ),
        "consent-update-tool-latest" => t::consent::update_tool_latest(
            &arg(args, "tool")?,
            &arg(args, "method")?,
            &arg(args, "from")?,
        ),
        "consent-update-back" => {
            t::consent::update_back(&arg(args, "tool")?, &arg(args, "from")?, &arg(args, "to")?)
        }
        "consent-update-running" => t::consent::update_running().to_owned(),
        "consent-update-machine" => {
            t::consent::update_machine(&arg(args, "machine")?, &arg(args, "organization")?)
        }
        "consent-update-worker-tool" => t::consent::update_worker_tool(
            &arg(args, "tool")?,
            &arg(args, "from")?,
            &arg(args, "to")?,
        ),
        "consent-update-notchling" => t::consent::update_notchling(&arg(args, "sha")?),
        "consent-install-files" => t::consent::install_files(count(args)?),
        "consent-install-sidecar" => t::consent::install_sidecar().to_owned(),
        "consent-model-download" => t::consent::model_download(
            &arg(args, "model")?,
            &arg(args, "download")?,
            &arg(args, "kept")?,
        ),
        "consent-cost-apply" => t::consent::cost_apply(count(args)?),
        "consent-cost-undo" => t::consent::cost_undo(count(args)?, cost_tool(&arg(args, "tool")?)),
        "consent-cost-profile" => t::consent::cost_profile(count(args)?),
        "consent-install-backup" => t::consent::install_backup().to_owned(),
        "consent-cost-backup" => t::consent::install_backup().to_owned(),
        _ => return None,
    })
}

pub fn change_word(change: &str) -> String {
    match change {
        "create" => t::consent::install_part_create(),
        "update" => t::consent::install_part_update(),
        "remove" => t::consent::install_part_remove(),
        "restore" => t::consent::install_part_restore(),
        other => return other.to_owned(),
    }
    .to_owned()
}

pub fn part_title(part: &Part) -> String {
    t::consent::install_part_title(&change_word(&part.change), &part.path)
}

pub fn part_counts(part: &Part) -> String {
    t::consent::install_part_counts(&part.added.to_string(), &part.removed.to_string())
}

pub fn part_hash(part: &Part) -> Option<String> {
    let short = |hash: &str| hash.chars().take(8).collect::<String>();
    part.after
        .as_deref()
        .map(short)
        .map(|after| t::consent::install_part_hash(&after))
}

pub fn part_also(path: &str) -> String {
    t::consent::install_part_also(path)
}

pub fn part_field(field: &Field) -> String {
    t::consent::install_part_field(&field.name, &field.before, &field.after)
}

pub fn stale_note(paths: &[String]) -> String {
    if paths.is_empty() {
        t::consent::install_stale_request().to_owned()
    } else {
        t::consent::install_stale_parts(&paths.join(", "))
    }
}

pub fn applied_note(reply: &Value, parts: &[Part]) -> Option<String> {
    let states = reply.get("parts").and_then(Value::as_array)?;
    let of = |wanted: &str| -> Vec<String> {
        states
            .iter()
            .filter(|s| s.get("state").and_then(Value::as_str) == Some(wanted))
            .filter_map(|s| s.get("id").and_then(Value::as_str))
            .map(|id| {
                parts
                    .iter()
                    .find(|p| p.id == id)
                    .map(|p| p.path.clone())
                    .filter(|p| !p.is_empty())
                    .unwrap_or_else(|| id.to_owned())
            })
            .collect()
    };
    let stale = of("stale");
    if !stale.is_empty() {
        return Some(stale_note(&stale));
    }
    let failed = of("failed");
    (!failed.is_empty()).then(|| t::consent::install_part_failed(&failed.join(", ")))
}
