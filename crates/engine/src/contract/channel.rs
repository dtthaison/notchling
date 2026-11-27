use super::{Capability, Tool};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Mode {
    Sends,
    NewDraft,
    ExistingSession,
}

impl Mode {
    pub fn user_sends(self) -> bool {
        !matches!(self, Mode::Sends)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ability {
    Reply,
    Start,
    Phone,
    Stop,
    SoftStop,
    Approve,
    Answer,
}

impl Ability {
    pub const ALL: [Ability; 7] = [
        Ability::Reply,
        Ability::Start,
        Ability::Phone,
        Ability::Stop,
        Ability::SoftStop,
        Ability::Approve,
        Ability::Answer,
    ];

    pub fn verb(self) -> &'static str {
        match self {
            Ability::Reply => "take replies",
            Ability::Start => "start sessions",
            Ability::Phone => "be used from the phone",
            Ability::Stop => "be stopped from Notchling",
            Ability::SoftStop => "be asked to stop at its next step",
            Ability::Approve => "have its permission prompts answered from Notchling",
            Ability::Answer => "have its questions answered from Notchling",
        }
    }

    pub fn capability(self) -> Option<Capability> {
        match self {
            Ability::Reply => Some(Capability::TurnSend),
            Ability::Start => Some(Capability::SessionNew),
            Ability::Stop | Ability::SoftStop => Some(Capability::TurnInterrupt),
            Ability::Approve => Some(Capability::PromptPermission),
            Ability::Answer => Some(Capability::PromptQuestion),
            Ability::Phone => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Abilities {
    pub reply: bool,
    pub start: bool,
    pub phone: bool,
    pub stop: bool,
    pub soft_stop: bool,
    pub approve: bool,
    pub answer: bool,
}

impl Abilities {
    pub const ALL: Abilities = Abilities {
        reply: true,
        start: true,
        phone: true,
        stop: true,
        soft_stop: true,
        approve: true,
        answer: true,
    };

    pub const NONE: Abilities = Abilities {
        reply: false,
        start: false,
        phone: false,
        stop: false,
        soft_stop: false,
        approve: false,
        answer: false,
    };

    pub fn missing(self) -> Vec<Ability> {
        Ability::ALL.into_iter().filter(|a| !self.has(*a)).collect()
    }

    pub fn has(self, ability: Ability) -> bool {
        match ability {
            Ability::Reply => self.reply,
            Ability::Start => self.start,
            Ability::Phone => self.phone,
            Ability::Stop => self.stop,
            Ability::SoftStop => self.soft_stop,
            Ability::Approve => self.approve,
            Ability::Answer => self.answer,
        }
    }
}

pub trait Channel: Sync {
    fn id(&self) -> &'static str;

    fn tool(&self) -> Tool;

    fn title(&self) -> &'static str;

    fn mode(&self) -> Mode;

    fn abilities(&self) -> Abilities;

    fn gap_notes(&self) -> &'static [(Ability, &'static str)] {
        &[]
    }
}

pub trait Opener {
    fn installed(&self) -> bool;

    fn open(&mut self, url: &str);

    fn panel(&mut self, url: &str);

    fn copy(&mut self, text: &str);
}

pub fn refusal(channel: &dyn Channel, ability: Ability) -> Option<String> {
    if channel.abilities().has(ability) {
        return None;
    }
    Some(format!(
        "{} can't {} while Notchling talks to it through {}.",
        channel.tool().title(),
        ability.verb(),
        channel.title()
    ))
}

pub fn of(tool: Tool) -> &'static [&'static dyn Channel] {
    super::channels::of(tool)
}

pub fn setting_key(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Antigravity => Some("antigravityChannel"),
        Tool::Claude | Tool::Codex => None,
    }
}

pub fn chosen(tool: Tool, value: Option<&str>) -> &'static dyn Channel {
    let all = of(tool);
    value
        .and_then(|id| all.iter().copied().find(|c| c.id() == id))
        .unwrap_or(all[0])
}

pub fn chooses(tool: Tool) -> bool {
    of(tool).len() > 1
}

#[cfg(test)]
#[path = "channel_tests.rs"]
mod tests;
