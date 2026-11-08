//! The Add machine and sd worker job sheets as typed state (Swift `AddMachineSheet`, `JobSheet`):
//! which field has the focus, the text in each, and what the sheet's buttons may do. The checks
//! are the engine's (`machines::forms`); typing goes through the notch's `type_key`, so no control
//! character ever gets in.

use notchling_engine::machines::forms::{
    job_problems, machine_problems, JobDraft, JobRepoDraft, KIND_CHOICES,
};

use super::pieces::DraftCheck;
use crate::notch::answer::{type_key, Typed};

/// A field of the Add sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddField {
    Name,
    Host,
    User,
}

impl AddField {
    pub const ALL: [AddField; 3] = [AddField::Name, AddField::Host, AddField::User];

    pub fn title(self) -> &'static str {
        match self {
            AddField::Name => "Name",
            AddField::Host => "Host",
            AddField::User => "User",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            AddField::Name => "m1",
            AddField::Host => "m1 or m1.tail1234.ts.net",
            AddField::User => "worker",
        }
    }

    fn next(self) -> AddField {
        match self {
            AddField::Name => AddField::Host,
            AddField::Host => AddField::User,
            AddField::User => AddField::Name,
        }
    }
}

pub const ADD_INTRO: &str = "Name it (m1), give its Tailscale name or address, and the user the worker runs as. Check only reads from the machine.";

/// The Add machine sheet.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddForm {
    pub name: String,
    pub host: String,
    pub user: String,
    pub focus: Option<AddField>,
    /// The last Check, for the fields as they were then.
    pub check: Option<DraftCheck>,
    pub checking: bool,
    pub error: Option<String>,
}

/// Printable text only: a paste can't carry a control character into a field.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

impl AddForm {
    pub fn new() -> AddForm {
        AddForm {
            focus: Some(AddField::Name),
            ..AddForm::default()
        }
    }

    pub fn text(&self, field: AddField) -> &str {
        match field {
            AddField::Name => &self.name,
            AddField::Host => &self.host,
            AddField::User => &self.user,
        }
    }

    fn text_mut(&mut self, field: AddField) -> &mut String {
        match field {
            AddField::Name => &mut self.name,
            AddField::Host => &mut self.host,
            AddField::User => &mut self.user,
        }
    }

    pub fn inputs(&self) -> Vec<String> {
        vec![self.name.clone(), self.host.clone(), self.user.clone()]
    }

    /// A key in the focused field. A change drops the last Check and error, since they were for
    /// other details; Tab moves on; Return is the caller's (Check, or Add once checked).
    pub fn type_key(&mut self, key: &str, key_char: Option<&str>, modified: bool) -> Typed {
        if key == "tab" {
            self.focus = Some(self.focus.map_or(AddField::Name, AddField::next));
            return Typed::Changed;
        }
        let Some(field) = self.focus else {
            return Typed::Ignored;
        };
        let typed = type_key(self.text_mut(field), key, key_char, modified);
        if typed == Typed::Changed {
            self.changed();
        }
        typed
    }

    pub fn paste(&mut self, text: &str) {
        if let Some(field) = self.focus {
            self.text_mut(field).push_str(&printable(text));
            self.changed();
        }
    }

    fn changed(&mut self) {
        self.check = None;
        self.error = None;
    }

    /// The problems shown under the fields: none while nothing is typed yet.
    pub fn problems(&self, existing: &[String]) -> Vec<String> {
        if self.name.is_empty() && self.host.is_empty() && self.user.is_empty() {
            return Vec::new();
        }
        machine_problems(&self.name, &self.host, &self.user, existing)
    }

    pub fn form_ok(&self, existing: &[String]) -> bool {
        !self.name.is_empty()
            && !self.host.is_empty()
            && !self.user.is_empty()
            && machine_problems(&self.name, &self.host, &self.user, existing).is_empty()
    }

    pub fn can_check(&self, existing: &[String], can_add: bool) -> bool {
        self.form_ok(existing) && !self.checking && can_add
    }

    /// Add: only after a Check found the machine reachable, for these same details.
    pub fn can_add(&self, existing: &[String], can_add: bool) -> bool {
        self.check
            .as_ref()
            .is_some_and(|c| c.reachable && c.inputs == self.inputs())
            && self.form_ok(existing)
            && can_add
            && !self.checking
    }

    /// A Check came back; kept only when the fields are still what it was made for.
    pub fn checked(&mut self, check: DraftCheck) {
        self.checking = false;
        if check.inputs == self.inputs() {
            self.check = Some(check);
        }
    }
}

/// A field of one repository row of the job sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RepoField {
    Github,
    LocalPath,
    CheckCommand,
    MachinePath,
    VerifySkill,
}

