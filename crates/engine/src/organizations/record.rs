use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::collections::BTreeMap;

pub const DOC: &str = "organizations";
pub const VERSION: i64 = 1;
pub const PERSONAL_ID: &str = "personal";
pub const PERSONAL_NAME: &str = "Personal";
pub const DEFAULT_COLOR: &str = "grey";
pub const NAME_MAX: usize = 40;
const OWNER_MAX: usize = 39;
const COLOR_MAX: usize = 20;
const ID_MAX: usize = 32;

fn grey() -> String {
    DEFAULT_COLOR.into()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MachineSettings {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_login: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_read: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_act: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plain_ssh: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Organization {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub github_owner: Option<String>,
    #[serde(default = "grey")]
    pub color: String,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub order: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub machines: Option<MachineSettings>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Organization {
    pub fn personal() -> Organization {
        Organization {
            id: PERSONAL_ID.into(),
            name: PERSONAL_NAME.into(),
            github_owner: None,
            color: grey(),
            folders: Vec::new(),
            order: None,
            machines: None,
            extra: Map::new(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shadow {
    #[serde(default)]
    pub accounts: BTreeMap<String, String>,
    #[serde(default)]
    pub vault: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Document {
    pub version: i64,
    pub organizations: Vec<Organization>,
    #[serde(default)]
    pub shadow: Shadow,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NameEmpty,
    NameTooLong,
    NameTaken(String),
    IdInvalid(String),
    IdTaken(String),
    IdUnknown(String),
    PersonalMissing,
    PersonalNotDeletable,
    ColorInvalid(String),
    OwnerInvalid(String),
    FolderInvalid(String),
    FolderTaken { folder: String, by: String },
    ShadowInvalid(String),
    NotEmpty(Counts),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub machines: usize,
    pub accounts: usize,
    pub secrets: usize,
}

impl Counts {
    pub fn is_empty(&self) -> bool {
        *self == Counts::default()
    }
}

impl Document {
    pub fn personal_only() -> Document {
        Document {
            version: VERSION,
            organizations: vec![Organization::personal()],
            shadow: Shadow::default(),
            extra: Map::new(),
        }
    }

    pub fn parse(text: &str) -> Result<Document, String> {
        let doc: Document = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if doc.version != VERSION {
            return Err(format!("version {} is not supported", doc.version));
        }
        if let Some(first) = doc.problems().first() {
            return Err(format!("the document is not valid: {first:?}"));
        }
        Ok(doc)
    }

    pub fn text(&self) -> Result<String, String> {
        serde_json::to_string_pretty(self).map_err(|e| e.to_string())
    }

    pub fn find(&self, id: &str) -> Option<&Organization> {
        self.organizations.iter().find(|o| o.id == id)
    }

    fn find_mut(&mut self, id: &str) -> Result<&mut Organization, Problem> {
        self.organizations
            .iter_mut()
            .find(|o| o.id == id)
            .ok_or_else(|| Problem::IdUnknown(id.into()))
    }

    pub fn problems(&self) -> Vec<Problem> {
        let mut out = Vec::new();
        let mut ids: Vec<&str> = Vec::new();
        let mut names: Vec<String> = Vec::new();
        let mut folders: Vec<(String, &str)> = Vec::new();
        if self.find(PERSONAL_ID).is_none() {
            out.push(Problem::PersonalMissing);
        }
        for org in &self.organizations {
            if !valid_id(&org.id) {
                out.push(Problem::IdInvalid(org.id.clone()));
            } else if ids.contains(&org.id.as_str()) {
                out.push(Problem::IdTaken(org.id.clone()));
            }
            ids.push(&org.id);
            if let Some(p) = name_problem(&org.name) {
                out.push(p);
            } else {
                let key = name_key(&org.name);
                if names.contains(&key) {
                    out.push(Problem::NameTaken(org.name.clone()));
                }
                names.push(key);
            }
            if !valid_color(&org.color) {
                out.push(Problem::ColorInvalid(org.color.clone()));
            }
            if let Some(owner) = &org.github_owner {
                if !valid_owner(owner) {
                    out.push(Problem::OwnerInvalid(owner.clone()));
                }
            }
            for folder in &org.folders {
                match normalize_folder(folder) {
                    None => out.push(Problem::FolderInvalid(folder.clone())),
                    Some(norm) => {
                        if let Some((_, by)) = folders.iter().find(|(f, _)| *f == norm) {
                            out.push(Problem::FolderTaken {
                                folder: norm.clone(),
                                by: (*by).into(),
                            });
                        }
                        folders.push((norm, &org.id));
                    }
                }
            }
        }
        for (key, value) in self.shadow.accounts.iter().chain(&self.shadow.vault) {
            if !valid_id(value) {
                out.push(Problem::ShadowInvalid(key.clone()));
            }
        }
        out
    }

    pub fn create(
        &mut self,
        name: &str,
        github_owner: Option<&str>,
        color: Option<&str>,
    ) -> Result<String, Vec<Problem>> {
        let name = name.trim().to_owned();
        let mut problems = Vec::new();
        if let Some(p) = name_problem(&name) {
            problems.push(p);
        } else if self.name_taken(&name, None) {
            problems.push(Problem::NameTaken(name.clone()));
        }
        let color = color.unwrap_or(DEFAULT_COLOR).trim().to_owned();
        if !valid_color(&color) {
            problems.push(Problem::ColorInvalid(color.clone()));
        }
        let owner = github_owner.map(str::trim).filter(|o| !o.is_empty());
        if let Some(o) = owner.filter(|o| !valid_owner(o)) {
            problems.push(Problem::OwnerInvalid(o.into()));
        }
        if !problems.is_empty() {
            return Err(problems);
        }
        let id = self.fresh_id(&name);
        let order = self
            .organizations
            .iter()
            .filter_map(|o| o.order)
            .max()
            .map_or(1, |n| n + 1);
        self.organizations.push(Organization {
            id: id.clone(),
            name,
            github_owner: owner.map(str::to_owned),
            color,
            folders: Vec::new(),
            order: Some(order),
            machines: None,
            extra: Map::new(),
        });
        Ok(id)
    }

    pub fn rename(&mut self, id: &str, name: &str) -> Result<(), Problem> {
        let name = name.trim().to_owned();
        if let Some(p) = name_problem(&name) {
            return Err(p);
        }
        self.find_mut(id)?;
        if self.name_taken(&name, Some(id)) {
            return Err(Problem::NameTaken(name));
        }
        self.find_mut(id)?.name = name;
        Ok(())
    }

    pub fn set_color(&mut self, id: &str, color: &str) -> Result<(), Problem> {
        let color = color.trim();
        if !valid_color(color) {
            return Err(Problem::ColorInvalid(color.into()));
        }
        self.find_mut(id)?.color = color.into();
        Ok(())
    }

    pub fn set_github_owner(&mut self, id: &str, owner: Option<&str>) -> Result<(), Problem> {
        let owner = owner.map(str::trim).filter(|o| !o.is_empty());
        if let Some(o) = owner.filter(|o| !valid_owner(o)) {
            return Err(Problem::OwnerInvalid(o.into()));
        }
        self.find_mut(id)?.github_owner = owner.map(str::to_owned);
        Ok(())
    }

    pub fn set_machine_settings(
        &mut self,
        id: &str,
        settings: Option<MachineSettings>,
    ) -> Result<(), Problem> {
        self.find_mut(id)?.machines = settings;
        Ok(())
    }

    pub fn set_folders(&mut self, id: &str, folders: &[String]) -> Result<(), Problem> {
        let mut norm: Vec<String> = Vec::new();
        for folder in folders {
            let n =
                normalize_folder(folder).ok_or_else(|| Problem::FolderInvalid(folder.clone()))?;
            if !norm.contains(&n) {
                norm.push(n);
            }
        }
        self.find_mut(id)?;
        for n in &norm {
            let other = self.organizations.iter().find(|o| {
                o.id != id
                    && o.folders
                        .iter()
                        .any(|f| normalize_folder(f).as_deref() == Some(n))
            });
            if let Some(o) = other {
                return Err(Problem::FolderTaken {
                    folder: n.clone(),
                    by: o.id.clone(),
                });
            }
        }
        self.find_mut(id)?.folders = norm;
        Ok(())
    }

    pub fn delete(&mut self, id: &str, counts: Counts) -> Result<(), Problem> {
        if id == PERSONAL_ID {
            return Err(Problem::PersonalNotDeletable);
        }
        let index = self
            .organizations
            .iter()
            .position(|o| o.id == id)
            .ok_or_else(|| Problem::IdUnknown(id.into()))?;
        if !counts.is_empty() {
            return Err(Problem::NotEmpty(counts));
        }
        self.organizations.remove(index);
        Ok(())
    }

    fn name_taken(&self, name: &str, except: Option<&str>) -> bool {
        let key = name_key(name);
        self.organizations
            .iter()
            .any(|o| Some(o.id.as_str()) != except && name_key(&o.name) == key)
    }

    fn fresh_id(&self, name: &str) -> String {
        let base = slug(name);
        let mut id = base.clone();
        let mut n = 2;
        while self.find(&id).is_some() {
            id = format!("{base}-{n}");
            n += 1;
        }
        id
    }
}

fn name_key(name: &str) -> String {
    name.trim().to_lowercase()
}

fn name_problem(name: &str) -> Option<Problem> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Some(Problem::NameEmpty);
    }
    if name.chars().count() > NAME_MAX {
        return Some(Problem::NameTooLong);
    }
    None
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= ID_MAX + 8
        && id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        && !id.starts_with('-')
        && !id.ends_with('-')
}

pub fn valid_color(color: &str) -> bool {
    !color.is_empty()
        && color.len() <= COLOR_MAX
        && color
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

pub fn valid_owner(owner: &str) -> bool {
    !owner.is_empty()
        && owner.len() <= OWNER_MAX
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
        && !owner.starts_with('-')
        && !owner.ends_with('-')
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out: String = out.trim_matches('-').chars().take(ID_MAX).collect();
    let out = out.trim_matches('-').to_owned();
    if out.is_empty() {
        "organization".into()
    } else {
        out
    }
}

pub fn normalize_folder(folder: &str) -> Option<String> {
    let folder = folder.trim();
    let (home, rest) = if folder == "~" {
        (true, "")
    } else if let Some(rest) = folder.strip_prefix("~/") {
        (true, rest)
    } else {
        (false, folder.strip_prefix('/')?)
    };
    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p if p.chars().any(char::is_control) => return None,
            p => parts.push(p),
        }
    }
    match (home, parts.is_empty()) {
        (true, true) => Some("~".into()),
        (true, false) => Some(format!("~/{}", parts.join("/"))),
        (false, true) => None,
        (false, false) => Some(format!("/{}", parts.join("/"))),
    }
}
