use crate::git_ops::Folders;
use crate::layout::Layout;
use crate::op_group::OpGroup;
use notchling_engine::machines::registry::RegistryFiles;
use notchling_engine::machines::roots::MachineRoots;
use notchling_engine::organizations::{
    EditError, FolderPreview, Organization, Organizations, Problem, Relation, COLORS, PERSONAL_ID,
};
use notchling_local_api::{ops, ApiError, Caller, ErrorCode, OpSpec};
use serde_json::{json, Value};

pub struct OrganizationsOps {
    store: Organizations,
    registry: RegistryFiles,
    sessions: Folders,
}

impl OrganizationsOps {
    pub fn new(store: Organizations, registry: RegistryFiles, sessions: Folders) -> Self {
        OrganizationsOps {
            store,
            registry,
            sessions,
        }
    }

    pub fn for_daemon(layout: &Layout, sessions: Folders) -> OrganizationsOps {
        let registry = RegistryFiles {
            read_only: true,
            ..MachineRoots::in_data(&layout.data_dir).registry()
        };
        OrganizationsOps::new(
            Organizations::new(&layout.data_dir).with_home(&layout.home()),
            registry,
            sessions,
        )
    }

    fn list(&self) -> Value {
        let view = self.store.view();
        let counts = self.store.counts(Some(&self.registry));
        let mut rows: Vec<&Organization> = view.organizations().iter().collect();
        rows.sort_by_key(|o| (o.id != PERSONAL_ID, o.order.unwrap_or(i64::MAX)));
        let organizations: Vec<Value> = rows
            .into_iter()
            .map(|o| {
                let c = counts.get(&o.id).copied().unwrap_or_default();
                json!({
                    "id": o.id,
                    "name": o.name,
                    "githubOwner": o.github_owner,
                    "color": o.color,
                    "folders": o.folders,
                    "personal": o.id == PERSONAL_ID,
                    "counts": {"accounts": c.accounts, "secrets": c.secrets, "machines": c.machines},
                })
            })
            .collect();
        json!({
            "organizations": organizations,
            "colors": COLORS,
            "onlyPersonal": view.only_personal(),
            "unreadable": view.why_closed(),
        })
    }

    fn preview(&self, id: &str, folders: &[String]) -> Result<Value, ApiError> {
        let sessions = (self.sessions)();
        let out = self
            .store
            .preview_folders(id, folders, &sessions)
            .map_err(fail)?;
        Ok(preview_json(&out))
    }
}

fn text<'a>(args: &'a Value, key: &str) -> Result<&'a str, ApiError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad_request(format!("{key} is needed")))
}

fn folder_list(args: &Value) -> Result<Vec<String>, ApiError> {
    args.get("folders")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::bad_request("folders is a list of paths"))?
        .iter()
        .map(|f| {
            f.as_str()
                .map(str::to_owned)
                .ok_or_else(|| ApiError::bad_request("folders is a list of paths"))
        })
        .collect()
}

pub fn problem_json(problem: &Problem) -> Value {
    match problem {
        Problem::NameEmpty => json!({"kind": "name_empty"}),
        Problem::NameTooLong => json!({"kind": "name_too_long"}),
        Problem::NameTaken(name) => json!({"kind": "name_taken", "name": name}),
        Problem::IdInvalid(id) => json!({"kind": "id_invalid", "id": id}),
        Problem::IdTaken(id) => json!({"kind": "id_taken", "id": id}),
        Problem::IdUnknown(id) => json!({"kind": "id_unknown", "id": id}),
        Problem::PersonalMissing => json!({"kind": "personal_missing"}),
        Problem::PersonalNotDeletable => json!({"kind": "personal_not_deletable"}),
        Problem::ColorInvalid(color) => json!({"kind": "color_invalid", "color": color}),
        Problem::OwnerInvalid(owner) => json!({"kind": "owner_invalid", "owner": owner}),
        Problem::FolderInvalid(folder) => json!({"kind": "folder_invalid", "folder": folder}),
        Problem::FolderTaken { folder, by } => {
            json!({"kind": "folder_taken", "folder": folder, "by": by})
        }
        Problem::ShadowInvalid(key) => json!({"kind": "shadow_invalid", "key": key}),
        Problem::NotEmpty(c) => json!({
            "kind": "not_empty",
            "counts": {"accounts": c.accounts, "secrets": c.secrets, "machines": c.machines},
        }),
    }
}

