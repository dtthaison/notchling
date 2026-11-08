//! The New Session sheet, pure (part U8, `NewSessionSheet.swift`): an account column (Claude Code
//! accounts only for now: the main login and each lane sessions run in), a folder column, then
//! Start, which asks the daemon for `session.new`. A folder Claude Code hasn't trusted is refused;
//! the Trust Folder pop-up then offers `folder.trust` and starts again.

use std::collections::BTreeSet;
use std::path::Path;

use notchling_engine::sessions::Agent;

use crate::acting::{self, Acting, Control, Request};

/// The most folders the column lists (`chatProjectFolders` keeps 30).
pub const MAX_FOLDERS: usize = 30;

/// One account to start on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AccountChoice {
    pub title: String,
    pub detail: String,
    /// The lane's config folder; `None` is the main login.
    pub config_dir: Option<String>,
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

/// The main login, then each lane a session runs in, by name.
pub fn accounts(agents: &[Agent]) -> Vec<AccountChoice> {
    let lanes: BTreeSet<&str> = agents
        .iter()
        .filter_map(|a| a.config_dir.as_deref())
        .collect();
    let mut list = vec![AccountChoice {
        title: "Claude Code".to_owned(),
        detail: "Main login".to_owned(),
        config_dir: None,
    }];
    list.extend(lanes.into_iter().map(|dir| AccountChoice {
        title: last_part(dir).to_owned(),
        detail: "Lane".to_owned(),
        config_dir: Some(dir.to_owned()),
    }));
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
        if matches!(self.stage, Stage::Untrusted | Stage::Failed(_)) {
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

    /// `session.new` for the choice, marking it out.
    pub fn start(&mut self) -> Option<Request> {
        let (account, folder) = self.chosen()?;
        let request = Request::new_session(folder, account.config_dir.as_deref(), &[]);
        self.stage = Stage::Starting;
        Some(request)
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
        let (_, folder) = self.chosen()?;
        let request = Request::trust_folder(folder);
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
            Stage::Failed(message) => message.clone(),
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

    /// The folder the Trust Folder pop-up names.
    pub fn untrusted_folder(&self) -> Option<&str> {
        (self.stage == Stage::Untrusted)
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
            accounts(&agents),
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
            s.accounts[1].config_dir.as_deref(),
            Some("/h/.claude-lanes/night")
        );
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
        let again = s.trusted(Ok(())).unwrap();
        assert_eq!(again.op, "session.new");
        assert_eq!(s.stage, Stage::Starting);
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