impl RepoField {
    pub const ALL: [RepoField; 5] = [
        RepoField::Github,
        RepoField::LocalPath,
        RepoField::CheckCommand,
        RepoField::MachinePath,
        RepoField::VerifySkill,
    ];

    pub fn title(self) -> &'static str {
        match self {
            RepoField::Github => "Repository",
            RepoField::LocalPath => "Checkout here",
            RepoField::CheckCommand => "Check command",
            RepoField::MachinePath => "Machine path",
            RepoField::VerifySkill => "Verify skill",
        }
    }

    pub fn placeholder(self) -> &'static str {
        match self {
            RepoField::Github => "owner/name",
            RepoField::LocalPath => "/Users/you/code/name",
            RepoField::CheckCommand => "just check-affected",
            RepoField::MachinePath => "default ~/src/<name>",
            RepoField::VerifySkill => ".claude/skills/verify/SKILL.md (optional)",
        }
    }
}

/// A field of the job sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobField {
    Repo(usize, RepoField),
    Health,
}

pub const HEALTH_PLACEHOLDER: &str = "https://example.com/health (optional)";
pub const ALL_REPOS_NOTE: &str = "Token scope then accepts other private repos. The token still needs Issues read/write only and no code access.";

/// The sd worker job sheet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JobForm {
    pub draft: JobDraft,
    pub focus: Option<JobField>,
    pub error: Option<String>,
}

impl JobForm {
    pub fn new(draft: JobDraft) -> JobForm {
        let focus = (!draft.repos.is_empty()).then_some(JobField::Repo(0, RepoField::Github));
        JobForm {
            draft,
            focus,
            error: None,
        }
    }

