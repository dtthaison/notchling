use notchling_locale as t;

use crate::welcome::facts::HookFacts;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Plan {
    Confirm { title: String, lines: Vec<String> },
    Say { text: String, problem: bool },
}

fn writes(action: &str) -> bool {
    matches!(action, "write" | "update" | "remove")
}

pub fn plan_of(name: &str, facts: &HookFacts) -> Plan {
    let say = |text: String, problem: bool| Plan::Say { text, problem };
    if !facts.install_on {
        return say(t::cluster::install_is_off(name), true);
    }
    if let Some(why) = &facts.refused {
        return say(t::cluster::install_refused(name, why), true);
    }
    if let Some(why) = &facts.blocked {
        return say(why.clone(), true);
    }
    let skills: Vec<&str> = facts
        .skills
        .iter()
        .filter(|s| writes(&s.action))
        .map(|s| s.name.as_str())
        .collect();
    if facts.changes.is_empty() && skills.is_empty() {
        return say(t::cluster::install_already(name), false);
    }
    let mut lines = vec![t::cluster::install_intro(name)];
    lines.extend(facts.changes.iter().cloned());
    lines.extend(facts.notes.iter().cloned());
    if !skills.is_empty() {
        lines.push(t::cluster::install_skills(&skills.join(", ")));
    }
    Plan::Confirm {
        title: t::cluster::install_title(name),
        lines,
    }
}

pub fn applied_note(name: &str, facts: &HookFacts) -> (String, bool) {
    if facts.applied {
        (t::cluster::install_done(name), false)
    } else {
        (t::cluster::install_nothing(name), true)
    }
}
