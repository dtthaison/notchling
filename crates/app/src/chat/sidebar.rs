//! The sidebar's sections and rows, from the sessions the engine lists.

use notchling_engine::search::SessionTool;
use notchling_engine::sessions::AgentGroup;
use notchling_kit::Status;

use super::data::{Session, Tool};
use super::format::age;
use crate::accounts::model::{ANTIGRAVITY, CODEX};
use crate::accounts::store::Snapshot;
use crate::search::wire::SessionRef;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SidebarRow {
    pub id: String,
    pub name: String,
    /// The status line under the name: what it needs, or its label.
    pub status_line: String,
    pub age: String,
    pub status: Status,
    pub tool: Tool,
    /// The Claude Code lane folder the session was found in; `None` on the main login.
    pub config_dir: Option<String>,
}

/// The account a row names under the session's name.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RowAccount {
    /// The email.
    pub text: String,
    /// The session runs in a lane, not on the main login (the icon says which).
    pub lane: bool,
}

/// The account a session runs on, for its row (the Swift `ChatModel.account(for:)`):
/// - Claude Code: the lane whose folder the session was found in, else the main login's account.
///   A folder no lane names is not guessed at, and nobody signed in on the main login says nothing.
/// - Codex and Antigravity: the provider's active account, which the Swift app leaves blank (they
///   have one login each, so it is the account any session of theirs runs on).
///
/// Only what the accounts store already holds, so it is `None` until the daemon has answered.
pub fn row_account(tool: Tool, config_dir: Option<&str>, s: &Snapshot) -> Option<RowAccount> {
    let named = |email: &str, lane: bool| {
        (!email.is_empty()).then(|| RowAccount {
            text: email.to_owned(),
            lane,
        })
    };
    match tool {
        Tool::Claude => match config_dir {
            Some(dir) => {
                let lane = s.lanes.iter().find(|l| l.path == dir)?;
                named(lane.email.as_deref().unwrap_or(&lane.name), true)
            }
            None => {
                let active = s.claude.active.as_ref()?;
                let account = s.claude.accounts.iter().find(|a| &a.id == active)?;
                named(&account.email, false)
            }
        },
        Tool::Codex | Tool::Antigravity => {
            let id = if tool == Tool::Codex {
                CODEX
            } else {
                ANTIGRAVITY
            };
            let provider = s.providers.providers.iter().find(|p| p.id == id)?;
            let active = provider.active_id.as_ref()?;
            let account = provider.accounts.iter().find(|a| &a.id == active)?;
            named(&account.email, false)
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    pub title: &'static str,
    pub rows: Vec<SidebarRow>,
}

/// The kit's status for a group.
pub fn status_of(group: AgentGroup) -> Status {
    match group {
        AgentGroup::NeedsInput => Status::Needs,
        AgentGroup::Working => Status::Working,
        AgentGroup::Completed => Status::Done,
    }
}

fn section_title(group: AgentGroup) -> &'static str {
    match group {
        AgentGroup::NeedsInput => "Needs input",
        AgentGroup::Working => "Working",
        AgentGroup::Completed => "Done",
    }
}

fn row(session: &Session, now_ms: i64) -> SidebarRow {
    let agent = &session.agent;
    let status_line = match &agent.detail {
        Some(detail) if !detail.is_empty() => detail.clone(),
        _ => agent.label.clone(),
    };
    SidebarRow {
        id: agent.id.clone(),
        name: agent.name.clone(),
        status_line,
        age: age(now_ms, agent.since_ms),
        status: status_of(agent.group),
        tool: session.tool,
        config_dir: agent.config_dir.clone(),
    }
}

/// The sidebar row a search hit opens (`SessionRef` from the search panel): a Codex row is keyed
/// `codex:<session id>`, an Antigravity row `agy:<session id>`; a Claude Code row by Claude Code's
/// own session id, else by its transcript's file stem.
pub fn row_for(sessions: &[Session], target: &SessionRef) -> Option<String> {
    let id = target.session_id.as_str();
    let found = match target.tool {
        SessionTool::Codex => sessions
            .iter()
            .find(|s| s.tool == Tool::Codex && s.agent.id == format!("codex:{id}")),
        SessionTool::Antigravity => sessions
            .iter()
            .find(|s| s.tool == Tool::Antigravity && s.agent.id == format!("agy:{id}")),
        SessionTool::Claude => sessions
            .iter()
            .filter(|s| s.tool == Tool::Claude)
            .find(|s| {
                s.agent.session_id().as_deref() == Some(id)
                    || s.transcript
                        .as_deref()
                        .and_then(|p| p.file_stem())
                        .and_then(|stem| stem.to_str())
                        == Some(id)
            }),
    };
    found.map(|s| s.agent.id.clone())
}

/// The sidebar row of the background job New Session just started (`session.new`'s `jobId`, the
/// short id `claude --bg` printed), once the list has it.
pub fn row_for_job(sessions: &[Session], job: &str) -> Option<String> {
    sessions
        .iter()
        .filter(|s| s.tool == Tool::Claude)
        .find(|s| {
            matches!(&s.agent.target, notchling_engine::sessions::AgentTarget::Job(j) if j == job)
        })
        .map(|s| s.agent.id.clone())
}

/// A job New Session started, waited for until the sidebar lists it. A new job takes a few loads
/// to show (its host writes its state after it starts), so it waits longer than a search hit,
/// then gives up: a row found much later must not take the selection from whatever is open.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingJob {
    pub job: String,
    expires_ms: i64,
}

