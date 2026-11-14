use super::check::{CheckPart, CheckResult, Outcome};
use super::record::{Fact, Recorder};
use super::{Tool, Version};
use crate::store;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const REFERENCES_DOC: &str = "versions/references";
pub const FAILED_DOC: &str = "versions/failed";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Reference {
    pub tool: Tool,
    pub version: Version,
    pub sha256: String,
    pub result: CheckResult,
    pub at_ms: i64,
    pub unchecked: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Failed {
    pub part: CheckPart,
    pub at_ms: i64,
    pub why: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Book {
    refs: BTreeMap<Tool, Reference>,
    failed: BTreeMap<Tool, BTreeMap<String, Failed>>,
}

impl Book {
    pub fn new() -> Book {
        Book::default()
    }

    pub fn reference(&self, tool: Tool) -> Option<&Reference> {
        self.refs.get(&tool)
    }

    pub fn failure(&self, tool: Tool, version: &Version) -> Option<&Failed> {
        self.failed.get(&tool)?.get(version.as_str())
    }

    pub fn failed_versions(&self, tool: Tool) -> Vec<(String, Failed)> {
        self.failed
            .get(&tool)
            .map(|m| m.iter().map(|(v, f)| (v.clone(), f.clone())).collect())
            .unwrap_or_default()
    }

    pub fn apply(
        &mut self,
        tool: Tool,
        version: &Version,
        sha256: &str,
        outcome: &Outcome,
        now_ms: i64,
    ) {
        match outcome.result {
            CheckResult::Passed | CheckResult::PartlyChecked => {
                self.refs.insert(
                    tool,
                    Reference {
                        tool,
                        version: version.clone(),
                        sha256: sha256.to_string(),
                        result: outcome.result,
                        at_ms: now_ms,
                        unchecked: outcome.unchecked.iter().map(|s| s.to_string()).collect(),
                    },
                );
                self.check_again(tool, version);
            }
            CheckResult::Failed => {
                let part = outcome.failed_part.unwrap_or(CheckPart::Version);
                self.failed.entry(tool).or_default().insert(
                    version.as_str().to_string(),
                    Failed {
                        part,
                        at_ms: now_ms,
                        why: outcome.why.clone(),
                    },
                );
            }
            CheckResult::CantTell => {}
        }
    }

    pub fn usable(&self, tool: Tool, installed: &Version, sha256: &str) -> bool {
        if sha256.is_empty() {
            return false;
        }
        let Some(reference) = self.refs.get(&tool) else {
            return false;
        };
        reference.version == *installed
            && reference.sha256 == sha256
            && !self.is_failed(tool, installed)
    }

    pub fn is_failed(&self, tool: Tool, version: &Version) -> bool {
        self.failure(tool, version).is_some()
    }

    pub fn check_again(&mut self, tool: Tool, version: &Version) {
        if let Some(map) = self.failed.get_mut(&tool) {
            map.remove(version.as_str());
            if map.is_empty() {
                self.failed.remove(&tool);
            }
        }
    }
}

pub fn load(data: &Path, recorder: &dyn Recorder) -> Book {
    let mut book = Book::new();
    match read_doc::<BTreeMap<String, Reference>>(data, REFERENCES_DOC) {
        Ok(Some(map)) => {
            for (name, reference) in map {
                if Tool::from_name(&name) == Some(reference.tool) {
                    book.refs.insert(reference.tool, reference);
                }
            }
        }
        Ok(None) => {}
        Err(()) => recorder.record(Fact::LoadFailed {
            doc: REFERENCES_DOC,
        }),
    }
    match read_doc::<BTreeMap<String, BTreeMap<String, Failed>>>(data, FAILED_DOC) {
        Ok(Some(map)) => {
            for (name, versions) in map {
                let Some(tool) = Tool::from_name(&name) else {
                    continue;
                };
                let clean: BTreeMap<String, Failed> = versions
                    .into_iter()
                    .filter(|(v, _)| Version::parse(v).is_some())
                    .collect();
                if !clean.is_empty() {
                    book.failed.insert(tool, clean);
                }
            }
        }
        Ok(None) => {}
        Err(()) => recorder.record(Fact::LoadFailed { doc: FAILED_DOC }),
    }
    book
}

fn read_doc<T: serde::de::DeserializeOwned>(data: &Path, name: &str) -> Result<Option<T>, ()> {
    match store::doc(data, name) {
        Ok(Some(text)) => serde_json::from_str(&text).map(Some).map_err(|_| ()),
        Ok(None) => Ok(None),
        Err(_) => Err(()),
    }
}

pub fn save(data: &Path, book: &Book) -> Result<(), String> {
    let refs: BTreeMap<&str, &Reference> = book.refs.iter().map(|(t, r)| (t.name(), r)).collect();
    let failed: BTreeMap<&str, &BTreeMap<String, Failed>> =
        book.failed.iter().map(|(t, f)| (t.name(), f)).collect();
    store::put_doc(
        data,
        REFERENCES_DOC,
        &serde_json::to_string(&refs).map_err(|e| e.to_string())?,
    )?;
    store::put_doc(
        data,
        FAILED_DOC,
        &serde_json::to_string(&failed).map_err(|e| e.to_string())?,
    )
}
