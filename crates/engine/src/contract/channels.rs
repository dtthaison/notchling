use super::channel::{Abilities, Ability, Channel, Mode};
use super::Tool;

pub struct ClaudeTerminal;

pub struct CodexExec;

pub struct AgyAcp;

pub struct AgyApp;

pub struct AgyRemote;

pub struct AgySidecar;

pub const ACP: &str = "acp";

pub const APP: &str = "app";

pub const REMOTE: &str = "remote";

pub const SIDECAR: &str = "sidecar";

impl Channel for ClaudeTerminal {
    fn id(&self) -> &'static str {
        "terminal"
    }

    fn tool(&self) -> Tool {
        Tool::Claude
    }

    fn title(&self) -> &'static str {
        "Claude Code (the session's terminal)"
    }

    fn mode(&self) -> Mode {
        Mode::Sends
    }

    fn abilities(&self) -> Abilities {
        Abilities::ALL
    }
}

impl Channel for CodexExec {
    fn id(&self) -> &'static str {
        "exec"
    }

    fn tool(&self) -> Tool {
        Tool::Codex
    }

    fn title(&self) -> &'static str {
        "Codex (codex exec)"
    }

    fn mode(&self) -> Mode {
        Mode::Sends
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            approve: false,
            answer: false,
            ..Abilities::ALL
        }
    }
}

impl Channel for AgyAcp {
    fn id(&self) -> &'static str {
        ACP
    }

    fn tool(&self) -> Tool {
        Tool::Antigravity
    }

    fn title(&self) -> &'static str {
        "Antigravity CLI (agy acp)"
    }

    fn mode(&self) -> Mode {
        Mode::Sends
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            answer: false,
            ..Abilities::ALL
        }
    }
}

impl Channel for AgyApp {
    fn id(&self) -> &'static str {
        APP
    }

    fn tool(&self) -> Tool {
        Tool::Antigravity
    }

    fn title(&self) -> &'static str {
        "the Antigravity app (a draft you send there)"
    }

    fn mode(&self) -> Mode {
        Mode::NewDraft
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            reply: true,
            start: true,
            ..Abilities::NONE
        }
    }
}

impl Channel for AgyRemote {
    fn id(&self) -> &'static str {
        REMOTE
    }

    fn tool(&self) -> Tool {
        Tool::Antigravity
    }

    fn title(&self) -> &'static str {
        "Antigravity Remote Control (the browser, you press Send there)"
    }

    fn mode(&self) -> Mode {
        Mode::ExistingSession
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            reply: true,
            start: true,
            ..Abilities::NONE
        }
    }
}

impl Channel for AgySidecar {
    fn id(&self) -> &'static str {
        SIDECAR
    }

    fn tool(&self) -> Tool {
        Tool::Antigravity
    }

    fn title(&self) -> &'static str {
        "Antigravity app (full)"
    }

    fn mode(&self) -> Mode {
        Mode::Sends
    }

    fn abilities(&self) -> Abilities {
        Abilities {
            reply: true,
            start: true,
            phone: true,
            soft_stop: true,
            approve: true,
            ..Abilities::NONE
        }
    }

    fn gap_notes(&self) -> &'static [(Ability, &'static str)] {
        &[
            (
                Ability::Stop,
                "No hook or sidecar call cancels a running turn; a stop ends it softly at the next model call (PreInvocation terminate).",
            ),
            (
                Ability::Answer,
                "No hook answers the agent's ask_question; the question is answered in the Antigravity app.",
            ),
        ]
    }
}

static CLAUDE: [&dyn Channel; 1] = [&ClaudeTerminal];

static CODEX: [&dyn Channel; 1] = [&CodexExec];

static ANTIGRAVITY: [&dyn Channel; 4] = [&AgyAcp, &AgyApp, &AgyRemote, &AgySidecar];

pub fn of(tool: Tool) -> &'static [&'static dyn Channel] {
    match tool {
        Tool::Claude => &CLAUDE,
        Tool::Codex => &CODEX,
        Tool::Antigravity => &ANTIGRAVITY,
    }
}
