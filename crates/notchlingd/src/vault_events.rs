use super::*;
use notchling_events::field::{Exe, List, Millis};
use notchling_events::kinds::choices::Answerer;
use notchling_events::kinds::secrets::{
    Approval, EntryAction, RequestResult, SecretResult, VaultEntryChanged, VaultSecretRequested,
    VaultSecretUsed,
};
use notchling_events::spec::Outcome;
use notchling_events::{scope, Device, Event, Folder, Ref, Secret, Session};

pub(super) const LOG_DOWN: &str = "the audit log can't be written";

pub(super) struct Trail {
    pub(super) names: Vec<String>,
    pub(super) folder: String,
    pub(super) command: String,
    pub(super) approval: Approval,
    pub(super) by: Option<Answerer>,
    pub(super) device: Option<String>,
    pub(super) waited_ms: Option<u64>,
}

impl Trail {
    pub(super) fn new(folder: String, command: String) -> Trail {
        Trail {
            names: Vec::new(),
            folder,
            command,
            approval: Approval::AlwaysAllow,
            by: Some(Answerer::NoOne),
            device: None,
            waited_ms: None,
        }
    }
}

fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(key, _)| {
        !key.is_empty() && key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
    })
}

fn command_exe(command: &str) -> Option<Exe> {
    let word = command
        .split_whitespace()
        .find(|word| !is_assignment(word))?;
    let name = word.rsplit('/').next().filter(|n| !n.is_empty())?;
    Some(Exe::named(name))
}

fn folder_ref(folder: &str) -> Option<Ref<Folder>> {
    (!folder.is_empty()).then(|| Ref::<Folder>::new(folder))
}

fn session_ref() -> Option<Ref<Session>> {
    scope::current().actor.and_then(|a| a.session)
}

pub(super) fn device_now() -> Option<String> {
    scope::current()
        .actor
        .and_then(|a| a.device)
        .map(|d| d.as_str().to_owned())
}

fn outcome_of(result: SecretResult) -> Outcome {
    match result {
        SecretResult::Given => Outcome::Ok,
        SecretResult::Failed => Outcome::Failed,
        _ => Outcome::Refused,
    }
}

fn request_outcome(result: RequestResult) -> Outcome {
    match result {
        RequestResult::Saved => Outcome::Ok,
        _ => Outcome::Refused,
    }
}

impl VaultOps {
    pub(super) fn note_used(&self, trail: &Trail, result: SecretResult, commit: bool) -> bool {
        let Some(events) = &self.events else {
            return true;
        };
        let secrets = trail.names.iter().map(|n| Ref::<Secret>::new(n)).collect();
        let event = Event::new(
            &VaultSecretUsed {
                secrets: List::new(secrets),
                session: session_ref(),
                folder: folder_ref(&trail.folder),
                command_exe: command_exe(&trail.command),
                approval: trail.approval,
                answered_by: trail.by,
                device: trail.device.as_deref().map(Ref::<Device>::new),
                result,
                waited_ms: trail.waited_ms.map(Millis),
            },
            outcome_of(result),
            crate::acting_ops::now_ms(),
        );
        scope::mark_recorded();
        if commit {
            events.commit(event)
        } else {
            events.push(event);
            true
        }
    }

    pub(super) fn note_requested(
        &self,
        name: &str,
        folder: &str,
        result: RequestResult,
        by: Option<Answerer>,
        commit: bool,
    ) -> bool {
        let Some(events) = &self.events else {
            return true;
        };
        let event = Event::new(
            &VaultSecretRequested {
                name: Ref::<Secret>::new(name),
                session: session_ref(),
                folder: folder_ref(folder),
                result,
                answered_by: by,
            },
            request_outcome(result),
            crate::acting_ops::now_ms(),
        );
        scope::mark_recorded();
        if commit {
            events.commit(event)
        } else {
            events.push(event);
            true
        }
    }

    pub(super) fn note_entry(&self, name: &str, action: EntryAction) {
        if let Some(events) = &self.events {
            events.push(Event::new(
                &VaultEntryChanged {
                    name: Ref::<Secret>::new(name),
                    action,
                },
                Outcome::Ok,
                crate::acting_ops::now_ms(),
            ));
        }
    }
}
