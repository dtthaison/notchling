use super::backup;
use super::kinds::{self, Kind, FIELD, KINDS};
use super::migrate::MigrateError;
use super::record::{Counts, Document, MachineSettings, Problem, DOC};
use super::view::{Resolved, State, View};
use crate::machines::registry::{RegistryFiles, RegistryState};
use crate::store::{self, DocChange};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditError {
    Unreadable(String),
    Problems(Vec<Problem>),
    NoSuchEntry,
    Store(String),
}

#[derive(Clone, Debug)]
pub struct Organizations {
    pub(super) data_dir: PathBuf,
}

pub(super) fn parse_doc(row: Option<&str>) -> Result<Document, String> {
    match row {
        None => Ok(Document::personal_only()),
        Some(text) => Document::parse(text),
    }
}

impl Organizations {
    pub fn new(data_dir: &Path) -> Organizations {
        Organizations {
            data_dir: data_dir.to_path_buf(),
        }
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn state(&self) -> State {
        match store::doc(&self.data_dir, DOC) {
            Err(why) => State::Unreadable(why),
            Ok(None) => State::Missing,
            Ok(Some(text)) => match Document::parse(&text) {
                Ok(doc) => State::Loaded(doc),
                Err(why) => State::Unreadable(why),
            },
        }
    }

    pub fn view(&self) -> View {
        View::new(self.state())
    }

    pub fn listing(&self, kind: Kind) -> Option<Vec<(String, Option<String>)>> {
        let text = store::doc(&self.data_dir, kind.doc()).ok()??;
        kinds::listing(kind, &text)
    }

    pub fn resolve_all(&self, kind: Kind) -> Vec<(String, Resolved)> {
        let view = self.view();
        self.listing(kind)
            .unwrap_or_default()
            .into_iter()
            .map(|(key, field)| {
                let resolved = view.resolve(kind, &key, field.as_deref());
                (key, resolved)
            })
            .collect()
    }

    pub fn resolve(&self, kind: Kind, key: &str) -> Resolved {
        let view = self.view();
        let wanted = kind.normal(key);
        let field = self
            .listing(kind)
            .and_then(|l| l.into_iter().find(|(k, _)| *k == wanted))
            .and_then(|(_, f)| f);
        view.resolve(kind, key, field.as_deref())
    }

    pub fn counts(&self, registry: Option<&RegistryFiles>) -> BTreeMap<String, Counts> {
        let mut out: BTreeMap<String, Counts> = BTreeMap::new();
        let view = self.view();
        if let Some(registry) = registry {
            for machine in registry.all() {
                let id = view.machine(machine.organization.as_deref()).id;
                out.entry(id).or_default().machines += 1;
            }
        }
        for kind in KINDS {
            for (_, resolved) in self.resolve_all(kind) {
                let counts = out.entry(resolved.id).or_default();
                if kind.is_account() {
                    counts.accounts += 1;
                } else {
                    counts.secrets += 1;
                }
            }
        }
        out
    }

    pub fn edit<T>(
        &self,
        change: impl FnOnce(&mut Document) -> Result<T, Vec<Problem>>,
    ) -> Result<T, EditError> {
        let mut result: Option<Result<T, EditError>> = None;
        let stored = store::update_doc(&self.data_dir, DOC, |row| {
            let mut doc = match parse_doc(row.as_deref()) {
                Ok(doc) => doc,
                Err(why) => {
                    result = Some(Err(EditError::Unreadable(why.clone())));
                    return Err(why);
                }
            };
            match change(&mut doc) {
                Ok(value) => {
                    let problems = doc.problems();
                    if !problems.is_empty() {
                        result = Some(Err(EditError::Problems(problems)));
                        return Err("the change is not valid".into());
                    }
                    let text = doc.text()?;
                    result = Some(Ok(value));
                    Ok(Some(text))
                }
                Err(problems) => {
                    result = Some(Err(EditError::Problems(problems)));
                    Err("the change is refused".into())
                }
            }
        });
        match (result, stored) {
            (Some(Ok(value)), Ok(_)) => Ok(value),
            (Some(Err(e)), _) => Err(e),
            (_, Err(why)) => Err(EditError::Store(why)),
            (None, Ok(_)) => Err(EditError::Store("nothing was changed".into())),
        }
    }

    pub fn create(
        &self,
        name: &str,
        github_owner: Option<&str>,
        color: Option<&str>,
    ) -> Result<String, EditError> {
        if self.view().only_personal() {
            self.migrate(None).map_err(|e| match e {
                MigrateError::Unreadable(why) => EditError::Unreadable(why),
                MigrateError::Store(why) => EditError::Store(why),
            })?;
        }
        self.edit(|doc| doc.create(name, github_owner, color))
    }

    pub fn rename(&self, id: &str, name: &str) -> Result<(), EditError> {
        self.edit(|doc| doc.rename(id, name).map_err(|p| vec![p]))
    }

    pub fn set_color(&self, id: &str, color: &str) -> Result<(), EditError> {
        self.edit(|doc| doc.set_color(id, color).map_err(|p| vec![p]))
    }

    pub fn set_github_owner(&self, id: &str, owner: Option<&str>) -> Result<(), EditError> {
        self.edit(|doc| doc.set_github_owner(id, owner).map_err(|p| vec![p]))
    }

    pub fn set_folders(&self, id: &str, folders: &[String]) -> Result<(), EditError> {
        self.edit(|doc| doc.set_folders(id, folders).map_err(|p| vec![p]))
    }

    pub fn set_machine_settings(
        &self,
        id: &str,
        settings: Option<MachineSettings>,
    ) -> Result<(), EditError> {
        self.edit(|doc| doc.set_machine_settings(id, settings).map_err(|p| vec![p]))
    }

    pub fn delete(&self, id: &str, registry: Option<&RegistryFiles>) -> Result<(), EditError> {
        let machines: Vec<Option<String>> = match registry.map(RegistryFiles::load) {
            Some(RegistryState::Unreadable(why)) => return Err(EditError::Unreadable(why)),
            Some(state) => state
                .machines()
                .into_iter()
                .map(|m| m.organization)
                .collect(),
            None => Vec::new(),
        };
        self.import_legacy();
        let names = [
            DOC,
            KINDS[0].doc(),
            KINDS[1].doc(),
            KINDS[2].doc(),
            KINDS[3].doc(),
        ];
        let mut failure: Option<EditError> = None;
        let stored = store::update_docs(&self.data_dir, &names, |old| {
            let mut doc = match parse_doc(old[0].as_deref()) {
                Ok(doc) => doc,
                Err(why) => {
                    failure = Some(EditError::Unreadable(why.clone()));
                    return Err(why);
                }
            };
            let view = View::new(State::Loaded(doc.clone()));
            let mut counts = Counts {
                machines: machines
                    .iter()
                    .filter(|m| view.machine(m.as_deref()).id == id)
                    .count(),
                ..Counts::default()
            };
            for (i, kind) in KINDS.iter().enumerate() {
                let Some(text) = old[i + 1].as_deref() else {
                    continue;
                };
                let Some(list) = kinds::listing(*kind, text) else {
                    let why = format!("{} can't be read", kind.doc());
                    failure = Some(EditError::Unreadable(why.clone()));
                    return Err(why);
                };
                for (key, field) in list {
                    if view.resolve(*kind, &key, field.as_deref()).id == id {
                        if kind.is_account() {
                            counts.accounts += 1;
                        } else {
                            counts.secrets += 1;
                        }
                    }
                }
            }
            if let Err(problem) = doc.delete(id, counts) {
                failure = Some(EditError::Problems(vec![problem]));
                return Err("the change is refused".into());
            }
            let problems = doc.problems();
            if !problems.is_empty() {
                failure = Some(EditError::Problems(problems));
                return Err("the change is not valid".into());
            }
            let mut changes = vec![DocChange::Keep; names.len()];
            changes[0] = DocChange::Set(doc.text()?);
            Ok(changes)
        });
        match (failure, stored) {
            (Some(e), _) => Err(e),
            (None, Ok(())) => Ok(()),
            (None, Err(why)) => Err(EditError::Store(why)),
        }
    }

    pub(super) fn import_legacy(&self) {
        for kind in KINDS {
            let _ = store::import_file(
                &self.data_dir,
                kind.doc(),
                &kind.legacy_file(&self.data_dir),
                |text| kinds::listing(kind, text).is_some(),
            );
        }
    }

    pub fn set_organization(
        &self,
        kind: Kind,
        key: &str,
        organization: &str,
    ) -> Result<(), EditError> {
        let wanted = kind.normal(key);
        let mut failure: Option<EditError> = None;
        let stored = store::update_docs(&self.data_dir, &[DOC, kind.doc()], |old| {
            let mut doc = match parse_doc(old[0].as_deref()) {
                Ok(doc) => doc,
                Err(why) => {
                    failure = Some(EditError::Unreadable(why.clone()));
                    return Err(why);
                }
            };
            if doc.find(organization).is_none() {
                failure = Some(EditError::Problems(vec![Problem::IdUnknown(
                    organization.into(),
                )]));
                return Err("unknown organization".into());
            }
            let Some(text) = old[1].as_deref() else {
                failure = Some(EditError::NoSuchEntry);
                return Err("no such entry".into());
            };
            let mut root: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
            let found = kind.entries_mut(&mut root).and_then(|entries| {
                entries
                    .iter_mut()
                    .find(|e| kind.key_of(e).as_deref() == Some(wanted.as_str()))
            });
            let Some(entry) = found else {
                failure = Some(EditError::NoSuchEntry);
                return Err("no such entry".into());
            };
            entry[FIELD] = Value::String(organization.into());
            shadow_of(&mut doc, kind).insert(kind.shadow_key(&wanted), organization.into());
            backup::keep(&self.data_dir, kind.doc(), text)?;
            Ok(vec![
                DocChange::Set(doc.text()?),
                DocChange::Set(kinds::render(text, &root)?),
            ])
        });
        match (failure, stored) {
            (Some(e), _) => Err(e),
            (None, Ok(())) => Ok(()),
            (None, Err(why)) => Err(EditError::Store(why)),
        }
    }

    pub fn move_shadow(&self, kind: Kind, from: &str, to: &str) -> Result<(), EditError> {
        let from = kind.shadow_key(&kind.normal(from));
        let to = kind.shadow_key(&kind.normal(to));
        let State::Loaded(mut doc) = self.state() else {
            return Ok(());
        };
        if !shadow_of(&mut doc, kind).contains_key(&from) {
            return Ok(());
        }
        self.edit(|doc| {
            let shadow = shadow_of(doc, kind);
            if let Some(value) = shadow.remove(&from) {
                shadow.insert(to, value);
            }
            Ok(())
        })
    }

    pub fn forget(&self, kind: Kind, key: &str) -> Result<(), EditError> {
        let shadow_key = kind.shadow_key(&kind.normal(key));
        self.edit(|doc| {
            shadow_of(doc, kind).remove(&shadow_key);
            Ok(())
        })
    }
}

pub(super) fn shadow_of(doc: &mut Document, kind: Kind) -> &mut BTreeMap<String, String> {
    if kind.is_account() {
        &mut doc.shadow.accounts
    } else {
        &mut doc.shadow.vault
    }
}
