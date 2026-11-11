//! The New Session sheet, pure (part U8, `NewSessionSheet.swift`): an account column (every Claude
//! Code account: the main login, each lane and each saved account, which gets a lane first; then
//! each Codex and Antigravity account), a folder column, then Start. Claude Code asks the daemon
//! for `session.new`; a folder it hasn't trusted is refused, and the Trust Folder pop-up then
//! offers `folder.trust` and starts again. Codex and Antigravity switch login first when the
//! account isn't the one in use, then open a draft row whose first message starts the session.

use std::collections::BTreeSet;
use std::path::Path;

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};

use crate::accounts::menu::{Place, Target};
use crate::accounts::model::{ANTIGRAVITY, CODEX};
use crate::accounts::wire::{Outcome, Provider, ProviderAccount};
use crate::acting::{self, Acting, Control, Request};
use crate::chat::data::{Session, Tool};
use crate::tools::requests::{AGY_DRAFT_PREFIX, CODEX_DRAFT_PREFIX};

/// The most folders the column lists (`chatProjectFolders` keeps 30).
pub const MAX_FOLDERS: usize = 30;

/// One account to start on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountChoice {
    pub title: String,
    pub detail: String,
    pub section: Section,
    pub pick: Pick,
}

/// The tool an account is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section {
    Claude,
    Codex,
    Antigravity,
}

impl Section {
    pub fn title(self) -> &'static str {
        match self {
            Section::Claude => "Claude Code",
            Section::Codex => "Codex",
            Section::Antigravity => "Antigravity",
        }
    }
}

/// What starting on an account takes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pick {
    /// Claude Code in this config folder (`None`: the main login).
    Claude(Option<String>),
    /// A saved Claude Code account no folder holds yet: a lane is made for it first.
    ClaudeSaved(String),
    /// Codex or Antigravity (`provider`) on `account`; `switch` when it isn't the login in use,
    /// since each has one login.
    Tool {
        provider: String,
        account: String,
        switch: bool,
    },
}

/// What Start asks of the view once picked.
#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    /// `session.new`.
    Request(Request),
    /// A lane for `account`, then `session.new` there ([`NewSession::lane_made`]).
    Lane { account: String },
    /// Switch `provider` to `account` first when `switch`, then a draft row in `folder`: its
    /// first message starts the session.
    Draft {
        provider: String,
        account: String,
        switch: bool,
        folder: String,
    },
}

/// Where the sheet is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Choosing,
    /// `session.new` is out.
    Starting,
    /// Claude Code refused the folder: the Trust Folder pop-up shows.
    Untrusted,
    /// `folder.trust` is out.
    Trusting,
    Failed(String),
    /// The switch to another Codex or Antigravity login needs a yes first (Antigravity restarts):
    /// Start then says `button` and confirms.
    Confirm {
        message: String,
        button: String,
    },
    /// Started; the sheet closes.
    Started,
}

/// The sheet's state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NewSession {
    pub accounts: Vec<AccountChoice>,
    pub folders: Vec<String>,
    pub account: Option<usize>,
    pub folder: Option<usize>,
    pub stage: Stage,
    /// For "~" in the folder lines.
    pub home: String,
}