impl PendingJob {
    pub const WAIT_MS: i64 = 30_000;

    pub fn new(job: &str, now_ms: i64) -> PendingJob {
        PendingJob {
            job: job.to_owned(),
            expires_ms: now_ms.saturating_add(Self::WAIT_MS),
        }
    }

    /// A load finished, listing `sessions`.
    pub fn after_load(&self, sessions: &[Session], now_ms: i64) -> Pending {
        match row_for_job(sessions, &self.job) {
            Some(id) => Pending::Found(id),
            None if now_ms >= self.expires_ms => Pending::GaveUp,
            None => Pending::Waiting,
        }
    }
}

/// A conversation the search panel asked Chat to show that the sidebar doesn't list yet. It waits
/// for the loads that follow, and gives up after a couple of them or a few seconds: a session
/// that never turns up (it was deleted, or is older than the sidebar reaches) must not be
/// selected whenever it finally does, long after anyone asked.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingSession {
    pub target: SessionRef,
    loads_left: u8,
    expires_ms: i64,
}

/// What a load made of a [`PendingSession`].
#[derive(Debug, PartialEq, Eq)]
pub enum Pending {
    /// The row to select.
    Found(String),
    /// Not listed yet; keep waiting.
    Waiting,
    /// Not listed, and no longer worth waiting for.
    GaveUp,
}

impl PendingSession {
    /// How many loads it waits through. The first may have begun before the ask, so it is two.
    pub const LOADS: u8 = 2;
    /// And how long, whatever the loads.
    pub const WAIT_MS: i64 = 5_000;

    pub fn new(target: SessionRef, now_ms: i64) -> PendingSession {
        PendingSession {
            target,
            loads_left: Self::LOADS,
            expires_ms: now_ms.saturating_add(Self::WAIT_MS),
        }
    }

    /// A load finished, listing `sessions`.
    pub fn after_load(&mut self, sessions: &[Session], now_ms: i64) -> Pending {
        if let Some(id) = row_for(sessions, &self.target) {
            return Pending::Found(id);
        }
        self.loads_left = self.loads_left.saturating_sub(1);
        if self.loads_left == 0 || now_ms >= self.expires_ms {
            Pending::GaveUp
        } else {
            Pending::Waiting
        }
    }
}

/// Needs input, Working, Done, in that order; a group with no sessions has no section. The rows
/// keep the engine's order (newest first within a group). A Claude Code session that has exited
/// (its process or job is gone) isn't listed: the maintainer asked on 2026-11-10 that Done hold
/// only sessions still there; search still finds an exited one.
pub fn sections(sessions: &[Session], now_ms: i64) -> Vec<Section> {
    [
        AgentGroup::NeedsInput,
        AgentGroup::Working,
        AgentGroup::Completed,
    ]
    .into_iter()
    .filter_map(|group| {
        let rows: Vec<SidebarRow> = sessions
            .iter()
            .filter(|s| s.agent.group == group && !exited(s))
            .map(|s| row(s, now_ms))
            .collect();
        (!rows.is_empty()).then(|| Section {
            title: section_title(group),
            rows,
        })
    })
    .collect()
}

