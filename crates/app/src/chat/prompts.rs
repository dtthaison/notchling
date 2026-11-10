//! The question or permission a Claude Code session waits on, as Chat shows it above the reply
//! box (pure). The same sources as the notch (`NotchModel::card`), best first: a prompt the hook
//! holds (the daemon's `prompts`, else the Swift hook's `requests/` files), then the question in
//! the transcript's tail, then the summary in a background job's state ("answer: Which target?
//! (A · B)"). A permission wins over a question, as in the notch.
//!
//! A permission read from the job's state summary ("approve Bash: ls -la") has no hook prompt to
//! decide; its answer goes to the job's terminal instead ([`permission_request`]): the daemon reads
//! the dialog off the screen and types the option's digit only when the dialog asks about the tool
//! and command the card showed.
//!
//! A background job writes its question into the transcript only once it's answered, and the
//! state's summary lacks later questions and breaks on brackets ("Side by side (Recommended)"). So
//! while a job waits on an answer Chat asks the daemon to read the question off its screen
//! (`session.question`, once per summary: [`screen_read_key`]), and that read wins over the summary.

use std::collections::{HashMap, HashSet};

use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::snapshot::PendingQuestion;
use serde_json::Value;

use crate::acting::{Decision, Request};
use crate::chat::data::Tool;
use crate::notch::model::question_from_needs;
use crate::notch::requests::{HookRequest, RequestKind};

/// What a session waits on.
#[derive(Clone, Debug, PartialEq)]
pub enum Prompt {
    Question(PendingQuestion),
    Permission(Box<HookRequest>),
}

impl Prompt {
    /// What an answer is recorded by (`NotchModel::answer_key`): the hook prompt's id, else the
    /// question's tool use id. Taken when the answer is sent, since the prompt can change before
    /// the reply comes back.
    pub fn key(&self) -> String {
        match self {
            Prompt::Question(q) => question_key(q),
            Prompt::Permission(r) => r.id.clone(),
        }
    }

    /// The part's name, as `.part_at` and the ? button show it.
    pub fn part(&self) -> &'static str {
        match self {
            Prompt::Question(_) => "Question Card",
            Prompt::Permission(_) => "Permission Card",
        }
    }
}

/// The id prefix of a permission read from a job's state summary.
pub const NEEDS_PREFIX: &str = "needs:";

/// What a Permission card's button sends: a hook-held prompt is decided by its own id; one read
/// from a job's state is answered in the job's terminal, by the row and with what the card showed,
/// so the daemon types nothing into a dialog that asks about something else.
pub fn permission_request(r: &HookRequest, decision: Decision) -> Request {
    if r.id.starts_with(NEEDS_PREFIX) {
        Request::permission_on_screen(&r.session_id, decision, &r.tool_name, &r.summary)
    } else {
        Request::permission(&r.id, decision)
    }
}

/// A background job waiting on a permission has no prompt in the daemon, only a summary in its
/// state: "approve Bash: ls -la". The tool and what it would run, as a card shows them. Always
/// allow is offered: the daemon refuses it, with nothing typed, when the dialog has no "don't ask
/// again" option.
pub fn permission_from_needs(agent: &Agent) -> Option<HookRequest> {
    if agent.group != AgentGroup::NeedsInput || !matches!(agent.target, AgentTarget::Job(_)) {
        return None;
    }
    let needs = agent.detail.as_deref()?;
    let body = needs.strip_prefix("approve ")?;
    let (tool, summary) = match body.split_once(": ") {
        Some((tool, summary)) => (tool.trim(), summary.trim()),
        None => (body.trim(), ""),
    };
    if tool.is_empty() {
        return None;
    }
    Some(HookRequest {
        id: format!("{NEEDS_PREFIX}{}:{needs}", agent.id),
        session_id: agent.id.clone(),
        created: 0.0,
        kind: RequestKind::Permission,
        tool_name: tool.to_owned(),
        summary: summary.to_owned(),
        can_always_allow: true,
        agy: None,
    })
}

/// A question's answer key: its hook prompt's id, else its tool use id.
pub fn question_key(q: &PendingQuestion) -> String {
    q.tool_use_id
        .strip_prefix("hook:")
        .unwrap_or(&q.tool_use_id)
        .to_owned()
}

/// What a render forces (`NOTCHLING_RENDER_PROMPT`), since no hook holds a prompt in a picture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preview {
    Question,
    Permission,
}

