//! A subagent opened from the Details Panel: its own transcript read-only where the conversation
//! goes (the Swift `ChatModel.subagentID`). Which one is open, how a step's tool use id finds its
//! subagent and what the slim header says are plain functions here; the window keeps one [`Open`].

use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::Subagent;

use super::inspector::subagent_row;

/// The subagent shown in place of the conversation.
#[derive(Clone, Debug)]
pub struct Open {
    /// As the last load listed it, so the header's status word stays current.
    pub agent: Subagent,
    /// Its transcript once a load has read it; `None` until then.
    pub conversation: Option<Conversation>,
}

impl Open {
    pub fn new(agent: &Subagent) -> Open {
        Open {
            agent: agent.clone(),
            conversation: None,
        }
    }

    /// Takes a finished load: the listing's newer copy of the subagent (a missing one stays as it
    /// was, the list can be empty for a moment) and, when `read` is this subagent's file, its
    /// conversation.
    pub fn apply(&mut self, listed: &[Subagent], read: Option<(String, Conversation)>) {
        if let Some(agent) = find(listed, &self.agent.id) {
            self.agent = agent.clone();
        }
        if let Some((path, conversation)) = read {
            if path == self.agent.path {
                self.conversation = Some(conversation);
            }
        }
    }
}

/// The subagent with this id.
pub fn find<'a>(subagents: &'a [Subagent], id: &str) -> Option<&'a Subagent> {
    subagents.iter().find(|s| s.id == id)
}

/// The subagent an Agent step started: its meta file names the call's tool use id.
pub fn for_step<'a>(subagents: &'a [Subagent], tool_use_id: &str) -> Option<&'a Subagent> {
    if tool_use_id.is_empty() {
        return None;
    }
    subagents
        .iter()
        .find(|s| s.tool_use_id.as_deref() == Some(tool_use_id))
}

/// Which subagent each Agent call opens (its tool call id, the subagent's id): all a conversation
/// shows of the list, so a load whose subagents only moved on (a running one's file grew) leaves
/// the conversation as it was.
pub fn links(subagents: &[Subagent]) -> Vec<(Option<String>, String)> {
    subagents
        .iter()
        .map(|s| (s.tool_use_id.clone(), s.id.clone()))
        .collect()
}

/// What the slim header above a subagent's transcript says: its description, and the detail line
/// its row in the panel shows ("Running · sonnet high · Explore").
pub fn header_text(agent: &Subagent) -> (String, String) {
    (agent.description.clone(), subagent_row(agent).detail)
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::transcript::subagents::Status;

    fn agent(id: &str, tool_use: Option<&str>) -> Subagent {
        Subagent {
            id: id.into(),
            path: format!("/s/subagents/agent-{id}.jsonl"),
            description: format!("job {id}"),
            agent_type: "Explore".into(),
            model: Some("sonnet".into()),
            effort: Some("high".into()),
            background: false,
            tool_use_id: tool_use.map(str::to_owned),
            started: 1.0,
            updated: 2.0,
            status: Status::Running,
        }
    }

    #[test]
    fn a_step_finds_the_subagent_its_tool_use_id_names() {
        let list = [agent("a", Some("toolu_1")), agent("b", Some("toolu_2"))];
        assert_eq!(for_step(&list, "toolu_2").map(|s| s.id.as_str()), Some("b"));
        assert!(for_step(&list, "toolu_9").is_none());
        // No id on either side never matches.
        let bare = [agent("c", None)];
        assert!(for_step(&bare, "").is_none());
        assert!(for_step(&bare, "toolu_1").is_none());
    }

    #[test]
    fn links_change_with_a_new_subagent_and_not_with_one_moving_on() {
        let before = [agent("a", Some("toolu_1"))];
        let mut moved = agent("a", Some("toolu_1"));
        moved.updated = 9.0;
        moved.status = Status::Completed;
        assert_eq!(links(&before), links(&[moved.clone()]));
        assert_ne!(links(&before), links(&[moved, agent("b", Some("toolu_2"))]));
    }

    #[test]
    fn the_header_names_the_subagent_and_says_how_it_is_going() {
        let (title, detail) = header_text(&agent("a", None));
        assert_eq!(title, "job a");
        assert_eq!(detail, "Running · sonnet high · Explore");
    }

    #[test]
    fn a_load_refreshes_the_open_subagent_and_takes_only_its_own_transcript() {
        let mut open = Open::new(&agent("a", None));
        assert!(open.conversation.is_none());

        // Another file's conversation is not this one's.
        open.apply(
            &[],
            Some(("/elsewhere.jsonl".into(), Conversation::default())),
        );
        assert!(open.conversation.is_none());

        let mut done = agent("a", None);
        done.status = Status::Completed;
        let path = done.path.clone();
        open.apply(&[done], Some((path, Conversation::default())));
        assert!(open.conversation.is_some());
        assert_eq!(open.agent.status, Status::Completed);

        // A listing without it leaves it open as it was.
        open.apply(&[], None);
        assert_eq!(open.agent.status, Status::Completed);
        assert!(open.conversation.is_some());
    }

    #[test]
    fn find_picks_by_agent_id() {
        let list = [agent("a", None), agent("b", None)];
        assert_eq!(find(&list, "b").map(|s| s.id.as_str()), Some("b"));
        assert!(find(&list, "z").is_none());
    }
}
