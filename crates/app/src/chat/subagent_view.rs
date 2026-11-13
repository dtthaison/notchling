use std::sync::Arc;

use notchling_engine::transcript::model::Conversation;
use notchling_engine::transcript::subagents::Subagent;

use super::inspector::subagent_row;

#[derive(Clone, Debug)]
pub struct Open {
    pub agent: Subagent,
    pub conversation: Option<Arc<Conversation>>,
}

impl Open {
    pub fn new(agent: &Subagent) -> Open {
        Open {
            agent: agent.clone(),
            conversation: None,
        }
    }

    pub fn apply_shared(&mut self, listed: &[Subagent], read: Option<(String, Arc<Conversation>)>) {
        if let Some(agent) = find(listed, &self.agent.id) {
            self.agent = agent.clone();
        }
        if let Some((path, conversation)) = read {
            if path == self.agent.path {
                self.conversation = Some(conversation);
            }
        }
    }

    pub fn apply(&mut self, listed: &[Subagent], read: Option<(String, Conversation)>) {
        self.apply_shared(listed, read.map(|(path, c)| (path, Arc::new(c))));
    }
}

pub fn find<'a>(subagents: &'a [Subagent], id: &str) -> Option<&'a Subagent> {
    subagents.iter().find(|s| s.id == id)
}

pub fn for_step<'a>(subagents: &'a [Subagent], tool_use_id: &str) -> Option<&'a Subagent> {
    if tool_use_id.is_empty() {
        return None;
    }
    subagents
        .iter()
        .find(|s| s.tool_use_id.as_deref() == Some(tool_use_id))
}

pub fn links(subagents: &[Subagent]) -> Vec<(Option<String>, String)> {
    subagents
        .iter()
        .map(|s| (s.tool_use_id.clone(), s.id.clone()))
        .collect()
}

pub fn link_for_step<'a>(
    links: &'a [(Option<String>, String)],
    tool_use_id: &str,
) -> Option<&'a str> {
    if tool_use_id.is_empty() {
        return None;
    }
    links
        .iter()
        .find(|(call, _)| call.as_deref() == Some(tool_use_id))
        .map(|(_, id)| id.as_str())
}

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
            origin: Default::default(),
        }
    }

    #[test]
    fn a_step_finds_the_subagent_its_tool_use_id_names() {
        let list = [agent("a", Some("toolu_1")), agent("b", Some("toolu_2"))];
        assert_eq!(for_step(&list, "toolu_2").map(|s| s.id.as_str()), Some("b"));
        assert!(for_step(&list, "toolu_9").is_none());
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

    #[test]
    fn an_agent_step_finds_its_subagent_in_the_links() {
        let links = links(&[agent("a", Some("call-1")), agent("b", None)]);
        assert_eq!(link_for_step(&links, "call-1"), Some("a"));
        assert_eq!(link_for_step(&links, "call-2"), None);
        assert_eq!(link_for_step(&links, ""), None);
    }
}