fn problem_text(problem: &Problem) -> String {
    match problem {
        Problem::NameEmpty => "An organization needs a name".into(),
        Problem::NameTooLong => "Names are 40 characters at most".into(),
        Problem::NameTaken(name) => format!("{name} is already the name of an organization"),
        Problem::IdInvalid(id) | Problem::IdTaken(id) | Problem::ShadowInvalid(id) => {
            format!("{id} isn't a valid organization id")
        }
        Problem::IdUnknown(id) => format!("There is no organization {id}"),
        Problem::PersonalMissing => "Personal is missing".into(),
        Problem::PersonalNotDeletable => "Personal can't be deleted".into(),
        Problem::ColorInvalid(color) => format!("{color} isn't one of the colours"),
        Problem::OwnerInvalid(owner) => format!("{owner} isn't a GitHub owner"),
        Problem::FolderInvalid(folder) => {
            format!("{folder} isn't a full path (use / or ~/ at the start)")
        }
        Problem::FolderTaken { folder, by } => format!("{folder} already belongs to {by}"),
        Problem::NotEmpty(_) => "The organization still has things in it".into(),
    }
}

pub fn fail(error: EditError) -> ApiError {
    match error {
        EditError::Unreadable(why) => ApiError::new(
            ErrorCode::Failed,
            format!("Organizations can't be read, so nothing was changed ({why})"),
        )
        .with_data(json!({"kind": "unreadable"})),
        EditError::Problems(problems) => {
            let first = problems
                .first()
                .map(problem_text)
                .unwrap_or_else(|| "The change is not valid".into());
            let all: Vec<Value> = problems.iter().map(problem_json).collect();
            ApiError::bad_request(first).with_data(json!({"problems": all}))
        }
        EditError::NoSuchEntry => ApiError::bad_request("There is no such entry"),
        EditError::Store(why) => ApiError::new(
            ErrorCode::Failed,
            format!("Organizations weren't saved ({why})"),
        ),
    }
}

fn preview_json(out: &FolderPreview) -> Value {
    let overlaps: Vec<Value> = out
        .overlaps
        .iter()
        .map(|o| {
            json!({
                "folder": o.folder,
                "organization": o.organization,
                "other": o.other,
                "relation": match o.relation {
                    Relation::Inside => "inside",
                    Relation::Contains => "contains",
                },
            })
        })
        .collect();
    let moved: Vec<Value> = out
        .moved
        .iter()
        .map(|m| json!({"folder": m.folder, "from": m.from, "to": m.to}))
        .collect();
    json!({
        "folders": out.folders,
        "problem": out.problem.as_ref().map(problem_json),
        "problemText": out.problem.as_ref().map(problem_text),
        "overlaps": overlaps,
        "moved": moved,
    })
}

impl OpGroup for OrganizationsOps {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::ORGANIZATIONS_LIST | ops::ORGANIZATIONS_FOLDERS_PREVIEW => Some(OpSpec::APP_QUIET),
            ops::ORGANIZATIONS_CREATE
            | ops::ORGANIZATIONS_RENAME
            | ops::ORGANIZATIONS_COLOR_SET
            | ops::ORGANIZATIONS_FOLDERS_SET
            | ops::ORGANIZATIONS_DELETE => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _caller: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        match op {
            ops::ORGANIZATIONS_LIST => Ok(self.list()),
            ops::ORGANIZATIONS_CREATE => {
                let name = text(args, "name")?;
                let owner = args.get("githubOwner").and_then(Value::as_str);
                let color = args.get("color").and_then(Value::as_str);
                let id = self.store.create(name, owner, color).map_err(fail)?;
                Ok(json!({"id": id}))
            }
            ops::ORGANIZATIONS_RENAME => {
                self.store
                    .rename(text(args, "id")?, text(args, "name")?)
                    .map_err(fail)?;
                Ok(json!({}))
            }
            ops::ORGANIZATIONS_COLOR_SET => {
                self.store
                    .set_color(text(args, "id")?, text(args, "color")?)
                    .map_err(fail)?;
                Ok(json!({}))
            }
            ops::ORGANIZATIONS_FOLDERS_SET => {
                let id = text(args, "id")?;
                self.store
                    .set_folders(id, &folder_list(args)?)
                    .map_err(fail)?;
                let kept = self
                    .store
                    .view()
                    .document()
                    .find(id)
                    .map(|o| o.folders.clone())
                    .unwrap_or_default();
                Ok(json!({"folders": kept}))
            }
            ops::ORGANIZATIONS_FOLDERS_PREVIEW => {
                self.preview(text(args, "id")?, &folder_list(args)?)
            }
            ops::ORGANIZATIONS_DELETE => {
                self.store
                    .delete(text(args, "id")?, &self.registry)
                    .map_err(fail)?;
                Ok(json!({}))
            }
            other => Err(ApiError::internal(format!("{other} has no handler"))),
        }
    }
}