    pub fn title(&self) -> &'static str {
        if self.draft.is_new {
            "Add sd Worker Job"
        } else {
            "Edit sd Worker Job"
        }
    }

    pub fn text(&self, field: JobField) -> &str {
        match field {
            JobField::Health => &self.draft.health_url,
            JobField::Repo(i, f) => self.draft.repos.get(i).map_or("", |r| match f {
                RepoField::Github => &r.github,
                RepoField::LocalPath => &r.local_path,
                RepoField::CheckCommand => &r.check_command,
                RepoField::MachinePath => &r.machine_path,
                RepoField::VerifySkill => &r.verify_skill,
            }),
        }
    }

    fn text_mut(&mut self, field: JobField) -> Option<&mut String> {
        match field {
            JobField::Health => Some(&mut self.draft.health_url),
            JobField::Repo(i, f) => self.draft.repos.get_mut(i).map(|r| match f {
                RepoField::Github => &mut r.github,
                RepoField::LocalPath => &mut r.local_path,
                RepoField::CheckCommand => &mut r.check_command,
                RepoField::MachinePath => &mut r.machine_path,
                RepoField::VerifySkill => &mut r.verify_skill,
            }),
        }
    }

    /// Every field in sheet order, for Tab.
    fn fields(&self) -> Vec<JobField> {
        let mut out = Vec::new();
        for i in 0..self.draft.repos.len() {
            out.extend(RepoField::ALL.iter().map(|f| JobField::Repo(i, *f)));
        }
        out.push(JobField::Health);
        out
    }

    pub fn type_key(&mut self, key: &str, key_char: Option<&str>, modified: bool) -> Typed {
        if key == "tab" {
            let fields = self.fields();
            let at = self.focus.and_then(|f| fields.iter().position(|x| *x == f));
            self.focus = fields
                .get(at.map_or(0, |i| (i + 1) % fields.len()))
                .copied();
            return Typed::Changed;
        }
        let Some(field) = self.focus else {
            return Typed::Ignored;
        };
        let Some(text) = self.text_mut(field) else {
            return Typed::Ignored;
        };
        let typed = type_key(text, key, key_char, modified);
        if typed == Typed::Changed {
            self.error = None;
        }
        typed
    }

    pub fn paste(&mut self, text: &str) {
        let Some(field) = self.focus else {
            return;
        };
        if let Some(t) = self.text_mut(field) {
            t.push_str(&printable(text));
            self.error = None;
        }
    }

    pub fn add_repo(&mut self) {
        self.draft.repos.push(JobRepoDraft::default());
        self.focus = Some(JobField::Repo(
            self.draft.repos.len() - 1,
            RepoField::Github,
        ));
    }

    pub fn remove_repo(&mut self, index: usize) {
        if index < self.draft.repos.len() {
            self.draft.repos.remove(index);
            self.focus = None;
        }
    }

    pub fn kinds(&self) -> &'static [&'static str] {
        &KIND_CHOICES
    }

    pub fn has_kind(&self, kind: &str) -> bool {
        self.draft.kinds.iter().any(|k| k == kind)
    }

    pub fn toggle_kind(&mut self, kind: &str) {
        if self.has_kind(kind) {
            self.draft.kinds.retain(|k| k != kind);
        } else {
            self.draft.kinds.push(kind.to_string());
        }
    }

    pub fn toggle_all_repos(&mut self) {
        self.draft.token_all_repos = !self.draft.token_all_repos;
    }

    /// The first three problems, shown under the fields.
    pub fn problems(&self) -> Vec<String> {
        job_problems(&self.draft).into_iter().take(3).collect()
    }

    pub fn can_save(&self) -> bool {
        job_problems(&self.draft).is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_text(form: &mut AddForm, text: &str) {
        for c in text.chars() {
            let s = c.to_string();
            form.type_key(&s, Some(&s), false);
        }
    }

    #[test]
    fn add_form_types_tabs_and_refuses_until_checked() {
        let mut f = AddForm::new();
        let existing = vec!["m2".to_string()];
        assert!(
            f.problems(&existing).is_empty(),
            "nothing typed, nothing said"
        );
        type_text(&mut f, "m1");
        f.type_key("tab", None, false);
        type_text(&mut f, "m1.test");
        f.type_key("tab", None, false);
        type_text(&mut f, "worker");
        assert_eq!(
            (f.name.as_str(), f.host.as_str(), f.user.as_str()),
            ("m1", "m1.test", "worker")
        );
        assert!(f.form_ok(&existing) && f.can_check(&existing, true));
        assert!(
            !f.can_check(&existing, false),
            "no Check while machines.json can't be read"
        );
        assert!(!f.can_add(&existing, true), "no Add before a Check");
        let check = DraftCheck {
            reachable: true,
            inputs: f.inputs(),
            ..DraftCheck::default()
        };
        f.checking = true;
        f.checked(check);
        assert!(f.can_add(&existing, true));
        // a change drops the check
        f.type_key("backspace", None, false);
        assert_eq!(f.check, None);
        assert!(!f.can_add(&existing, true));
        // a check for other details is dropped
        let stale = DraftCheck {
            reachable: true,
            inputs: vec!["x".into(), "y".into(), "z".into()],
            ..DraftCheck::default()
        };
        f.checked(stale);
        assert_eq!(f.check, None);
    }

    #[test]
    fn add_form_names_problems_and_pastes_printable_text_only() {
        let mut f = AddForm::new();
        type_text(&mut f, "M1");
        let problems = f.problems(&[]);
        assert_eq!(problems.len(), 3, "{problems:?}");
        f.focus = Some(AddField::Host);
        f.paste("m1.\x1btest\n");
        assert_eq!(f.host, "m1.test");
        assert_eq!(f.type_key("escape", Some("\u{1b}"), false), Typed::Ignored);
        assert_eq!(f.type_key("enter", Some("\r"), false), Typed::Submit);
    }

    #[test]
    fn job_form_edits_repos_kinds_and_saves_only_when_whole() {
        let mut f = JobForm::new(JobDraft::new("m1"));
        assert_eq!(f.title(), "Add sd Worker Job");
        assert!(!f.can_save());
        assert_eq!(f.problems(), ["Add at least one repository"]);
        f.add_repo();
        assert_eq!(f.focus, Some(JobField::Repo(0, RepoField::Github)));
        for c in "me/app".chars() {
            let s = c.to_string();
            f.type_key(&s, Some(&s), false);
        }
        f.type_key("tab", None, false);
        assert_eq!(f.focus, Some(JobField::Repo(0, RepoField::LocalPath)));
        f.paste("/Users/me/code/app");
        f.type_key("tab", None, false);
        f.paste("just check");
        assert!(f.can_save(), "{:?}", f.problems());
        f.toggle_kind("ios");
        assert!(f.has_kind("ios"));
        f.toggle_kind("ios");
        assert!(!f.has_kind("ios"));
        f.toggle_all_repos();
        assert!(f.draft.token_all_repos);
        // Tab from the last repo field reaches Health, then wraps
        f.focus = Some(JobField::Repo(0, RepoField::VerifySkill));
        f.type_key("tab", None, false);
        assert_eq!(f.focus, Some(JobField::Health));
        f.paste("ftp://x");
        assert_eq!(f.problems(), ["Health URL: http or https"]);
        f.type_key("tab", None, false);
        assert_eq!(f.focus, Some(JobField::Repo(0, RepoField::Github)));
        f.remove_repo(0);
        assert!(f.draft.repos.is_empty() && f.focus.is_none());
        assert_eq!(
            f.text(JobField::Repo(3, RepoField::Github)),
            "",
            "a gone row reads empty"
        );
    }
}
