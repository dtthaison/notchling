use super::agents::Access;
use serde_json::{json, Value};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int,
    Double,
    String,
    StringList,
    Dict,
    Json,
    Any,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fallback {
    None,
    Bool(bool),
    Int(i64),
    Double(f64),
    Str(&'static str),
    Empty,
}

#[derive(Clone, Copy, Debug)]
pub struct KeySpec {
    pub name: &'static str,
    pub kind: Kind,
    pub default: Fallback,
    pub allowed: &'static [&'static str],
    pub from_awake: bool,
    pub doc: &'static str,
    pub range: Option<(i64, i64)>,
    pub access: Access,
    pub title: &'static str,
    pub about: &'static str,
    pub why: &'static str,
}

impl KeySpec {
    pub fn default_value(&self) -> Option<Value> {
        match (self.default, self.kind) {
            (Fallback::None, _) => None,
            (Fallback::Bool(b), _) => Some(json!(b)),
            (Fallback::Int(n), _) => Some(json!(n)),
            (Fallback::Double(n), _) => Some(json!(n)),
            (Fallback::Str(s), _) => Some(json!(s)),
            (Fallback::Empty, Kind::StringList) => Some(json!([])),
            (Fallback::Empty, _) => Some(json!({})),
        }
    }
}

impl KeySpec {
    pub fn normalize(&self, value: Value) -> Value {
        let Some((low, high)) = self.range else {
            return value;
        };
        if let Some(n) = value.as_i64() {
            json!(n.clamp(low, high))
        } else if value.is_u64() {
            json!(high)
        } else {
            value
        }
    }

    pub fn accepts(&self, value: &Value) -> bool {
        match self.kind {
            Kind::Bool => value.is_boolean(),
            Kind::Int => value.is_i64() || value.is_u64(),
            Kind::Double => value.is_number(),
            Kind::String => value
                .as_str()
                .is_some_and(|s| self.allowed.is_empty() || self.allowed.contains(&s)),
            Kind::StringList => value
                .as_array()
                .is_some_and(|a| a.iter().all(Value::is_string)),
            Kind::Dict => value.is_object(),
            Kind::Json => !value.is_null(),
            Kind::Any => true,
        }
    }
}

const fn key(
    name: &'static str,
    kind: Kind,
    default: Fallback,
    from_awake: bool,
    doc: &'static str,
) -> KeySpec {
    KeySpec {
        name,
        kind,
        default,
        allowed: &[],
        from_awake,
        doc,
        range: None,
        access: Access::Unset,
        title: "",
        about: "",
        why: "",
    }
}

impl KeySpec {
    const fn ranged(mut self, low: i64, high: i64) -> KeySpec {
        self.range = Some((low, high));
        self
    }

    const fn class(
        mut self,
        access: Access,
        title: &'static str,
        about: &'static str,
        why: &'static str,
    ) -> KeySpec {
        self.access = access;
        self.title = title;
        self.about = about;
        self.why = why;
        self
    }

    pub const fn free(
        self,
        title: &'static str,
        about: &'static str,
        why: &'static str,
    ) -> KeySpec {
        self.class(Access::Free, title, about, why)
    }

    pub const fn ask(self, title: &'static str, about: &'static str, why: &'static str) -> KeySpec {
        self.class(Access::Ask, title, about, why)
    }

    pub const fn never(
        self,
        title: &'static str,
        about: &'static str,
        why: &'static str,
    ) -> KeySpec {
        self.class(Access::Never, title, about, why)
    }
}

const fn choice(
    name: &'static str,
    default: &'static str,
    allowed: &'static [&'static str],
    from_awake: bool,
    doc: &'static str,
) -> KeySpec {
    KeySpec {
        name,
        kind: Kind::String,
        default: Fallback::Str(default),
        allowed,
        from_awake,
        doc,
        range: None,
        access: Access::Unset,
        title: "",
        about: "",
        why: "",
    }
}

use Fallback as D;
use Kind as K;