/// Every account to start on: Claude Code's (`targets`: the main login, each lane, each saved
/// account; with none known, the main login and the lanes sessions run in), then each Codex and
/// Antigravity account that can be used (`providers`), the one in use first.
pub fn accounts(
    targets: &[Target],
    agents: &[Agent],
    providers: &[Provider],
) -> Vec<AccountChoice> {
    let claude = |title: &str, detail: &str, pick: Pick| AccountChoice {
        title: title.to_owned(),
        detail: detail.to_owned(),
        section: Section::Claude,
        pick,
    };
    let mut list: Vec<AccountChoice> = targets
        .iter()
        .map(|t| match &t.place {
            Place::Main => claude(&t.title, "Main login", Pick::Claude(None)),
            Place::Lane(path) => claude(&t.title, "Lane", Pick::Claude(Some(path.clone()))),
            Place::Saved(account) => {
                claude(&t.title, "Opens a lane", Pick::ClaudeSaved(account.clone()))
            }
        })
        .collect();
    if list.is_empty() {
        list.push(claude("Claude Code", "Main login", Pick::Claude(None)));
        let lanes: BTreeSet<&str> = agents
            .iter()
            .filter_map(|a| a.config_dir.as_deref())
            .collect();
        list.extend(
            lanes
                .into_iter()
                .map(|dir| claude(last_part(dir), "Lane", Pick::Claude(Some(dir.to_owned())))),
        );
    }
    for (id, section) in [(CODEX, Section::Codex), (ANTIGRAVITY, Section::Antigravity)] {
        let Some(p) = providers.iter().find(|p| p.id == id && p.available) else {
            continue;
        };
        let active = p.active_id.as_deref();
        let mut usable: Vec<&ProviderAccount> = p
            .accounts
            .iter()
            .filter(|acc| Some(acc.id.as_str()) == active || (acc.switchable && !acc.needs_login))
            .collect();
        usable.sort_by_key(|acc| Some(acc.id.as_str()) != active);
        let tool = |title: &str, detail: String, account: &str, switch: bool| AccountChoice {
            title: title.to_owned(),
            detail,
            section,
            pick: Pick::Tool {
                provider: id.to_owned(),
                account: account.to_owned(),
                switch,
            },
        };
        for acc in &usable {
            let in_use = Some(acc.id.as_str()) == active;
            let detail = if in_use {
                "In use".to_owned()
            } else {
                format!("Switches {}", section.title())
            };
            list.push(tool(&acc.email, detail, &acc.id, !in_use));
        }
        // Signed in with a login it doesn't keep (a ChatGPT or Google account): that one.
        if usable.is_empty() {
            if let Some(title) = &p.unlisted_active_title {
                list.push(tool(title, "In use".to_owned(), "", false));
            }
        }
    }
    list
}

/// The folders to offer: the ones picked before (`chatProjectFolders`, newest first), then where
/// sessions run, newest first; each once, at most [`MAX_FOLDERS`].
pub fn folders(saved: &[String], agents: &[Agent]) -> Vec<String> {
    let mut by_age: Vec<&Agent> = agents.iter().filter(|a| !a.cwd.is_empty()).collect();
    by_age.sort_by_key(|a| std::cmp::Reverse(a.since_ms));
    let mut seen = BTreeSet::new();
    saved
        .iter()
        .map(String::as_str)
        .chain(by_age.into_iter().map(|a| a.cwd.as_str()))
        .filter(|f| seen.insert(f.to_owned()))
        .take(MAX_FOLDERS)
        .map(str::to_owned)
        .collect()
}

fn last_part(path: &str) -> &str {
    Path::new(path)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(path)
}

