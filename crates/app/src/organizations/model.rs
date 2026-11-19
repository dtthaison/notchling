use notchling_engine::organizations::{NAME_MAX, PERSONAL_ID};
use notchling_locale::{self as t, fmt};
use serde_json::Value;

pub const MOVED_SHOWN: usize = 6;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    pub accounts: u64,
    pub secrets: u64,
    pub machines: u64,
}

impl Counts {
    fn parse(value: &Value) -> Counts {
        let n = |key: &str| value.get(key).and_then(Value::as_u64).unwrap_or(0);
        Counts {
            accounts: n("accounts"),
            secrets: n("secrets"),
            machines: n("machines"),
        }
    }

    pub fn is_empty(&self) -> bool {
        *self == Counts::default()
    }

    pub fn line(&self) -> String {
        t::organizations::counts(
            &t::organizations::count_accounts(self.accounts),
            &t::organizations::count_secrets(self.secrets),
            &t::organizations::count_machines(self.machines),
        )
    }

    pub fn things(&self) -> String {
        let mut parts = Vec::new();
        if self.accounts > 0 {
            parts.push(t::organizations::count_accounts(self.accounts));
        }
        if self.secrets > 0 {
            parts.push(t::organizations::count_secrets(self.secrets));
        }
        if self.machines > 0 {
            parts.push(t::organizations::count_machines(self.machines));
        }
        fmt::list(&parts)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: String,
    pub name: String,
    pub color: String,
    pub github_owner: Option<String>,
    pub folders: Vec<String>,
    pub personal: bool,
    pub counts: Counts,
}

impl Row {
    fn parse(value: &Value) -> Option<Row> {
        let text = |key: &str| value.get(key).and_then(Value::as_str).map(str::to_owned);
        let id = text("id")?;
        Some(Row {
            personal: value.get("personal").and_then(Value::as_bool) == Some(true)
                || id == PERSONAL_ID,
            name: text("name")?,
            color: text("color").unwrap_or_else(|| "grey".into()),
            github_owner: text("githubOwner"),
            folders: value
                .get("folders")
                .and_then(Value::as_array)
                .map(|f| {
                    f.iter()
                        .filter_map(|f| f.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default(),
            counts: value.get("counts").map(Counts::parse).unwrap_or_default(),
            id,
        })
    }

    pub fn initial(&self) -> String {
        self.name
            .chars()
            .next()
            .map(|c| c.to_uppercase().collect())
            .unwrap_or_default()
    }

    pub fn refusal(&self) -> Option<String> {
        if self.personal {
            Some(t::organizations::error_personal().to_owned())
        } else if !self.counts.is_empty() {
            Some(t::organizations::error_not_empty(
                &self.name,
                &self.counts.things(),
            ))
        } else {
            None
        }
    }
}

pub fn colour_name(color: &str) -> String {
    match color {
        "grey" => t::organizations::colour_grey(),
        "orange" => t::organizations::colour_orange(),
        "yellow" => t::organizations::colour_yellow(),
        "lime" => t::organizations::colour_lime(),
        "teal" => t::organizations::colour_teal(),
        "blue" => t::organizations::colour_blue(),
        "purple" => t::organizations::colour_purple(),
        "pink" => t::organizations::colour_pink(),
        "red" => t::organizations::colour_red(),
        other => other,
    }
    .to_owned()
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Load {
    #[default]
    Idle,
    Loading,
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Preview {
    Checking,
    Ready(Notes),
    Refused(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Notes {
    pub overlaps: Vec<String>,
    pub moved: Vec<String>,
    pub head: String,
    pub more: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FolderEdit {
    pub id: String,
    pub drafts: Vec<String>,
    pub preview: Preview,
    pub error: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum Open {
    #[default]
    Nothing,
    Menu(String),
    New,
    Rename(String),
    Colour(String),
    Folders(FolderEdit),
    ConfirmDelete(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notice {
    pub row: Option<String>,
    pub text: String,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrganizationsModel {
    pub rows: Vec<Row>,
    pub colors: Vec<String>,
    pub unreadable: Option<String>,
    pub load: Load,
    pub open: Open,
    pub notice: Option<Notice>,
    pub new_color: Option<String>,
    pub home: String,
    pub reading: bool,
}

impl OrganizationsModel {
    pub fn start(&mut self) -> bool {
        if self.reading {
            return false;
        }
        self.reading = true;
        if self.rows.is_empty() {
            self.load = Load::Loading;
        }
        true
    }

    pub fn finish(&mut self, reply: Result<Value, String>) {
        self.reading = false;
        match reply {
            Err(why) => self.load = Load::Failed(why),
            Ok(value) => {
                self.load = Load::Idle;
                self.rows = value
                    .get("organizations")
                    .and_then(Value::as_array)
                    .map(|rows| rows.iter().filter_map(Row::parse).collect())
                    .unwrap_or_default();
                self.colors = value
                    .get("colors")
                    .and_then(Value::as_array)
                    .map(|c| {
                        c.iter()
                            .filter_map(|c| c.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default();
                self.unreadable = value
                    .get("unreadable")
                    .and_then(Value::as_str)
                    .map(str::to_owned);
                self.forget_gone_rows();
            }
        }
    }

    fn forget_gone_rows(&mut self) {
        let here = |id: &str| self.rows.iter().any(|r| r.id == id);
        let gone = match &self.open {
            Open::Menu(id) | Open::Rename(id) | Open::Colour(id) | Open::ConfirmDelete(id) => {
                !here(id)
            }
            Open::Folders(edit) => !here(&edit.id),
            Open::Nothing | Open::New => false,
        };
        if gone {
            self.open = Open::Nothing;
        }
        if self
            .notice
            .as_ref()
            .and_then(|n| n.row.as_deref())
            .is_some_and(|id| !here(id))
        {
            self.notice = None;
        }
    }

    pub fn row(&self, id: &str) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }

    pub fn name_of(&self, id: &str) -> String {
        self.row(id)
            .map_or_else(|| id.to_owned(), |r| r.name.clone())
    }

    pub fn only_personal(&self) -> bool {
        self.rows.iter().all(|r| r.personal)
    }

    pub fn editable(&self) -> bool {
        self.unreadable.is_none()
    }

    pub fn toggle_menu(&mut self, id: &str) {
        self.notice = None;
        self.open = match &self.open {
            Open::Menu(open) if open == id => Open::Nothing,
            _ => Open::Menu(id.to_owned()),
        };
    }

    pub fn close(&mut self) {
        self.open = Open::Nothing;
        self.notice = None;
    }

    pub fn begin_new(&mut self) {
        self.notice = None;
        self.new_color = None;
        self.open = Open::New;
    }

    pub fn begin_rename(&mut self, id: &str) {
        self.notice = None;
        self.open = Open::Rename(id.to_owned());
    }

    pub fn begin_colour(&mut self, id: &str) {
        self.notice = None;
        self.open = Open::Colour(id.to_owned());
    }

    pub fn begin_folders(&mut self, id: &str) {
        self.notice = None;
        let drafts = self.row(id).map(|r| r.folders.clone()).unwrap_or_default();
        self.open = Open::Folders(FolderEdit {
            id: id.to_owned(),
            drafts,
            preview: Preview::Ready(Notes::default()),
            error: None,
        });
    }

    pub fn ask_delete(&mut self, id: &str) {
        let refusal = self.row(id).and_then(Row::refusal);
        self.open = Open::Nothing;
        match refusal {
            Some(text) => {
                self.notice = Some(Notice {
                    row: Some(id.to_owned()),
                    text,
                })
            }
            None => {
                self.notice = None;
                self.open = Open::ConfirmDelete(id.to_owned());
            }
        }
    }

    pub fn fail(&mut self, row: Option<&str>, message: &str, data: Option<&Value>) {
        let subject = row.map(|id| self.name_of(id)).unwrap_or_default();
        self.notice = Some(Notice {
            row: row.map(str::to_owned),
            text: self.failure_text(message, data, &subject),
        });
    }

    pub fn failure_text(&self, message: &str, data: Option<&Value>, subject: &str) -> String {
        let problem = data
            .and_then(|d| d.get("problems"))
            .and_then(Value::as_array)
            .and_then(|p| p.first());
        match problem {
            Some(problem) => self.problem_text(problem, subject),
            None if data.and_then(|d| d.get("kind")).and_then(Value::as_str)
                == Some("unreadable") =>
            {
                t::organizations::unreadable(message)
            }
            None => t::organizations::error_failed(message),
        }
    }

    pub fn problem_text(&self, problem: &Value, subject: &str) -> String {
        let text = |key: &str| problem.get(key).and_then(Value::as_str).unwrap_or_default();
        match text("kind") {
            "name_empty" => t::organizations::error_name_empty().to_owned(),
            "name_too_long" => t::organizations::error_name_too_long(&fmt::count(NAME_MAX as u64)),
            "name_taken" => t::organizations::error_name_taken(text("name")),
            "color_invalid" => t::organizations::error_colour(text("color")),
            "folder_invalid" => t::organizations::error_folder_invalid(text("folder")),
            "folder_taken" => {
                t::organizations::error_folder_taken(text("folder"), &self.name_of(text("by")))
            }
            "personal_not_deletable" => t::organizations::error_personal().to_owned(),
            "not_empty" => t::organizations::error_not_empty(
                subject,
                &problem
                    .get("counts")
                    .map(Counts::parse)
                    .unwrap_or_default()
                    .things(),
            ),
            "id_unknown" => t::organizations::error_gone().to_owned(),
            _ => t::organizations::error_failed(text("kind")),
        }
    }

    pub fn tilde(&self, folder: &str) -> String {
        match folder.strip_prefix(self.home.as_str()) {
            Some(rest) if !self.home.is_empty() && rest.starts_with('/') => format!("~{rest}"),
            _ => folder.to_owned(),
        }
    }

    pub fn tilde_line(&self, row: &Row) -> String {
        if row.folders.is_empty() {
            t::organizations::folders_none().to_owned()
        } else {
            let shown: Vec<String> = row.folders.iter().map(|f| self.tilde(f)).collect();
            t::organizations::folders_line(&fmt::list(&shown))
        }
    }

    pub fn drafts(&self) -> Option<&[String]> {
        match &self.open {
            Open::Folders(edit) => Some(&edit.drafts),
            _ => None,
        }
    }

    pub fn add_folder(&mut self, typed: &str) -> bool {
        let typed = typed.trim();
        let Open::Folders(edit) = &mut self.open else {
            return false;
        };
        if typed.is_empty() || edit.drafts.iter().any(|d| d == typed) {
            return false;
        }
        edit.drafts.push(typed.to_owned());
        edit.preview = Preview::Checking;
        edit.error = None;
        true
    }

    pub fn remove_folder(&mut self, index: usize) -> bool {
        let Open::Folders(edit) = &mut self.open else {
            return false;
        };
        if index >= edit.drafts.len() {
            return false;
        }
        edit.drafts.remove(index);
        edit.preview = Preview::Checking;
        edit.error = None;
        true
    }

    pub fn folders_changed(&self) -> bool {
        match &self.open {
            Open::Folders(edit) => self.row(&edit.id).is_some_and(|r| r.folders != edit.drafts),
            _ => false,
        }
    }

    pub fn can_save_folders(&self) -> bool {
        match &self.open {
            Open::Folders(edit) => {
                self.folders_changed() && matches!(edit.preview, Preview::Ready(_))
            }
            _ => false,
        }
    }

    pub fn finish_preview(
        &mut self,
        id: &str,
        drafts: &[String],
        reply: Result<Value, (String, Option<Value>)>,
    ) {
        let preview = match &reply {
            Ok(value) => Some(self.parse_preview(value)),
            Err((message, data)) => Some(Preview::Refused(self.failure_text(
                message,
                data.as_ref(),
                "",
            ))),
        };
        let kept: Option<Vec<String>> = match &reply {
            Ok(value) if value.get("problem").is_none_or(Value::is_null) => {
                value.get("folders").and_then(Value::as_array).map(|f| {
                    f.iter()
                        .filter_map(|f| f.as_str().map(str::to_owned))
                        .collect()
                })
            }
            _ => None,
        };
        let Open::Folders(edit) = &mut self.open else {
            return;
        };
        if edit.id != id || edit.drafts != drafts {
            return;
        }
        if let Some(kept) = kept {
            edit.drafts = kept;
        }
        if let Some(preview) = preview {
            edit.preview = preview;
        }
    }

    pub fn parse_preview(&self, reply: &Value) -> Preview {
        if let Some(problem) = reply.get("problem").filter(|p| !p.is_null()) {
            return Preview::Refused(self.problem_text(problem, ""));
        }
        let text = |v: &Value, key: &str| {
            v.get(key)
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let overlaps = reply
            .get("overlaps")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|o| {
                        let (folder, other) = (
                            self.tilde(&text(o, "folder")),
                            self.tilde(&text(o, "other")),
                        );
                        let name = self.name_of(&text(o, "organization"));
                        if text(o, "relation") == "contains" {
                            t::organizations::folders_contains(&folder, &other, &name)
                        } else {
                            t::organizations::folders_inside(&folder, &other, &name)
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        let moved_all: Vec<String> = reply
            .get("moved")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .map(|m| {
                        t::organizations::folders_moved_row(
                            &self.tilde(&text(m, "folder")),
                            &self.name_of(&text(m, "from")),
                            &self.name_of(&text(m, "to")),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        let total = moved_all.len();
        let more = (total > MOVED_SHOWN).then(|| {
            t::organizations::folders_moved_more(&fmt::count((total - MOVED_SHOWN) as u64))
        });
        let head = if total == 0 {
            t::organizations::folders_moved_none().to_owned()
        } else {
            t::organizations::folders_moved(total as u64)
        };
        Preview::Ready(Notes {
            overlaps,
            moved: moved_all.into_iter().take(MOVED_SHOWN).collect(),
            head,
            more,
        })
    }
}
