use super::vault_events::Trail;
use super::*;
use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::cluster::grants::encode_env;
use notchling_engine::vault::VaultValue;
use notchling_events::kinds::choices::Answerer;

#[derive(Clone, Debug)]
pub struct ReleaseFor {
    pub node_name: String,
    pub job: String,
    pub command: String,
    pub folder: String,
}

pub const DENIED: &str = "denied";

pub fn was_denied(error: &ApiError) -> bool {
    error
        .data
        .as_ref()
        .and_then(|d| d.get(DENIED))
        .and_then(Value::as_bool)
        .unwrap_or(false)
}

pub type Still<'a> = dyn Fn() -> Result<(), String> + 'a;

pub(super) enum Who<'a> {
    Caller(&'a Caller),
    Grant(&'a ReleaseFor),
    Machine(&'a str),
}

pub struct Released {
    pub values: Vec<(VaultEntry, GrantValue)>,
    pub asked: bool,
    pub by: Option<Answerer>,
    pub device: Option<String>,
    pub waited_ms: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Looked {
    pub exists: bool,
    pub asks: bool,
}

impl VaultOps {
    pub(super) fn release_picked(
        &self,
        who: &Who<'_>,
        names: &[String],
        (folder, command): (&str, &str),
        (trail, still): (&mut Trail, &Still<'_>),
    ) -> Result<Vec<(VaultEntry, VaultValue)>, (ApiError, SecretResult)> {
        let none = |e: ApiError| (e, SecretResult::Failed);
        let all = self.entries().map_err(none)?;
        let mut picked: Vec<VaultEntry> = Vec::new();
        for name in names {
            let id = name.to_lowercase();
            match all.iter().find(|e| e.id() == id) {
                Some(e) if !picked.iter().any(|p| p.id() == id) => picked.push(e.clone()),
                Some(_) => {}
                None => return Err(none(missing(who, name, &all))),
            }
        }
        trail.names = picked.iter().map(|e| e.name.clone()).collect();
        let fail = |e: ApiError| (e, SecretResult::Failed);
        let asking: Vec<VaultEntry> = picked.iter().filter(|e| e.asks()).cloned().collect();
        if !asking.is_empty() {
            let asked = self
                .approve(who, &asking, command, folder)
                .map_err(|r| (r.err, r.result))?;
            trail.approval = Approval::Asked;
            trail.by = Some(asked.by);
            trail.device = asked.device.clone();
            trail.waited_ms = Some(asked.waited_ms);
            if !self.switch.is_on() {
                return Err((self.switch.refusal(), SecretResult::Cancelled));
            }
            match asked.decision {
                Decision::Deny => {
                    let why = match asked.state {
                        State::Answered => SecretResult::Denied,
                        State::Expired => SecretResult::Expired,
                        _ => SecretResult::Cancelled,
                    };
                    let mut error = said_no("The user didn't allow it.").with_reason(match why {
                        SecretResult::Denied => "denied",
                        SecretResult::Expired => "expired",
                        _ => "cancelled",
                    });
                    if why == SecretResult::Denied {
                        error = error.with_field(DENIED, json!(true));
                    }
                    return Err((error, why));
                }
                Decision::Once => {}
                Decision::Always => {
                    let ids: BTreeSet<String> = asking.iter().map(VaultEntry::id).collect();
                    self.store
                        .stop_asking(&ids)
                        .map_err(|e| fail(failed(e.to_string())))?;
                    let names: Vec<&str> = asking.iter().map(|e| e.name.as_str()).collect();
                    self.publish_changed(&names, false);
                    for name in &names {
                        self.note_entry(name, EntryAction::AskFirstOff);
                    }
                }
            }
        }
        if let Err(why) = still() {
            let e = ApiError::new(ErrorCode::Forbidden, why);
            return Err((e, SecretResult::Cancelled));
        }
        let mut pairs = Vec::new();
        for entry in picked {
            match self.store.value(&entry) {
                Ok(Some(value)) => pairs.push((entry, value)),
                Ok(None) => {
                    return Err(fail(failed(format!(
                        "{} has no value saved. Ask the user to update it with secret-request {}.",
                        entry.name, entry.name
                    ))))
                }
                Err(e) => return Err(fail(failed(e.to_string()))),
            }
        }
        if !self.switch.is_on() {
            return Err((self.switch.refusal(), SecretResult::Cancelled));
        }
        Ok(pairs)
    }

    pub fn release(
        &self,
        who: &ReleaseFor,
        names: &[String],
        still: &Still<'_>,
    ) -> Result<Released, ApiError> {
        if !self.switch.is_on() {
            self.off_hook().run();
            return Err(self.switch.refusal());
        }
        let mut trail = Trail::new(who.folder.clone(), who.command.clone());
        let got = self.release_picked(
            &Who::Grant(who),
            names,
            (&who.folder, &who.command),
            (&mut trail, still),
        );
        match got {
            Ok(pairs) => {
                let mut values = Vec::new();
                for pair in &pairs {
                    let (env, _) = list::environment(std::slice::from_ref(pair));
                    let value = encode_env(&env).map_err(failed)?;
                    values.push((pair.0.clone(), value));
                }
                Ok(Released {
                    values,
                    asked: trail.approval == Approval::Asked,
                    by: trail.by,
                    device: trail.device,
                    waited_ms: trail.waited_ms,
                })
            }
            Err((e, result)) => {
                if !trail.names.is_empty() {
                    self.note_used(&trail, result, false);
                }
                Err(e)
            }
        }
    }

    pub fn lookup(&self, names: &[String]) -> Result<Vec<Looked>, ApiError> {
        if !self.switch.is_on() {
            self.off_hook().run();
            return Err(self.switch.refusal());
        }
        let all = self.entries()?;
        Ok(names
            .iter()
            .map(|name| {
                let id = name.to_lowercase();
                let found = all.iter().find(|e| e.id() == id);
                Looked {
                    exists: found.is_some(),
                    asks: found.is_some_and(VaultEntry::asks),
                }
            })
            .collect())
    }

    pub fn info(&self, name: &str) -> Result<Option<VaultEntry>, ApiError> {
        if !self.switch.is_on() {
            self.off_hook().run();
            return Err(self.switch.refusal());
        }
        let id = name.to_lowercase();
        Ok(self.entries()?.into_iter().find(|e| e.id() == id))
    }

    pub fn note_refused(&self, who: &ReleaseFor, name: &str) {
        let mut trail = Trail::new(who.folder.clone(), who.command.clone());
        trail.names = vec![name.to_string()];
        self.note_used(&trail, SecretResult::Denied, false);
    }

    pub fn note_released(&self, who: &ReleaseFor, name: &str, released: &Released) -> bool {
        let mut trail = Trail::new(who.folder.clone(), who.command.clone());
        trail.names = vec![name.to_string()];
        trail.approval = if released.asked {
            Approval::Asked
        } else {
            Approval::AlwaysAllow
        };
        trail.by = released.by.or(Some(Answerer::NoOne));
        trail.device = released.device.clone();
        trail.waited_ms = released.waited_ms;
        self.note_used(&trail, SecretResult::Given, true)
    }
}

fn missing(who: &Who<'_>, name: &str, all: &[VaultEntry]) -> ApiError {
    let which = if valid_name(name) {
        format!("No secret named {name}.")
    } else {
        "No secret by that name.".to_string()
    };
    match who {
        Who::Grant(_) | Who::Machine(_) => failed(which),
        Who::Caller(_) => {
            let saved = if all.is_empty() {
                "none".to_string()
            } else {
                all.iter()
                    .map(|e| e.name.clone())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            failed(format!(
                "{which} Saved: {saved}. Ask the user for it with secret-request."
            ))
        }
    }
}