/// A new Codex or Antigravity row in `folder`, listed until its first message starts the session
/// (`codex-new:<n>`, `agy-new:<n>`). `None` for any other provider.
pub fn draft_session(provider: &str, folder: &str, now_ms: i64) -> Option<Session> {
    let (prefix, tool, host) = match provider {
        CODEX => (CODEX_DRAFT_PREFIX, Tool::Codex, "Codex"),
        ANTIGRAVITY => (AGY_DRAFT_PREFIX, Tool::Antigravity, "Antigravity"),
        _ => return None,
    };
    let project = last_part(folder).to_owned();
    let agent = Agent {
        id: format!("{prefix}{now_ms}"),
        name: format!("New {host} Session"),
        project,
        cwd: folder.to_owned(),
        group: AgentGroup::Completed,
        label: "New".to_owned(),
        detail: None,
        since_ms: now_ms,
        target: AgentTarget::Resume {
            session_id: String::new(),
            cwd: folder.to_owned(),
        },
        suggested_reply: None,
        transcript_path: None,
        host: host.to_owned(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
    };
    Some(Session {
        agent,
        tool,
        transcript: None,
    })
}

/// A folder's name in the list: its own, or with its parent when another folder shares it
/// (`ChatModel.folderTitle`).
pub fn folder_title(path: &str, among: &[String]) -> String {
    let name = last_part(path);
    let shared = among
        .iter()
        .any(|other| other != path && last_part(other) == name);
    match Path::new(path).parent().and_then(|p| p.file_name()) {
        Some(parent) if shared => format!("{}/{name}", parent.to_string_lossy()),
        _ => name.to_owned(),
    }
}

/// The folder's parent, with the home folder as "~".
pub fn parent_line(path: &str, home: &str) -> String {
    let parent = Path::new(path)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    match parent.strip_prefix(home) {
        Some(rest) if !home.is_empty() => format!("~{rest}"),
        _ => parent,
    }
}

impl NewSession {
    /// The first account and the newest folder picked (`pickDefaults`).
    pub fn new(accounts: Vec<AccountChoice>, folders: Vec<String>, home: &str) -> NewSession {
        NewSession {
            account: (!accounts.is_empty()).then_some(0),
            folder: (!folders.is_empty()).then_some(0),
            accounts,
            folders,
            stage: Stage::Choosing,
            home: home.to_owned(),
        }
    }

    pub fn pick_account(&mut self, i: usize) {
        if i < self.accounts.len() {
            self.account = Some(i);
            self.reset();
        }
    }

    pub fn pick_folder(&mut self, i: usize) {
        if i < self.folders.len() {
            self.folder = Some(i);
            self.reset();
        }
    }

    /// A folder chosen in the open panel goes to the top of the list.
    pub fn add_folder(&mut self, path: &str) {
        self.folders.retain(|f| f != path);
        self.folders.insert(0, path.to_owned());
        self.folders.truncate(MAX_FOLDERS);
        self.folder = Some(0);
        self.reset();
    }

    /// Another pick clears a refusal, as `.onChange` did.
    fn reset(&mut self) {
        if matches!(
            self.stage,
            Stage::Untrusted | Stage::Failed(_) | Stage::Confirm { .. }
        ) {
            self.stage = Stage::Choosing;
        }
    }

    fn chosen(&self) -> Option<(&AccountChoice, &str)> {
        Some((
            self.accounts.get(self.account?)?,
            self.folders.get(self.folder?)?,
        ))
    }

    pub fn busy(&self) -> bool {
        matches!(self.stage, Stage::Starting | Stage::Trusting)
    }

    /// Start: an account and a folder, nothing out, and acting on.
    pub fn start_control(&self, acting: Acting) -> Control {
        let gate = acting.control();
        if !gate.enabled {
            return gate;
        }
        let ready = self.chosen().is_some() && !self.busy();
        Control {
            enabled: ready,
            tooltip: None,
        }
    }

    /// `session.new` for a Claude Code choice in a folder it already has, marking it out.
    pub fn start(&mut self) -> Option<Request> {
        let (account, folder) = self.chosen()?;
        let Pick::Claude(dir) = &account.pick else {
            return None;
        };
        let request = Request::new_session(folder, dir.as_deref(), &[]);
        self.stage = Stage::Starting;
        Some(request)
    }

    /// What Start does for the choice, marking it out.
    pub fn plan(&mut self) -> Option<Plan> {
        let (account, folder) = self.chosen()?;
        let plan = match account.pick.clone() {
            Pick::Claude(_) => return self.start().map(Plan::Request),
            Pick::ClaudeSaved(account) => Plan::Lane { account },
            Pick::Tool {
                provider,
                account,
                switch,
            } => Plan::Draft {
                provider,
                account,
                switch,
                folder: folder.to_owned(),
            },
        };
        self.stage = Stage::Starting;
        Some(plan)
    }

    /// The lane made for a saved account: the choice now starts there (`session.new`).
    pub fn lane_made(&mut self, result: Result<String, String>) -> Option<Request> {
        match result {
            Ok(path) => {
                let i = self.account?;
                let choice = self.accounts.get_mut(i)?;
                choice.pick = Pick::Claude(Some(path));
                choice.detail = "Lane".to_owned();
                self.start()
            }
            Err(message) => {
                self.stage = Stage::Failed(message);
                None
            }
        }
    }

    /// The answer to switching Codex or Antigravity before a draft: done, or a yes to ask for.
    /// True when the draft can be made.
    pub fn switched(&mut self, result: Result<Outcome, String>) -> bool {
        match result {
            Ok(Outcome::Done { .. }) => {
                self.stage = Stage::Started;
                true
            }
            Ok(Outcome::NeedsConfirmation {
                message, button, ..
            }) => {
                self.stage = Stage::Confirm { message, button };
                false
            }
            Err(message) => {
                self.stage = Stage::Failed(message);
                false
            }
        }
    }

    /// Whether Start now confirms a switch.
    pub fn confirming(&self) -> bool {
        matches!(self.stage, Stage::Confirm { .. })
    }

    /// The daemon's answer to `session.new`.
    pub fn started(&mut self, result: Result<(), String>) {
        self.stage = match result {
            Ok(()) => Stage::Started,
            Err(message) if acting::is_untrusted(&message) => Stage::Untrusted,
            Err(message) => Stage::Failed(message),
        };
    }

    /// The pop-up's Trust Folder and Start: `folder.trust`, marking it out.
    pub fn trust(&mut self) -> Option<Request> {
        if self.stage != Stage::Untrusted {
            return None;
        }
        let (account, folder) = self.chosen()?;
        let Pick::Claude(dir) = &account.pick else {
            return None;
        };
        let request = Request::trust_folder(folder, dir.as_deref());
        self.stage = Stage::Trusting;
        Some(request)
    }

    /// The daemon's answer to `folder.trust`: trusted, it starts again (the caller sends the
    /// returned `session.new`).
    pub fn trusted(&mut self, result: Result<(), String>) -> Option<Request> {
        match result {
            Ok(()) => self.start(),
            Err(message) => {
                self.stage = Stage::Failed(message);
                None
            }
        }
    }

    /// The pop-up's Cancel.
    pub fn cancel_trust(&mut self) {
        if self.stage == Stage::Untrusted {
            self.stage = Stage::Choosing;
        }
    }

    /// The footer's line: what Start will do, or what went wrong.
    pub fn footer(&self) -> String {
        match &self.stage {
            Stage::Failed(message) | Stage::Confirm { message, .. } => message.clone(),
            Stage::Untrusted => "Claude Code hasn't trusted this folder yet.".to_owned(),
            Stage::Starting => "Starting…".to_owned(),
            Stage::Trusting => "Trusting the folder…".to_owned(),
            Stage::Choosing | Stage::Started => match self.chosen() {
                Some((account, folder)) => format!(
                    "{} ({}) in {}",
                    account.title,
                    account.detail,
                    folder_title(folder, &self.folders)
                ),
                None => "Pick an account and a folder.".to_owned(),
            },
        }
    }

    /// The folder the Trust Folder pop-up names, while it asks and while it trusts.
    pub fn untrusted_folder(&self) -> Option<&str> {
        matches!(self.stage, Stage::Untrusted | Stage::Trusting)
            .then(|| self.chosen().map(|(_, f)| f))
            .flatten()
    }
}

/// The Trust Folder pop-up's words.
pub const TRUST_TITLE: &str = "Trust This Folder?";
pub fn trust_text(folder: &str) -> String {
    format!(
        "Claude Code hasn't trusted {folder} yet. Trust Folder and Start accepts Claude Code's \
         “Do you trust this folder?” prompt for it, as running claude there would, then starts \
         the session."
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acting::OFF_MESSAGE;
    use notchling_engine::sessions::{AgentGroup, AgentTarget};
    use serde_json::json;

    fn agent(cwd: &str, since: i64, lane: Option<&str>) -> Agent {
        Agent {
            id: format!("job:{cwd}"),
            name: last_part(cwd).into(),
            project: last_part(cwd).into(),
            cwd: cwd.into(),
            group: AgentGroup::Working,
            label: "Working".into(),
            detail: None,
            since_ms: since,
            target: AgentTarget::Job("j".into()),
            suggested_reply: None,
            transcript_path: None,
            host: "Background".into(),
            headless: false,
            config_dir: lane.map(str::to_owned),
            command: None,
            resume_session_id: None,
        }
    }

    fn sheet() -> NewSession {
        let agents = [
            agent("/h/work/api", 10, None),
            agent("/h/work/site", 30, Some("/h/.claude-lanes/night")),
            agent("/h/old/api", 20, None),
        ];
        NewSession::new(
            accounts(&[], &agents, &[]),
            folders(&["/h/work/new".into()], &agents),
            "/h",
        )
    }

    #[test]
    fn accounts_are_the_main_login_then_each_lane() {
        let s = sheet();
        let titles: Vec<(&str, &str)> = s
            .accounts
            .iter()
            .map(|a| (a.title.as_str(), a.detail.as_str()))
            .collect();
        assert_eq!(titles, [("Claude Code", "Main login"), ("night", "Lane")]);
        assert_eq!(
            s.accounts[1].pick,
            Pick::Claude(Some("/h/.claude-lanes/night".into()))
        );
    }

    fn provider(id: &str, active: Option<&str>, accounts: &[(&str, bool)]) -> Provider {
        Provider {
            id: id.into(),
            title: id.into(),
            available: true,
            active_id: active.map(str::to_owned),
            accounts: accounts
                .iter()
                .map(|(email, switchable)| ProviderAccount {
                    id: format!("id-{email}"),
                    email: (*email).into(),
                    plan: None,
                    needs_login: false,
                    switchable: *switchable,
                })
                .collect(),
            ..Provider::default()
        }
    }

    fn every_tool() -> NewSession {
        let targets = [
            Target {
                title: "me@a.com".into(),
                place: Place::Main,
                account: Some("a".into()),
            },
            Target {
                title: "night@b.com".into(),
                place: Place::Lane("/h/.claude-lanes/night".into()),
                account: Some("b".into()),
            },
            Target {
                title: "spare@c.com".into(),
                place: Place::Saved("c".into()),
                account: Some("c".into()),
            },
        ];
        let providers = [
            provider(
                CODEX,
                Some("id-on@x.com"),
                &[
                    ("off@x.com", true),
                    ("on@x.com", true),
                    ("locked@x.com", false),
                ],
            ),
            provider(ANTIGRAVITY, Some("id-g@y.com"), &[("g@y.com", true)]),
            Provider {
                available: false,
                ..provider("claudeDesktop", None, &[("d@z.com", true)])
            },
        ];
        NewSession::new(
            accounts(&targets, &[], &providers),
            vec!["/h/w".into()],
            "/h",
        )
    }

    #[test]
    fn every_claude_code_account_then_codex_then_antigravity() {
        let s = every_tool();
        let rows: Vec<(Section, &str, &str)> = s
            .accounts
            .iter()
            .map(|a| (a.section, a.title.as_str(), a.detail.as_str()))
            .collect();
        assert_eq!(
            rows,
            [
                (Section::Claude, "me@a.com", "Main login"),
                (Section::Claude, "night@b.com", "Lane"),
                (Section::Claude, "spare@c.com", "Opens a lane"),
                (Section::Codex, "on@x.com", "In use"),
                (Section::Codex, "off@x.com", "Switches Codex"),
                (Section::Antigravity, "g@y.com", "In use"),
            ]
        );
    }

    #[test]
    fn start_plans_a_session_a_lane_first_or_a_draft() {
        let mut s = every_tool();
        assert!(matches!(s.plan(), Some(Plan::Request(r)) if r.op == "session.new"));
        s.stage = Stage::Choosing;
        s.pick_account(2);
        assert_eq!(
            s.plan(),
            Some(Plan::Lane {
                account: "c".into()
            })
        );
        let r = s.lane_made(Ok("/h/.claude-lanes/spare".into())).unwrap();
        assert_eq!(
            r.args,
            json!({"folder": "/h/w", "config_dir": "/h/.claude-lanes/spare"})
        );
        s.stage = Stage::Choosing;
        s.pick_account(4);
        assert_eq!(
            s.plan(),
            Some(Plan::Draft {
                provider: CODEX.into(),
                account: "id-off@x.com".into(),
                switch: true,
                folder: "/h/w".into(),
            })
        );
        assert!(s.busy());
    }

    #[test]
    fn a_switch_that_needs_a_yes_asks_in_the_footer() {
        let mut s = every_tool();
        s.pick_account(4);
        s.plan();
        let ask = Outcome::NeedsConfirmation {
            title: "Switch?".into(),
            message: "Antigravity restarts.".into(),
            button: "Switch and Start".into(),
        };
        assert!(!s.switched(Ok(ask)));
        assert!(s.confirming());
        assert_eq!(s.footer(), "Antigravity restarts.");
        let done = Outcome::Done {
            state: "switched".into(),
            sign_in_url: None,
        };
        assert!(s.switched(Ok(done)));
        assert_eq!(s.stage, Stage::Started);
        assert!(!s.switched(Err("no".into())));
        assert_eq!(s.stage, Stage::Failed("no".into()));
    }

    #[test]
    fn a_draft_row_is_codex_or_antigravity_in_the_folder() {
        let codex = draft_session(CODEX, "/h/w/api", 7).unwrap();
        assert_eq!(codex.agent.id, "codex-new:7");
        assert_eq!(
            (codex.tool, codex.agent.cwd.as_str()),
            (Tool::Codex, "/h/w/api")
        );
        assert_eq!(codex.agent.project, "api");
        let agy = draft_session(ANTIGRAVITY, "/h/w", 8).unwrap();
        assert_eq!(
            (agy.agent.id.as_str(), agy.tool),
            ("agy-new:8", Tool::Antigravity)
        );
        assert!(crate::chat::used::is_draft(&agy.agent.id));
        assert_eq!(draft_session("claudeDesktop", "/h/w", 9), None);
    }

    #[test]
    fn folders_are_the_saved_then_the_newest_each_once() {
        let s = sheet();
        assert_eq!(
            s.folders,
            ["/h/work/new", "/h/work/site", "/h/old/api", "/h/work/api"]
        );
        assert_eq!(folder_title("/h/work/api", &s.folders), "work/api");
        assert_eq!(folder_title("/h/work/site", &s.folders), "site");
        assert_eq!(parent_line("/h/work/site", "/h"), "~/work");
        assert_eq!(parent_line("/srv/x", "/h"), "/srv");
        let many: Vec<String> = (0..40).map(|i| format!("/f/{i}")).collect();
        assert_eq!(folders(&many, &[]).len(), MAX_FOLDERS);
    }

    #[test]
    fn start_waits_for_acting_and_a_choice() {
        let mut s = sheet();
        assert_eq!(s.start_control(Acting::Off).tooltip, Some(OFF_MESSAGE));
        assert!(!s.start_control(Acting::Detached).enabled);
        assert!(s.start_control(Acting::On).enabled);
        let empty = NewSession::new(vec![], vec![], "/h");
        assert!(!empty.start_control(Acting::On).enabled);
        s.pick_account(1);
        let r = s.start().unwrap();
        assert_eq!(
            r.args,
            json!({"folder": "/h/work/new", "config_dir": "/h/.claude-lanes/night"})
        );
        assert!(s.busy() && !s.start_control(Acting::On).enabled);
        s.started(Ok(()));
        assert_eq!(s.stage, Stage::Started);
    }

    #[test]
    fn an_untrusted_folder_opens_the_pop_up_and_trusting_starts_again() {
        let mut s = sheet();
        s.start();
        s.started(Err("Workspace not trusted".into()));
        assert_eq!(s.stage, Stage::Untrusted);
        assert_eq!(s.untrusted_folder(), Some("/h/work/new"));
        assert!(s.footer().contains("hasn't trusted"));
        let trust = s.trust().unwrap();
        assert_eq!(trust.op, "folder.trust");
        assert_eq!(trust.args, json!({"folder": "/h/work/new"}));
        assert_eq!(s.trust(), None, "only from the pop-up");
        assert_eq!(
            s.untrusted_folder(),
            Some("/h/work/new"),
            "named while it trusts"
        );
        let again = s.trusted(Ok(())).unwrap();
        assert_eq!(again.op, "session.new");
        assert_eq!(s.stage, Stage::Starting);
    }

    #[test]
    fn a_lane_trusts_the_folder_in_its_own_config_folder() {
        let mut s = sheet();
        s.pick_account(1);
        s.start();
        s.started(Err("Workspace not trusted".into()));
        let trust = s.trust().unwrap();
        assert_eq!(
            trust.args,
            json!({"folder": "/h/work/new", "config_dir": "/h/.claude-lanes/night"})
        );
    }

    #[test]
    fn cancel_or_another_pick_closes_the_pop_up_and_failures_say_why() {
        let mut s = sheet();
        s.start();
        s.started(Err("Workspace not trusted".into()));
        s.cancel_trust();
        assert_eq!(s.stage, Stage::Choosing);
        s.start();
        s.started(Err("Workspace not trusted".into()));
        s.pick_folder(2);
        assert_eq!(s.stage, Stage::Choosing);
        s.start();
        s.started(Err(OFF_MESSAGE.into()));
        assert_eq!(s.footer(), OFF_MESSAGE);
        s.stage = Stage::Untrusted;
        s.trust();
        assert_eq!(s.trusted(Err("no tty".into())), None);
        assert_eq!(s.stage, Stage::Failed("no tty".into()));
        s.add_folder("/h/picked");
        assert_eq!((s.folder, s.folders[0].as_str()), (Some(0), "/h/picked"));
        assert_eq!(s.footer(), "Claude Code (Main login) in picked");
    }
}