impl Preview {
    /// `question` or `permission`; anything else shows nothing.
    pub fn parse(value: &str) -> Option<Preview> {
        match value {
            "question" => Some(Preview::Question),
            "permission" => Some(Preview::Permission),
            _ => None,
        }
    }
}

/// The oldest prompt a hook holds for `agent` (it has waited longest; the next shows once that
/// one is answered). The daemon's list when it is followed, else the Swift hook's files. An
/// Antigravity permission (`agy`) isn't Claude's and has its own card.
fn held(
    agent: &Agent,
    prompts: Option<&[Value]>,
    files: &HashMap<String, HookRequest>,
) -> Option<HookRequest> {
    let session = agent.session_id();
    let mine = |request: &HookRequest| {
        request.session_id == agent.id || session.as_deref() == Some(request.session_id.as_str())
    };
    match prompts {
        Some(list) => list
            .iter()
            .filter_map(HookRequest::from_prompt)
            .filter(|r| r.agy.is_none() && mine(r))
            .min_by(|a, b| a.created.total_cmp(&b.created)),
        None => files
            .values()
            .filter(|r| mine(r))
            .max_by(|a, b| a.created.total_cmp(&b.created))
            .cloned(),
    }
}

/// The prompt Chat shows for a row, or `None`. `answered` is what the windows already answered
/// (`session_source::View::answered`).
pub fn prompt_for(
    agent: &Agent,
    tool: Tool,
    prompts: Option<&[Value]>,
    files: &HashMap<String, HookRequest>,
    from_transcript: Option<&PendingQuestion>,
    from_screen: Option<&PendingQuestion>,
    answered: &HashSet<String>,
) -> Option<Prompt> {
    if tool != Tool::Claude || agent.command.is_some() {
        return None;
    }
    let request = held(agent, prompts, files).filter(|r| !answered.contains(&r.id));
    if let Some(r) = &request {
        if r.kind == RequestKind::Permission {
            return Some(Prompt::Permission(Box::new(r.clone())));
        }
    }
    let question = request
        .as_ref()
        .and_then(|r| r.question().cloned())
        .or_else(|| from_transcript.cloned())
        .or_else(|| from_screen.cloned())
        .or_else(|| question_from_needs(agent))
        .filter(|q| !answered.contains(&q.tool_use_id));
    if let Some(q) = question {
        return Some(Prompt::Question(q));
    }
    permission_from_needs(agent)
        .filter(|r| !answered.contains(&r.id))
        .map(|r| Prompt::Permission(Box::new(r)))
}

/// What a screen read of `agent`'s question is for: a background Claude job waiting on an answer,
/// keyed by its row and state summary, so the screen is read once per question, not per draw.
pub fn screen_read_key(agent: &Agent, tool: Tool) -> Option<String> {
    if tool != Tool::Claude
        || agent.command.is_some()
        || agent.group != AgentGroup::NeedsInput
        || !matches!(agent.target, AgentTarget::Job(_))
    {
        return None;
    }
    let needs = agent.detail.as_deref()?;
    needs
        .starts_with("answer:")
        .then(|| format!("{}|{needs}", agent.id))
}

/// `session.question`'s reply as a question: `None` when no question showed.
pub fn screen_question(reply: &Value) -> Option<PendingQuestion> {
    let items = crate::notch::requests::question_items(reply);
    let id = reply.get("id").and_then(Value::as_str)?;
    (!items.is_empty()).then(|| PendingQuestion {
        tool_use_id: id.to_owned(),
        items,
    })
}

/// The made-up prompt a render draws.
pub fn preview_prompt(preview: Preview) -> Prompt {
    use notchling_engine::transcript::snapshot::QuestionItem;
    match preview {
        Preview::Question => Prompt::Question(PendingQuestion {
            tool_use_id: "hook:preview".into(),
            items: vec![
                QuestionItem {
                    header: "Database".into(),
                    question: "Which database should the new service use?".into(),
                    options: vec![
                        "Postgres".into(),
                        "SQLite".into(),
                        "Keep what it has".into(),
                    ],
                    details: vec![
                        "Matches production, needs a running server".into(),
                        "One file, nothing to run".into(),
                        String::new(),
                    ],
                    multi_select: false,
                },
                QuestionItem {
                    header: "Checks".into(),
                    question: "Which checks should run before it merges?".into(),
                    options: vec!["Unit tests".into(), "Lint".into(), "Type check".into()],
                    details: vec![String::new(); 3],
                    multi_select: true,
                },
            ],
        }),
        Preview::Permission => Prompt::Permission(Box::new(HookRequest {
            id: "preview".into(),
            session_id: "preview".into(),
            created: 0.0,
            kind: RequestKind::Permission,
            tool_name: "Bash".into(),
            summary: "sqlx migrate run --database-url postgres://localhost/api_dev".into(),
            can_always_allow: true,
            agy: None,
        })),
    }
}

