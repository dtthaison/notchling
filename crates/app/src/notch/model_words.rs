use notchling_engine::sessions::{Agent, AgentGroup, AgentTarget};
use notchling_engine::transcript::snapshot::{PendingQuestion, QuestionItem};
use notchling_kit::Status;
use notchling_locale as t;

use super::model::NotchModel;

pub fn status(group: AgentGroup) -> Status {
    match group {
        AgentGroup::NeedsInput => Status::Needs,
        AgentGroup::Working => Status::Working,
        AgentGroup::Completed => Status::Done,
    }
}

pub fn tab_title(group: AgentGroup) -> &'static str {
    match group {
        AgentGroup::NeedsInput => t::notch::tab_input(),
        AgentGroup::Working => t::notch::tab_working(),
        AgentGroup::Completed => t::notch::tab_done(),
    }
}

pub fn tab_id(group: AgentGroup) -> &'static str {
    match group {
        AgentGroup::NeedsInput => "input",
        AgentGroup::Working => "working",
        AgentGroup::Completed => "done",
    }
}

pub fn empty_text(model: &NotchModel) -> String {
    if model.agents.is_empty() {
        t::notch::empty_no_agents().to_owned()
    } else {
        match model.tab {
            AgentGroup::NeedsInput => t::notch::empty_input(),
            AgentGroup::Working => t::notch::empty_working(),
            AgentGroup::Completed => t::notch::empty_completed(),
        }
        .to_owned()
    }
}

pub fn short_host(host: &str) -> &str {
    match host {
        "Background" => t::notch::host_background(),
        "claude -p" => "-p",
        other => other,
    }
}

pub fn ago(since_ms: i64, now_ms: i64) -> String {
    let s = ((now_ms - since_ms) / 1000).max(0);
    match s {
        0..=59 => t::notch::age_now().to_owned(),
        60..=3599 => t::notch::age_minutes(s / 60),
        3600..=86_399 => t::notch::age_hours(s / 3600),
        _ => t::notch::age_days(s / 86_400),
    }
}

pub fn one_line(text: &str) -> String {
    text.replace('\n', " ").trim().to_owned()
}

pub fn last_paragraph(text: &str) -> String {
    let paragraph = text
        .rsplit("\n\n")
        .find(|p| !p.trim().is_empty())
        .unwrap_or(text);
    paragraph.chars().take(400).collect()
}

pub fn question_from_needs(agent: &Agent) -> Option<PendingQuestion> {
    if agent.group != AgentGroup::NeedsInput || !matches!(agent.target, AgentTarget::Job(_)) {
        return None;
    }
    let needs = agent.detail.as_deref()?;
    let body = needs.strip_prefix("answer: ")?.strip_suffix(')')?;
    let (text, options) = body.rsplit_once(" (")?;
    let options: Vec<String> = options.split(" · ").map(|o| o.trim().to_owned()).collect();
    if options.len() < 2 || text.contains('(') {
        return None;
    }
    let details = vec![String::new(); options.len()];
    Some(PendingQuestion {
        tool_use_id: format!("needs:{}:{needs}", agent.id),
        items: vec![QuestionItem {
            header: String::new(),
            question: text.to_owned(),
            options,
            details,
            multi_select: false,
        }],
    })
}