/// What the Sidebar draws: everything its draw function reads, so a part can draw only when it
/// changes. Equal for two states that differ only in what no row shows (a draft, a menu).
#[derive(Clone, Debug, PartialEq)]
pub struct SidebarModel {
    pub sections: Vec<Section>,
    /// Each row's account, in row order across the sections.
    pub accounts: Vec<Option<RowAccount>>,
    pub selected: Option<String>,
    pub daemon_note: Option<&'static str>,
    pub unread: usize,
}

/// The Sidebar's model: its sections (ages and statuses as strings, so a tick that changes none
/// changes nothing), each row's account from the accounts store, the selection and the footer's
/// note about the daemon.
pub fn sidebar_model(
    sessions: &[Session],
    now_ms: i64,
    selected: Option<&str>,
    snapshot: &Snapshot,
    daemon_note: Option<&'static str>,
) -> SidebarModel {
    let sections = sections(sessions, now_ms);
    let accounts: Vec<Option<RowAccount>> = sections
        .iter()
        .flat_map(|s| &s.rows)
        .map(|r| row_account(r.tool, r.config_dir.as_deref(), snapshot))
        .collect();
    SidebarModel {
        sections,
        accounts,
        selected: selected.map(str::to_owned),
        daemon_note,
        unread: 0,
    }
}

/// The session id a row's Copy Session ID puts on the clipboard: the tool's own id (Claude Code's
/// session UUID is the row id; Codex and Antigravity rows carry a `codex:` or `agy:` prefix). A
/// draft that hasn't started has none.
pub fn copyable_id(row_id: &str) -> Option<&str> {
    if row_id.starts_with("codex-new:") || row_id.starts_with("agy-new:") {
        return None;
    }
    let id = row_id
        .strip_prefix("codex:")
        .or_else(|| row_id.strip_prefix("agy:"))
        .unwrap_or(row_id);
    (!id.is_empty()).then_some(id)
}

