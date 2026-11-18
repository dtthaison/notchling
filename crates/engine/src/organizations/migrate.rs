use super::backup;
use super::book::{parse_doc, shadow_of, Organizations};
use super::kinds::{self, FIELD, KINDS};
use super::record::{Document, DOC};
use super::view::{Gap, Source, State, View};
use crate::machines::registry::{RegistryFiles, REGISTRY_DOC};
use crate::store::{self, DocChange};
use serde_json::Value;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub created: bool,
    pub stamped: usize,
    pub restored: usize,
    pub backups: Vec<&'static str>,
    pub skipped: Vec<(&'static str, String)>,
    pub gaps: Vec<Gap>,
    pub machines: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrateError {
    Unreadable(String),
    Store(String),
}

impl Organizations {
    pub fn migrate(&self, registry: Option<&RegistryFiles>) -> Result<Report, MigrateError> {
        let names = [
            DOC,
            KINDS[0].doc(),
            KINDS[1].doc(),
            KINDS[2].doc(),
            KINDS[3].doc(),
        ];
        let mut report = Report::default();
        let mut failure: Option<MigrateError> = None;
        let stored = store::update_docs(&self.data_dir, &names, |old| {
            let mut doc = match old[0].as_deref() {
                None => {
                    report.created = true;
                    Document::personal_only()
                }
                Some(text) => match parse_doc(Some(text)) {
                    Ok(doc) => doc,
                    Err(why) => {
                        failure = Some(MigrateError::Unreadable(why.clone()));
                        return Err(why);
                    }
                },
            };
            let before = doc.clone();
            let view = View::new(State::Loaded(doc.clone()));
            let mut changes = vec![DocChange::Keep; names.len()];
            for (i, kind) in KINDS.iter().enumerate() {
                let Some(text) = old[i + 1].as_deref() else {
                    continue;
                };
                let Ok(mut root) = serde_json::from_str::<Value>(text) else {
                    report.skipped.push((kind.doc(), "not JSON".into()));
                    continue;
                };
                let Some(entries) = kind.entries_mut(&mut root) else {
                    report.skipped.push((kind.doc(), "not a list".into()));
                    continue;
                };
                let mut changed = false;
                for entry in entries.iter_mut() {
                    let Some(key) = kind.key_of(entry) else {
                        continue;
                    };
                    let field = kinds::field_of(entry);
                    let resolved = view.resolve(*kind, &key, field.as_deref());
                    if field.is_none() {
                        match resolved.source {
                            Source::Shadow => report.restored += 1,
                            _ => {
                                report.stamped += 1;
                                if !view.only_personal() {
                                    report.gaps.push(Gap {
                                        kind: *kind,
                                        key: key.clone(),
                                    });
                                }
                            }
                        }
                        entry[FIELD] = Value::String(resolved.id.clone());
                        changed = true;
                    }
                    shadow_of(&mut doc, *kind).insert(kind.shadow_key(&key), resolved.id);
                }
                if changed {
                    if backup::keep(&self.data_dir, kind.doc(), text)? {
                        report.backups.push(kind.doc());
                    }
                    changes[i + 1] = DocChange::Set(kinds::render(text, &root)?);
                }
            }
            if report.created || doc != before {
                changes[0] = DocChange::Set(doc.text()?);
            }
            Ok(changes)
        });
        if let Some(e) = failure {
            return Err(e);
        }
        stored.map_err(MigrateError::Store)?;
        if let Some(registry) = registry {
            match registry.adopt_organizations() {
                Ok(n) => report.machines = n,
                Err(why) => report.skipped.push((REGISTRY_DOC, why)),
            }
        }
        Ok(report)
    }
}