pub const KEYS: &[KeySpec] = &[
    choice("mode", "off", &["off", "on", "whileWorking"], true, "Keep Mac Awake: off, always on, or while an agent works")
        .free("Keep Mac awake", "Keep the Mac awake: off, on, or while agents work", "a preference of this Mac; it gives an agent nothing"),
    key("active", K::Bool, D::Bool(false), true, "Awake v1's on/off flag; read only when `mode` is unset, so the migration folds it into `mode`")
        .never("Keep awake (old)", "The first Awake version's on and off flag, folded into Keep Mac awake", "kept only for the migration"),
    key("keepDisplayOn", K::Bool, D::Bool(false), true, "Keep the display on too, not only the system")
        .free("Keep display on", "Keep the display on too while the Mac stays awake", "a display preference"),
    key("continueAfterSleep", K::Bool, D::Bool(true), true, "Resume background sessions after the Mac wakes (WakeResumer)")
        .ask("Continue agents after sleep", "When the Mac wakes, tell background agents whose reply sleep cut off to continue", "it types into sessions, so it changes what runs"),
    key("hideNotch", K::Bool, D::Bool(false), true, "Hide the notch (the menu's Show Notch is its inverse)")
        .free("Hide the notch", "Hide the agents in the notch (Settings shows it as Show agents in the notch)", "a display preference"),
    key("answerFromNotch", K::Bool, D::Bool(true), true, "Answer questions and permissions from the notch; first launch turns it on")
        .ask("Answer prompts from the notch", "Answer questions and permission prompts from the notch", "it changes where permission prompts are answered"),
    key("keepPages", K::Bool, D::Bool(true), true, "Keep local copies of published artifacts (the page hook)")
        .ask("Keep local copies of artifacts", "Keep a local copy of every page published with the Artifact tool", "it changes what Claude Code's hook does on every publish"),
    key("showPet", K::Bool, D::Bool(true), true, "The animated pet in the notch")
        .free("Animate pet", "Show the animated pet in the notch", "a display preference"),
    key("replyFromNotch", K::Bool, D::Bool(true), true, "A reply box in the notch")
        .free("Reply from the notch", "Show a reply box for background agents in the notch", "a display preference"),
    key("showLastReply", K::Bool, D::Bool(true), true, "Show the last reply in the notch")
        .free("Show the last reply", "Show an agent's last reply when peeking at the notch", "a display preference"),
    key("askedAccessibility", K::Bool, D::Bool(false), true, "The Accessibility prompt was shown once (TabPerch)")
        .never("Accessibility asked", "The Accessibility prompt was shown once", "the app's own record"),
    choice("notchWidth", "medium", &["small", "medium", "large"], true, "Width of the notch's panel")
        .free("Notch width", "Width of the notch's panel: small, medium or large", "a display preference"),
    key("dismissed", K::Dict, D::Empty, true, "Done rows cleared in the notch: agent id to time (seconds)")
        .never("Cleared notch rows", "Done rows cleared from the notch", "the app's own record"),
    key("hiddenAgents", K::Dict, D::Empty, true, "Agents hidden in the notch: agent id to the group they were hidden in")
        .never("Hidden notch agents", "Agents hidden in the notch", "the app's own record"),
    key("accounts", K::Json, D::None, true, "Saved Claude Code accounts (JSON of AccountStore's list)")
        .never("Claude Code accounts", "The saved Claude Code accounts", "logins: only the Accounts window changes them"),
    key("untrackedCostAccount", K::Any, D::None, true, "Only in the migration list; nothing in the code reads it")
        .never("Untracked cost account", "An old key nothing reads", "kept only for the migration"),
    key("antigravityAccounts", K::Json, D::None, true, "Saved Antigravity accounts (JSON)")
        .never("Antigravity accounts", "The saved Antigravity accounts", "logins: only the Accounts window changes them"),
    key("claudeDesktopAccounts", K::StringList, D::Empty, true, "Claude Desktop accounts signed in once (account UUIDs)")
        .never("Claude Desktop accounts", "Claude Desktop accounts signed in once", "logins: only the Accounts window changes them"),
    key("codexAccounts", K::Json, D::None, true, "Saved Codex accounts (JSON)")
        .never("Codex accounts", "The saved Codex accounts", "logins: only the Accounts window changes them"),
    choice("limitMode", "notifyAndSwitch", &["off", "notify", "notifyAndSwitch"], true, "Claude Code limit watching")
        .ask("Claude Code limit", "When the Claude Code account in use nears its limit: off, notify, or notify and switch", "Notify and Switch moves sessions to another login"),
    key("limitSwitchAt", K::Int, D::Int(95), true, "Percent at which a limit switch happens (reads stay in 80 to 99)").ranged(80, 99)
        .ask("Switch at", "The percent of a limit at which Notify and Switch moves to another account, 80 to 99", "it decides when logins switch"),
    choice("codexLimitMode", "notify", &["off", "notify", "notifyAndSwitch"], true, "Codex limit watching")
        .ask("Codex limit", "When the Codex account in use nears its limit: off, notify, or notify and switch", "Notify and Switch moves to another login"),
    choice("antigravityLimitMode", "notify", &["off", "notify", "notifyAndSwitch"], true, "Antigravity limit watching")
        .ask("Antigravity limit", "When the Antigravity account in use nears its limit: off, notify, or notify and switch", "Notify and Switch moves to another login"),
    key("codexNetwork", K::Bool, D::Bool(true), true, "Open the network in Codex turns (Allow Network in Codex Turns)")
        .never("Allow network in Codex turns", "Codex turns and delegated Codex jobs may use the network", "a sandbox permission: an agent could open the network to its own delegated jobs"),
    key("migratedFromAwake", K::Bool, D::Bool(false), false, "The Awake migration already ran")
        .never("Awake migration done", "The Awake migration already ran", "the app's own record"),
    key("chatClosedSessions", K::Dict, D::Empty, false, "Sessions closed in Chat: row id to time (seconds)")
        .never("Closed Chat sessions", "Sessions closed in Chat", "the app's own record"),
    key("chatUsedSessions", K::Dict, D::Empty, false, "Codex and Antigravity sessions used in Chat: row id to time (seconds)")
        .never("Used Chat sessions", "Codex and Antigravity sessions used in Chat", "the app's own record"),
    key("chatInspector", K::Bool, D::Bool(false), false, "The Details panel is open")
        .free("Details Panel open", "Show the Details Panel beside a conversation", "a display preference"),
    key("codexPicks", K::Json, D::None, false, "Model and effort picked per Codex session: row id to pick (JSON)")
        .never("Codex model picks", "The model and effort picked for each Codex session", "picked per session in Chat; it decides which model runs"),
    key("chatProjectFolders", K::StringList, D::Empty, false, "Recent folders for New Session (at most 30)")
        .never("Recent folders", "Recent folders New Session offers", "the app's own history of where sessions start"),
    key("newSessionChoice", K::String, D::None, false, "The account last picked in New Session")
        .never("New Session account", "The account New Session picked last", "it decides which login a new session runs under"),
    key("chatOpen", K::Bool, D::Bool(false), false, "The Chat window was open at quit")
        .never("Chat open at quit", "The Chat window was open at quit", "the app's own record"),
    key("chatSelection", K::String, D::None, false, "The session selected in Chat")
        .never("Selected session", "The session selected in Chat", "the app's own record"),
    key("chatWindowFrame", K::Json, D::None, false, "Chat window's last frame: display UUID, then place and size on that display (JSON)")
        .never("Chat window frame", "Where the Chat window sat last: the display and its place and size on it", "the app's own record"),
    key("chatPanels", K::Json, D::None, false, "Chat's Sidebar and Details Panel: width as a ratio of the window (absent: default), and whether each is hidden (JSON)")
        .never("Chat panel sizes", "How wide the Sidebar and the Details Panel are, and whether each is hidden", "the app's own record"),
    key("chatPageWidth", K::Double, D::Double(520.0), false, "Width of the page/file panel beside a conversation")
        .free("Page panel width", "Width in points of the page or file panel beside a conversation", "a display preference"),
    choice("chatHistory", "4mb", &["2mb", "4mb", "8mb", "16mb"], false, "How much of a conversation Chat loads: the newest 2, 4, 8 or 16 MB of its file")
        .free("Conversation history to load", "How much of a long conversation Chat loads: the newest 2, 4, 8 or 16 MB of its file", "a display and memory preference"),
    choice("appearance", "system", &["system", "light", "dark"], false, "How Notchling's windows look: System follows macOS, Light and Dark force it")
        .free("Appearance", "How Notchling's windows look: system follows macOS, light or dark forces it", "a display preference"),
    key("searchShortcut", K::String, D::Str("ctrl+alt+p"), false, "The global key that opens Search from any app: modifiers and a key joined by +, such as ctrl+alt+p, or off")
        .ask("Search shortcut", "The global key that opens Search from any app: modifiers and a key joined by +, such as ctrl+alt+p, or off", "it takes a key from every other app"),
    key("vaultReminderSent", K::Dict, D::Empty, false, "Vault expiry reminders already sent: secret to days")
        .never("Vault reminders sent", "Expiry reminders the vault already sent", "the vault's own record"),
    key("bridgeEnabled", K::Bool, D::Bool(false), false, "The phone bridge switch")
        .never("Phone Bridge (saved)", "The phone bridge switch as last saved", "a switch: only the user turns it on"),
    key("debugAccessCertsFile", K::String, D::None, false, "Debug builds: a file of Cloudflare Access certs (`-debugAccessCertsFile`)")
        .never("Access certificates file", "Debug builds: a file of Cloudflare Access certificates for the bridge", "security: it decides which certificates the bridge trusts"),
];

pub fn spec(name: &str) -> Option<&'static KeySpec> {
    KEYS.iter().find(|k| k.name == name)
}

pub fn awake_keys() -> impl Iterator<Item = &'static KeySpec> {
    KEYS.iter().filter(|k| k.from_awake)
}