/// The marker of the daemon's dialog refusal (`engine::reply::exec::Failure::Dialog`).
const DIALOG: &str = "The session is asking you something";

/// Whether a failed send was refused because the session has a question or permission open.
pub fn is_dialog(message: &str) -> bool {
    message.contains(DIALOG)
}

/// What the status line under the reply box says about a failed send. The daemon's dialog refusal
/// points at the terminal; with a card showing above the box, the answer is there.
pub fn error_text(message: &str, card_shown: bool) -> String {
    if card_shown && is_dialog(message) {
        message.replace("Answer it in the terminal", "Answer it above")
    } else {
        message.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::{AgentGroup, AgentTarget};
    use serde_json::json;

    fn job(id: &str, session: &str, needs: Option<&str>) -> Agent {
        Agent {
            id: id.into(),
            name: "job".into(),
            project: "p".into(),
            cwd: "/w/p".into(),
            group: AgentGroup::NeedsInput,
            label: "Needs input".into(),
            detail: needs.map(str::to_owned),
            since_ms: 1,
            target: AgentTarget::Job(id.into()),
            suggested_reply: None,
            transcript_path: Some(format!("/c/projects/-w-p/{session}.jsonl")),
            host: "Background".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
        }
    }

    fn permission_prompt(id: &str, session: &str, created: i64) -> Value {
        json!({"id": id, "sessionId": session, "kind": "permission", "toolName": "Bash",
               "summary": "rm -rf build", "canAlwaysAllow": true, "createdMs": created})
    }

    fn question_prompt(id: &str, session: &str) -> Value {
        json!({"id": id, "sessionId": session, "kind": "question", "toolName": "AskUserQuestion",
               "summary": "Which db?", "createdMs": 5,
               "question": {"items": [{"header": "DB", "question": "Which db?",
                   "options": ["Postgres", "SQLite"], "details": ["", ""], "multiSelect": false}]}})
    }

    fn none() -> HashMap<String, HookRequest> {
        HashMap::new()
    }

    fn pick(
        agent: &Agent,
        prompts: Option<&[Value]>,
        transcript: Option<&PendingQuestion>,
        answered: &HashSet<String>,
    ) -> Option<Prompt> {
        prompt_for(
            agent,
            Tool::Claude,
            prompts,
            &none(),
            transcript,
            None,
            answered,
        )
    }

    fn q(id: &str, text: &str) -> PendingQuestion {
        use notchling_engine::transcript::snapshot::QuestionItem;
        PendingQuestion {
            tool_use_id: id.into(),
            items: vec![QuestionItem {
                header: String::new(),
                question: text.into(),
                options: vec!["A".into(), "B".into()],
                details: vec![String::new(); 2],
                multi_select: false,
            }],
        }
    }

    #[test]
    fn a_job_waiting_on_an_answer_is_read_off_its_screen_once_per_summary() {
        let needs = "answer: How? (Side by side (Recommended) · Full)";
        let agent = job("job1", "sess-1", Some(needs));
        assert_eq!(
            screen_read_key(&agent, Tool::Claude).as_deref(),
            Some("job1|answer: How? (Side by side (Recommended) · Full)")
        );
        assert_eq!(screen_read_key(&agent, Tool::Codex), None);
        assert_eq!(
            screen_read_key(&job("j", "s", Some("approve Bash: ls")), Tool::Claude),
            None
        );
        let mut working = agent.clone();
        working.group = AgentGroup::Working;
        assert_eq!(screen_read_key(&working, Tool::Claude), None);
        // The read wins over the summary, and keeps its brackets.
        let reply = json!({"id": "screen:job1:How?", "items": [{"header": "", "question": "How?",
            "options": ["Side by side (Recommended)", "Full"], "details": ["", ""], "multiSelect": false}]});
        let read = screen_question(&reply).expect("a question");
        let shown = prompt_for(
            &agent,
            Tool::Claude,
            None,
            &none(),
            None,
            Some(&read),
            &HashSet::new(),
        );
        let Some(Prompt::Question(q)) = shown else {
            panic!("a question")
        };
        assert_eq!(q.items[0].options, ["Side by side (Recommended)", "Full"]);
        assert_eq!(q.tool_use_id, "screen:job1:How?");
        assert_eq!(screen_question(&json!({"items": []})), None);
    }

    #[test]
    fn a_held_permission_shows_by_the_sessions_id_and_the_oldest_first() {
        let agent = job("job1", "sess-1", None);
        let prompts = vec![
            permission_prompt("p2", "sess-1", 2_000),
            permission_prompt("p1", "sess-1", 1_000),
            permission_prompt("other", "sess-9", 500),
        ];
        let shown = pick(&agent, Some(&prompts), None, &HashSet::new()).unwrap();
        let Prompt::Permission(r) = shown else {
            panic!("a permission")
        };
        assert_eq!((r.id.as_str(), r.tool_name.as_str()), ("p1", "Bash"));
    }

    #[test]
    fn the_oldest_held_prompt_shows_and_a_hook_question_beats_the_transcripts() {
        let agent = job("job1", "sess-1", Some("answer: Pick? (X · Y)"));
        let both = vec![
            question_prompt("q1", "sess-1"),
            permission_prompt("p1", "sess-1", 1),
        ];
        assert!(matches!(
            pick(&agent, Some(&both), None, &HashSet::new()),
            Some(Prompt::Permission(_))
        ));
        let only_q = vec![question_prompt("q1", "sess-1")];
        let transcript = q("toolu_1", "From the transcript?");
        let shown = pick(&agent, Some(&only_q), Some(&transcript), &HashSet::new()).unwrap();
        assert_eq!(shown.key(), "q1", "answered by the hook prompt's id");
        let Prompt::Question(question) = shown else {
            panic!("a question")
        };
        assert_eq!(question.items[0].question, "Which db?");
    }

    #[test]
    fn without_a_hook_the_transcript_then_the_jobs_state_summary() {
        let agent = job(
            "job1",
            "sess-1",
            Some("answer: Which target? (Prod · Staging)"),
        );
        let transcript = q("toolu_1", "From the transcript?");
        let shown = pick(&agent, Some(&[]), Some(&transcript), &HashSet::new()).unwrap();
        assert_eq!(shown.key(), "toolu_1");
        let from_needs = pick(&agent, Some(&[]), None, &HashSet::new()).unwrap();
        let Prompt::Question(question) = from_needs else {
            panic!("a question")
        };
        assert_eq!(question.items[0].question, "Which target?");
        assert_eq!(question.items[0].options, ["Prod", "Staging"]);
        assert!(question.tool_use_id.starts_with("needs:job1:"));
    }

    #[test]
    fn a_job_waiting_on_a_permission_shows_it_from_its_state_and_answers_it_on_screen() {
        let agent = job("job1", "sess-1", Some("approve Bash: ls -la && pwd"));
        let shown = pick(&agent, Some(&[]), None, &HashSet::new()).unwrap();
        let Prompt::Permission(r) = shown.clone() else {
            panic!("a permission")
        };
        assert_eq!(r.tool_name, "Bash");
        assert_eq!(r.summary, "ls -la && pwd");
        assert!(
            r.can_always_allow,
            "the daemon refuses it if the dialog has none"
        );
        // Answered in the job's terminal, by the row, with what the card showed.
        assert_eq!(
            permission_request(&r, Decision::Deny),
            Request::permission_on_screen("job1", Decision::Deny, "Bash", "ls -la && pwd")
        );
        assert_eq!(
            permission_request(&r, Decision::Allow).args,
            json!({"id": "job1", "decision": "allow",
                   "expect": {"tool": "Bash", "summary": "ls -la && pwd"}})
        );
        // The card is hidden by its key once answered, and shows again for another request.
        let answered: HashSet<String> = [shown.key()].into();
        assert_eq!(pick(&agent, Some(&[]), None, &answered), None);
        let next = job("job1", "sess-1", Some("approve Bash: cargo test"));
        assert!(pick(&next, Some(&[]), None, &answered).is_some());
        // A hook's prompt for the same session is the real one, decided by its own id.
        let held = vec![permission_prompt("p1", "sess-1", 1)];
        let Some(Prompt::Permission(hook)) = pick(&agent, Some(&held), None, &HashSet::new())
        else {
            panic!("the hook's permission")
        };
        assert_eq!(
            permission_request(&hook, Decision::Always),
            Request::permission("p1", Decision::Always)
        );
        // Not for a session that isn't waiting.
        let mut idle = agent.clone();
        idle.group = AgentGroup::Working;
        assert_eq!(pick(&idle, Some(&[]), None, &HashSet::new()), None);
        // A tool with no summary still shows.
        let bare = job("job2", "s2", Some("approve Edit"));
        assert!(pick(&bare, Some(&[]), None, &HashSet::new()).is_some());
    }

    #[test]
    fn what_was_answered_is_hidden_by_its_key() {
        let agent = job("job1", "sess-1", None);
        let prompts = vec![permission_prompt("p1", "sess-1", 1_000)];
        let answered: HashSet<String> = ["p1".to_owned()].into();
        assert_eq!(pick(&agent, Some(&prompts), None, &answered), None);
        let transcript = q("toolu_1", "Q?");
        let answered: HashSet<String> = ["toolu_1".to_owned()].into();
        assert_eq!(pick(&agent, None, Some(&transcript), &answered), None);
        // A hook question is hidden by the prompt's id, not the `hook:` name.
        let prompts = vec![question_prompt("q1", "sess-1")];
        let answered: HashSet<String> = ["q1".to_owned()].into();
        assert_eq!(pick(&agent, Some(&prompts), None, &answered), None);
    }

    #[test]
    fn only_a_claude_code_session_that_isnt_a_command_has_a_card() {
        let agent = job("job1", "sess-1", None);
        let prompts = vec![permission_prompt("p1", "sess-1", 1)];
        let by_tool = |tool| {
            prompt_for(
                &agent,
                tool,
                Some(&prompts),
                &none(),
                None,
                None,
                &HashSet::new(),
            )
        };
        assert!(by_tool(Tool::Claude).is_some());
        assert!(by_tool(Tool::Codex).is_none() && by_tool(Tool::Antigravity).is_none());
        let mut command = agent.clone();
        command.command = Some("make".into());
        assert_eq!(
            prompt_for(
                &command,
                Tool::Claude,
                Some(&prompts),
                &none(),
                None,
                None,
                &HashSet::new()
            ),
            None
        );
        // An Antigravity permission has its own card.
        let agy = json!({"id": "agy-perm:agy:s:1", "sessionId": "agy:s", "tool": "agy",
            "kind": "permission", "rpcId": "1", "options": [{"id": "a", "name": "Allow"}]});
        let row = job("agy:s", "s", None);
        assert_eq!(pick(&row, Some(&[agy]), None, &HashSet::new()), None);
    }

    #[test]
    fn the_swift_hooks_files_are_read_only_while_the_daemons_prompts_are_not_followed() {
        let agent = job("job1", "sess-1", None);
        let request = HookRequest::parse(&json!({
            "id": "r1", "session_id": "sess-1", "pid": 4, "created": 10.0,
            "hook_event_name": "PermissionRequest", "tool_name": "Bash",
            "tool_input": {"command": "ls"}
        }))
        .unwrap();
        let files = HashMap::from([("sess-1".to_owned(), request)]);
        let from_files = |prompts: Option<&[Value]>| {
            prompt_for(
                &agent,
                Tool::Claude,
                prompts,
                &files,
                None,
                None,
                &HashSet::new(),
            )
        };
        assert!(matches!(from_files(None), Some(Prompt::Permission(_))));
        assert_eq!(
            from_files(Some(&[])),
            None,
            "the daemon's list is the truth"
        );
    }

    #[test]
    fn the_dialog_refusal_points_above_only_while_a_card_shows() {
        let refusal =
            "The session is asking you something. Answer it in the terminal, then send again.";
        assert_eq!(
            error_text(refusal, true),
            "The session is asking you something. Answer it above, then send again."
        );
        assert_eq!(error_text(refusal, false), refusal);
        assert_eq!(
            error_text("Workspace not trusted: /w", true),
            "Workspace not trusted: /w"
        );
        assert!(is_dialog(refusal));
        assert!(!is_dialog("acting is off"));
    }

    #[test]
    fn the_wording_matches_what_the_engine_says() {
        use notchling_engine::reply::exec::Failure;
        assert!(is_dialog(&Failure::Dialog.to_string()));
        assert!(Failure::Dialog
            .to_string()
            .contains("Answer it in the terminal"));
    }

    #[test]
    fn a_render_preview_has_a_question_with_two_tabs_and_a_permission() {
        assert_eq!(Preview::parse("question"), Some(Preview::Question));
        assert_eq!(Preview::parse("permission"), Some(Preview::Permission));
        assert_eq!(Preview::parse("1"), None);
        let Prompt::Question(question) = preview_prompt(Preview::Question) else {
            panic!("a question")
        };
        assert_eq!(question.items.len(), 2);
        assert!(question.items[1].multi_select);
        assert_eq!(
            preview_prompt(Preview::Permission).part(),
            "Permission Card"
        );
    }
}
