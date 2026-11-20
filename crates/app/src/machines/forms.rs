use notchling_engine::machines::forms::{
    job_problems, machine_problems, JobDraft, JobRepoDraft, KIND_CHOICES,
};

use super::pieces::DraftCheck;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    fn previous(self) -> AddField {
        match self {
            AddField::Name => AddField::User,
            AddField::Host => AddField::Name,
            AddField::User => AddField::Host,
        }
    }
}

pub const ADD_INTRO: &str = "Name it (m1), give its Tailscale name or address, and the user the worker runs as. Check only reads from the machine.";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AddForm {
    pub name: String,
    pub host: String,
    pub user: String,
    pub focus: Option<AddField>,
    pub check: Option<DraftCheck>,
    pub checking: bool,
    pub error: Option<String>,
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

    pub fn set_text(&mut self, field: AddField, text: &str) {
        if self.text(field) != text {
            *self.text_mut(field) = text.to_owned();
            self.changed();
        }
    }

    pub fn tab_from(&mut self, from: Option<AddField>) -> AddField {
        let next = from.or(self.focus).map_or(AddField::Name, AddField::next);
        self.focus = Some(next);
        next
    }

    pub fn tab_back_from(&mut self, from: Option<AddField>) -> AddField {
        let back = from
            .or(self.focus)
            .map_or(AddField::User, AddField::previous);
        self.focus = Some(back);
        back
    }

    fn changed(&mut self) {
        self.check = None;
        self.error = None;
    }

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

    pub fn can_add(&self, existing: &[String], can_add: bool) -> bool {
        self.check
            .as_ref()
            .is_some_and(|c| c.reachable && c.inputs == self.inputs())
            && self.form_ok(existing)
            && can_add
            && !self.checking
    }

    pub fn checked(&mut self, check: DraftCheck) {
        self.checking = false;
        if check.inputs == self.inputs() {
            self.check = Some(check);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobField {
    Repo(usize, RepoField),
    Health,
}

pub const HEALTH_PLACEHOLDER: &str = "https://example.com/health (optional)";
pub const ALL_REPOS_NOTE: &str = "Token scope then accepts other private repos. The token still needs Issues read/write only and no code access.";

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

    pub fn fields(&self) -> Vec<JobField> {
        let mut out = Vec::new();
        for i in 0..self.draft.repos.len() {
            out.extend(RepoField::ALL.iter().map(|f| JobField::Repo(i, *f)));
        }
        out.push(JobField::Health);
        out
    }

    pub fn set_text(&mut self, field: JobField, text: &str) {
        if self.text(field) == text {
            return;
        }
        if let Some(current) = self.text_mut(field) {
            *current = text.to_owned();
            self.error = None;
        }
    }

    pub fn tab_from(&mut self, from: Option<JobField>) -> Option<JobField> {
        let fields = self.fields();
        let at = from
            .or(self.focus)
            .and_then(|f| fields.iter().position(|x| *x == f));
        self.focus = fields
            .get(at.map_or(0, |i| (i + 1) % fields.len()))
            .copied();
        self.focus
    }

    pub fn tab_back_from(&mut self, from: Option<JobField>) -> Option<JobField> {
        let fields = self.fields();
        let at = from
            .or(self.focus)
            .and_then(|f| fields.iter().position(|x| *x == f));
        self.focus = fields
            .get(at.map_or(fields.len().saturating_sub(1), |i| {
                (i + fields.len() - 1) % fields.len()
            }))
            .copied();
        self.focus
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

    fn type_text(form: &mut AddForm, field: AddField, text: &str) {
        form.set_text(field, text);
    }

    #[test]
    fn add_form_types_tabs_and_refuses_until_checked() {
        let mut f = AddForm::new();
        let existing = vec!["m2".to_string()];
        assert!(
            f.problems(&existing).is_empty(),
            "nothing typed, nothing said"
        );
        type_text(&mut f, AddField::Name, "m1");
        assert_eq!(f.tab_from(None), AddField::Host);
        type_text(&mut f, AddField::Host, "m1.test");
        assert_eq!(f.tab_from(None), AddField::User);
        type_text(&mut f, AddField::User, "worker");
        assert_eq!(f.tab_from(Some(AddField::User)), AddField::Name);
        assert_eq!(f.tab_back_from(Some(AddField::Name)), AddField::User);
        assert_eq!(f.tab_back_from(Some(AddField::User)), AddField::Host);
        assert_eq!(f.tab_back_from(None), AddField::Name);
        f.focus = None;
        assert_eq!(f.tab_back_from(None), AddField::User);
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
        type_text(&mut f, AddField::User, "worke");
        assert_eq!(f.check, None);
        assert!(!f.can_add(&existing, true));
        let stale = DraftCheck {
            reachable: true,
            inputs: vec!["x".into(), "y".into(), "z".into()],
            ..DraftCheck::default()
        };
        f.checked(stale);
        assert_eq!(f.check, None);
    }

    #[test]
    fn add_form_names_problems_and_forgets_a_check_when_a_field_changes() {
        let mut f = AddForm::new();
        type_text(&mut f, AddField::Name, "M1");
        let problems = f.problems(&[]);
        assert_eq!(problems.len(), 3, "{problems:?}");
        f.check = Some(DraftCheck::default());
        f.set_text(AddField::Name, "M1");
        assert!(f.check.is_some(), "the same text is no change");
        f.set_text(AddField::Host, "m1.test");
        assert_eq!(f.check, None);
        assert_eq!(f.host, "m1.test");
    }

    #[test]
    fn job_form_edits_repos_kinds_and_saves_only_when_whole() {
        let mut f = JobForm::new(JobDraft::new("m1"));
        assert_eq!(f.title(), "Add sd Worker Job");
        assert!(!f.can_save());
        assert_eq!(f.problems(), ["Add at least one repository"]);
        f.add_repo();
        assert_eq!(f.focus, Some(JobField::Repo(0, RepoField::Github)));
        f.set_text(JobField::Repo(0, RepoField::Github), "me/app");
        f.tab_from(None);
        assert_eq!(f.focus, Some(JobField::Repo(0, RepoField::LocalPath)));
        f.set_text(
            JobField::Repo(0, RepoField::LocalPath),
            "/Users/me/code/app",
        );
        f.tab_from(None);
        f.set_text(JobField::Repo(0, RepoField::CheckCommand), "just check");
        assert!(f.can_save(), "{:?}", f.problems());
        f.toggle_kind("ios");
        assert!(f.has_kind("ios"));
        f.toggle_kind("ios");
        assert!(!f.has_kind("ios"));
        f.toggle_all_repos();
        assert!(f.draft.token_all_repos);
        f.focus = Some(JobField::Repo(0, RepoField::VerifySkill));
        f.tab_from(None);
        assert_eq!(f.focus, Some(JobField::Health));
        f.set_text(JobField::Health, "ftp://x");
        assert_eq!(f.problems(), ["Health URL: http or https"]);
        f.tab_from(Some(JobField::Health));
        assert_eq!(f.focus, Some(JobField::Repo(0, RepoField::Github)));
        assert_eq!(f.tab_back_from(None), Some(JobField::Health));
        assert_eq!(
            f.tab_back_from(Some(JobField::Health)),
            Some(JobField::Repo(0, RepoField::VerifySkill))
        );
        f.focus = None;
        assert_eq!(f.tab_back_from(None), Some(JobField::Health));
        f.remove_repo(0);
        assert!(f.draft.repos.is_empty() && f.focus.is_none());
        assert_eq!(
            f.text(JobField::Repo(3, RepoField::Github)),
            "",
            "a gone row reads empty"
        );
    }
}