fn exited(s: &Session) -> bool {
    s.tool == Tool::Claude && s.agent.label == "Exited"
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use notchling_engine::sessions::{Agent, AgentTarget};

    pub(crate) fn session(
        id: &str,
        group: AgentGroup,
        detail: Option<&str>,
        tool: Tool,
    ) -> Session {
        Session {
            agent: Agent {
                id: id.into(),
                name: format!("name {id}"),
                project: "p".into(),
                cwd: "/w/p".into(),
                group,
                label: match group {
                    AgentGroup::NeedsInput => "Needs input".into(),
                    AgentGroup::Working => "Working".into(),
                    AgentGroup::Completed => "Idle".into(),
                },
                detail: detail.map(String::from),
                since_ms: 1_000_000_000_000 - 120_000,
                target: AgentTarget::Job(id.into()),
                suggested_reply: None,
                transcript_path: None,
                host: "Background".into(),
                headless: false,
                config_dir: None,
                command: None,
                resume_session_id: None,
            },
            tool,
            transcript: None,
        }
    }

    #[test]
    fn copy_session_id_gives_the_tools_own_id() {
        let claude = "be50d36a-4348-4ba6-ab58-96959399a893";
        assert_eq!(copyable_id(claude), Some(claude));
        assert_eq!(copyable_id("codex:019a-77"), Some("019a-77"));
        assert_eq!(copyable_id("agy:c0ffee"), Some("c0ffee"));
        assert_eq!(copyable_id("codex-new:1234"), None);
        assert_eq!(copyable_id("agy-new:1234"), None);
        assert_eq!(copyable_id("codex:"), None);
    }

    #[test]
    fn a_claude_session_that_exited_is_not_listed() {
        let mut gone = session("gone", AgentGroup::Completed, None, Tool::Claude);
        gone.agent.label = "Exited".into();
        let sessions = vec![
            gone,
            session("idle", AgentGroup::Completed, None, Tool::Claude),
        ];
        let sections = sections(&sessions, 1_000_000_000_000);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].rows.len(), 1);
        assert_eq!(sections[0].rows[0].status_line, "Idle");
        // Only exited ones: no Done section at all.
        let mut only = session("x", AgentGroup::Completed, None, Tool::Claude);
        only.agent.label = "Exited".into();
        assert!(super::sections(&[only], 1_000_000_000_000).is_empty());
    }

    #[test]
    fn sections_follow_group_order_and_skip_empty_groups() {
        let sessions = vec![
            session("a", AgentGroup::Completed, None, Tool::Claude),
            session(
                "b",
                AgentGroup::NeedsInput,
                Some("answer: Which?"),
                Tool::Claude,
            ),
            session("c", AgentGroup::Completed, None, Tool::Codex),
        ];
        let sections = sections(&sessions, 1_000_000_000_000);
        assert_eq!(
            sections.iter().map(|s| s.title).collect::<Vec<_>>(),
            ["Needs input", "Done"]
        );
        assert_eq!(sections[0].rows[0].status_line, "answer: Which?");
        assert_eq!(sections[0].rows[0].status, Status::Needs);
        assert_eq!(sections[1].rows[0].status_line, "Idle");
        assert_eq!(sections[1].rows[0].age, "2m");
        assert_eq!(sections[1].rows[1].tool, Tool::Codex);
    }

    fn accounts() -> Snapshot {
        use crate::accounts::wire::{
            ClaudeAccount, ClaudeList, Lane, Provider, ProviderAccount, ProviderList,
        };
        let provider = |id: &str, email: &str| Provider {
            id: id.into(),
            active_id: Some("x1".into()),
            accounts: vec![ProviderAccount {
                id: "x1".into(),
                email: email.into(),
                ..ProviderAccount::default()
            }],
            ..Provider::default()
        };
        Snapshot {
            claude: ClaudeList {
                on: true,
                active: Some("a1".into()),
                accounts: vec![ClaudeAccount {
                    id: "a1".into(),
                    email: "me@example.com".into(),
                    ..ClaudeAccount::default()
                }],
            },
            lanes: vec![
                Lane {
                    name: "night".into(),
                    path: "/lanes/night".into(),
                    account_id: Some("a2".into()),
                    email: Some("ops@example.com".into()),
                },
                Lane {
                    name: "bare".into(),
                    path: "/lanes/bare".into(),
                    account_id: None,
                    email: None,
                },
            ],
            providers: ProviderList {
                accounts_on: true,
                providers: vec![
                    provider("codex", "cx@example.com"),
                    provider("antigravity", "ag@example.com"),
                ],
                syncing: false,
            },
            ..Snapshot::default()
        }
    }

    fn account(tool: Tool, dir: Option<&str>, s: &Snapshot) -> Option<(String, bool)> {
        row_account(tool, dir, s).map(|a| (a.text, a.lane))
    }

    #[test]
    fn a_claude_row_names_its_lane_or_the_main_login() {
        let s = accounts();
        assert_eq!(
            account(Tool::Claude, None, &s),
            Some(("me@example.com".into(), false))
        );
        assert_eq!(
            account(Tool::Claude, Some("/lanes/night"), &s),
            Some(("ops@example.com".into(), true))
        );
        // A lane that has no email yet is told by its name.
        assert_eq!(
            account(Tool::Claude, Some("/lanes/bare"), &s),
            Some(("bare".into(), true))
        );
    }

    #[test]
    fn a_claude_row_in_a_folder_no_lane_names_has_no_account() {
        assert_eq!(account(Tool::Claude, Some("/elsewhere"), &accounts()), None);
    }

    #[test]
    fn nobody_signed_in_or_no_daemon_says_nothing() {
        assert_eq!(account(Tool::Claude, None, &Snapshot::default()), None);
        assert_eq!(account(Tool::Codex, None, &Snapshot::default()), None);
        let mut s = accounts();
        s.claude.active = None;
        assert_eq!(account(Tool::Claude, None, &s), None);
        s.claude.active = Some("gone".into());
        assert_eq!(account(Tool::Claude, None, &s), None);
    }

    #[test]
    fn codex_and_antigravity_rows_name_their_active_account() {
        let s = accounts();
        assert_eq!(
            account(Tool::Codex, None, &s),
            Some(("cx@example.com".into(), false))
        );
        assert_eq!(
            account(Tool::Antigravity, None, &s),
            Some(("ag@example.com".into(), false))
        );
        let mut s = s;
        s.providers.providers[0].active_id = None;
        assert_eq!(account(Tool::Codex, None, &s), None);
        s.providers.providers[1].accounts[0].email.clear();
        assert_eq!(account(Tool::Antigravity, None, &s), None);
    }

    #[test]
    fn a_row_keeps_its_lane_folder() {
        let mut lane = session("l", AgentGroup::Working, None, Tool::Claude);
        lane.agent.config_dir = Some("/lanes/night".into());
        let sections = sections(&[lane], 0);
        assert_eq!(
            sections[0].rows[0].config_dir.as_deref(),
            Some("/lanes/night")
        );
    }

    #[test]
    fn no_sessions_no_sections() {
        assert!(sections(&[], 0).is_empty());
    }

    fn target(tool: SessionTool, id: &str) -> SessionRef {
        SessionRef {
            tool,
            session_id: id.into(),
        }
    }

    /// New Session closed and left the old session open: the started job was never selected.
    #[test]
    fn a_started_job_is_selected_once_the_sidebar_lists_it() {
        let pending = PendingJob::new("6ce4c19c", 1_000);
        let other = session("6220cc2b", AgentGroup::Completed, None, Tool::Claude);
        assert_eq!(
            pending.after_load(std::slice::from_ref(&other), 2_000),
            Pending::Waiting
        );
        let mut started = session("6ce4c19c", AgentGroup::Completed, None, Tool::Claude);
        started.agent.id = "be50d36a-4348-4ba6-ab58-96959399a893".into();
        assert_eq!(
            pending.after_load(&[other.clone(), started], 3_000),
            Pending::Found("be50d36a-4348-4ba6-ab58-96959399a893".into())
        );
        // A Codex row with the same text is not it, and a job that never shows is let go.
        let codex = session("6ce4c19c", AgentGroup::Completed, None, Tool::Codex);
        assert_eq!(pending.after_load(&[codex], 2_000), Pending::Waiting);
        assert_eq!(
            pending.after_load(&[other], 1_000 + PendingJob::WAIT_MS),
            Pending::GaveUp
        );
    }

    #[test]
    fn a_search_hit_finds_its_sidebar_row() {
        let mut by_transcript = session("job-1", AgentGroup::Working, None, Tool::Claude);
        by_transcript.transcript = Some("/claude/projects/-w-p/3f1c2a9e.jsonl".into());
        let mut resumed = session("job-2", AgentGroup::Completed, None, Tool::Claude);
        resumed.agent.target = AgentTarget::Resume {
            session_id: "7c1e".into(),
            cwd: "/w/p".into(),
        };
        let codex = session("codex:0199a1b2", AgentGroup::Completed, None, Tool::Codex);
        let agy = session(
            "agy:d674-13c2",
            AgentGroup::Completed,
            None,
            Tool::Antigravity,
        );
        let sessions = vec![by_transcript, resumed, codex, agy];
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Claude, "3f1c2a9e")).as_deref(),
            Some("job-1"),
            "the transcript's stem"
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Claude, "7c1e")).as_deref(),
            Some("job-2"),
            "a resume target's id"
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Codex, "0199a1b2")).as_deref(),
            Some("codex:0199a1b2")
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Codex, "3f1c2a9e")),
            None
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Claude, "nope")),
            None
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Antigravity, "d674-13c2")).as_deref(),
            Some("agy:d674-13c2")
        );
        assert_eq!(
            row_for(&sessions, &target(SessionTool::Antigravity, "x")),
            None
        );
    }

    fn codex_target(id: &str) -> SessionRef {
        SessionRef {
            tool: SessionTool::Codex,
            session_id: id.into(),
        }
    }

    #[test]
    fn a_session_asked_for_is_selected_when_a_load_lists_it() {
        let mut pending = PendingSession::new(codex_target("x"), 1_000);
        let rows = vec![session("codex:x", AgentGroup::Completed, None, Tool::Codex)];
        assert_eq!(
            pending.after_load(&rows, 1_100),
            Pending::Found("codex:x".into())
        );
    }

    #[test]
    fn a_session_that_never_turns_up_is_given_up_on_after_a_couple_of_loads() {
        let mut pending = PendingSession::new(codex_target("x"), 1_000);
        let other = vec![session("codex:y", AgentGroup::Completed, None, Tool::Codex)];
        // The first load may have started before the ask; the second is the next one.
        assert_eq!(pending.after_load(&other, 1_050), Pending::Waiting);
        assert_eq!(pending.after_load(&other, 1_200), Pending::GaveUp);
    }

    #[test]
    fn a_session_is_given_up_on_after_a_few_seconds_whatever_the_loads() {
        let mut pending = PendingSession::new(codex_target("x"), 1_000);
        assert_eq!(
            pending.after_load(&[], 1_000 + PendingSession::WAIT_MS),
            Pending::GaveUp
        );
        // It found nothing, so a later load listing it does not matter: the caller dropped it.
    }

    fn model(sessions: &[Session], selected: Option<&str>, snapshot: &Snapshot) -> SidebarModel {
        sidebar_model(sessions, 1_000_000_000_000, selected, snapshot, None)
    }

    fn two() -> Vec<Session> {
        vec![
            session("a", AgentGroup::Working, None, Tool::Claude),
            session("b", AgentGroup::Completed, None, Tool::Codex),
        ]
    }

    #[test]
    fn the_model_is_equal_when_nothing_shown_changed() {
        // The model takes only what the Sidebar draws: the same inputs, whatever else the window
        // holds (a draft, an open menu), give an equal model, and a later clock that rounds to the
        // same age too.
        let (sessions, snapshot) = (two(), accounts());
        let before = model(&sessions, Some("a"), &snapshot);
        assert_eq!(before, model(&sessions, Some("a"), &snapshot));
        let later = sidebar_model(
            &sessions,
            1_000_000_000_000 + 5_000,
            Some("a"),
            &snapshot,
            None,
        );
        assert_eq!(before, later);
    }

    #[test]
    fn a_new_age_or_status_changes_the_model() {
        let (mut sessions, snapshot) = (two(), accounts());
        let before = model(&sessions, None, &snapshot);
        let older = sidebar_model(
            &sessions,
            1_000_000_000_000 + 120_000,
            None,
            &snapshot,
            None,
        );
        assert_ne!(before, older, "the age 2m became 4m");
        sessions[0].agent.group = AgentGroup::NeedsInput;
        assert_ne!(before, model(&sessions, None, &snapshot));
        let mut with_note = model(&sessions, None, &snapshot);
        assert_eq!(with_note.daemon_note, None);
        with_note.daemon_note = Some("Daemon not running");
        assert_ne!(with_note, model(&sessions, None, &snapshot));
    }

    #[test]
    fn a_changed_row_account_changes_the_model() {
        let sessions = vec![session("a", AgentGroup::Working, None, Tool::Claude)];
        let mut snapshot = accounts();
        let before = model(&sessions, None, &snapshot);
        assert_eq!(
            before.accounts,
            [Some(RowAccount {
                text: "me@example.com".into(),
                lane: false
            })]
        );
        snapshot.claude.accounts[0].email = "other@example.com".into();
        assert_ne!(before, model(&sessions, None, &snapshot));
        snapshot.claude.active = None;
        assert_eq!(model(&sessions, None, &snapshot).accounts, [None]);
    }

    #[test]
    fn selecting_another_row_changes_the_model() {
        let (sessions, snapshot) = (two(), accounts());
        assert_ne!(
            model(&sessions, Some("a"), &snapshot),
            model(&sessions, Some("b"), &snapshot)
        );
        assert_ne!(
            model(&sessions, None, &snapshot),
            model(&sessions, Some("a"), &snapshot)
        );
    }
}
