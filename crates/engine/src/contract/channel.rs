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
}

impl Ability {
    pub const ALL: [Ability; 4] = [
        Ability::Reply,
        Ability::Start,
        Ability::Phone,
        Ability::Stop,
    ];

    pub fn verb(self) -> &'static str {
        match self {
            Ability::Reply => "take replies",
            Ability::Start => "start sessions",
            Ability::Phone => "be used from the phone",
            Ability::Stop => "be stopped from Notchling",
        }
    }

    pub fn capability(self) -> Option<Capability> {
        match self {
            Ability::Reply => Some(Capability::TurnSend),
            Ability::Start => Some(Capability::SessionNew),
            Ability::Stop => Some(Capability::TurnInterrupt),
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
}

impl Abilities {
    pub const ALL: Abilities = Abilities {
        reply: true,
        start: true,
        phone: true,
        stop: true,
    };

    pub fn has(self, ability: Ability) -> bool {
        match ability {
            Ability::Reply => self.reply,
            Ability::Start => self.start,
            Ability::Phone => self.phone,
            Ability::Stop => self.stop,
        }
    }
}

pub trait Channel: Sync {
    fn id(&self) -> &'static str;

    fn tool(&self) -> Tool;

    fn title(&self) -> &'static str;

    fn mode(&self) -> Mode;

    fn abilities(&self) -> Abilities;
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
