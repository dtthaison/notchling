//! The Swift app's UserDefaults keys, enumerated from `legacy/macos/Sources` (every `forKey:` and
//! `@AppStorage`, the keys held in constants, and the `migrateFromAwake` list).

use serde_json::{json, Value};

/// How a value is stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Bool,
    Int,
    Double,
    String,
    /// An array of strings.
    StringList,
    /// A dictionary (`[String: Double]` and so on).
    Dict,
    /// Foundation `Data` holding JSON (`JSONEncoder` output). Exports carry it as base64 text; the
    /// migration decodes it to the JSON value.
    Json,
    /// Not read anywhere in the code; kept as given.
    Any,
}

/// What a missing key reads as.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fallback {
    /// Unset reads as nothing.
    None,
    Bool(bool),
    Int(i64),
    Double(f64),
    Str(&'static str),
    /// An empty list or dictionary, by [`Kind`].
    Empty,
}

/// One setting.
#[derive(Clone, Copy, Debug)]
pub struct KeySpec {
    pub name: &'static str,
    pub kind: Kind,
    pub default: Fallback,
    /// Allowed strings, when the key is an enum; empty means any.
    pub allowed: &'static [&'static str],
    /// True when `AppDelegate.migrateFromAwake` copies it from the old `local.awake.menubar` domain.
    pub from_awake: bool,
    pub doc: &'static str,
}

impl KeySpec {
    /// The value an unset key reads as.
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
    /// True when `value` is exactly what this key stores: the right JSON type and, for an enum, one of
    /// the allowed strings.
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
    }
}

use Fallback as D;
use Kind as K;

/// Every key the Swift app reads or writes (36). The spec's "30" was an estimate before the
/// enumeration; the code has 24 migrated keys, `migratedFromAwake` and 11 more.
pub const KEYS: &[KeySpec] = &[
    // Migrated from the old Awake domain (the `migrateFromAwake` list, in its order).
    choice("mode", "off", &["off", "on", "whileWorking"], true, "Keep Mac Awake: off, always on, or while an agent works"),
    key("active", K::Bool, D::Bool(false), true, "Awake v1's on/off flag; read only when `mode` is unset, so the migration folds it into `mode`"),
    key("keepDisplayOn", K::Bool, D::Bool(false), true, "Keep the display on too, not only the system"),
    key("continueAfterSleep", K::Bool, D::Bool(true), true, "Resume background sessions after the Mac wakes (WakeResumer)"),
    key("hideNotch", K::Bool, D::Bool(false), true, "Hide the notch (the menu's Show Notch is its inverse)"),
    key("answerFromNotch", K::Bool, D::Bool(true), true, "Answer questions and permissions from the notch; first launch turns it on"),
    key("keepPages", K::Bool, D::Bool(true), true, "Keep local copies of published artifacts (the page hook)"),
    key("showPet", K::Bool, D::Bool(true), true, "The animated pet in the notch"),
    key("replyFromNotch", K::Bool, D::Bool(true), true, "A reply box in the notch"),
    key("showLastReply", K::Bool, D::Bool(true), true, "Show the last reply in the notch"),
    key("askedAccessibility", K::Bool, D::Bool(false), true, "The Accessibility prompt was shown once (TabPerch)"),
    choice("notchWidth", "medium", &["small", "medium", "large"], true, "Width of the notch's panel"),
    key("dismissed", K::Dict, D::Empty, true, "Done rows cleared in the notch: agent id to time (seconds)"),
    key("hiddenAgents", K::Dict, D::Empty, true, "Agents hidden in the notch: agent id to the group they were hidden in"),
    key("accounts", K::Json, D::None, true, "Saved Claude Code accounts (JSON of AccountStore's list)"),
    key("untrackedCostAccount", K::Any, D::None, true, "Only in the migration list; nothing in the code reads it"),
    key("antigravityAccounts", K::Json, D::None, true, "Saved Antigravity accounts (JSON)"),
    key("claudeDesktopAccounts", K::StringList, D::Empty, true, "Claude Desktop accounts signed in once (account UUIDs)"),
    key("codexAccounts", K::Json, D::None, true, "Saved Codex accounts (JSON)"),
    choice("limitMode", "notifyAndSwitch", &["off", "notify", "notifyAndSwitch"], true, "Claude Code limit watching"),
    key("limitSwitchAt", K::Int, D::Int(95), true, "Percent at which a limit switch happens"),
    choice("codexLimitMode", "notify", &["off", "notify", "notifyAndSwitch"], true, "Codex limit watching"),
    choice("antigravityLimitMode", "notify", &["off", "notify"], true, "Antigravity limit watching (never switches)"),
    key("codexNetwork", K::Bool, D::Bool(true), true, "Open the network in Codex turns (Allow Network in Codex Turns)"),
    // Not migrated.
    key("migratedFromAwake", K::Bool, D::Bool(false), false, "The Awake migration already ran"),
    key("chatClosedSessions", K::Dict, D::Empty, false, "Sessions closed in Chat: row id to time (seconds)"),
    key("chatInspector", K::Bool, D::Bool(false), false, "The Details panel is open"),
    key("codexPicks", K::Json, D::None, false, "Model and effort picked per Codex session: row id to pick (JSON)"),
    key("chatProjectFolders", K::StringList, D::Empty, false, "Recent folders for New Session (at most 30)"),
    key("newSessionChoice", K::String, D::None, false, "The account last picked in New Session"),
    key("chatOpen", K::Bool, D::Bool(false), false, "The Chat window was open at quit"),
    key("chatSelection", K::String, D::None, false, "The session selected in Chat"),
    key("chatPageWidth", K::Double, D::Double(520.0), false, "Width of the page/file panel beside a conversation"),
    key("vaultReminderSent", K::Dict, D::Empty, false, "Vault expiry reminders already sent: secret to days"),
    key("bridgeEnabled", K::Bool, D::Bool(false), false, "The phone bridge switch"),
    key("debugAccessCertsFile", K::String, D::None, false, "Debug builds: a file of Cloudflare Access certs (`-debugAccessCertsFile`)"),
];

/// The spec of a key, by name.
pub fn spec(name: &str) -> Option<&'static KeySpec> {
    KEYS.iter().find(|k| k.name == name)
}

/// The keys `migrateFromAwake` copies, in the Swift list's order.
pub fn awake_keys() -> impl Iterator<Item = &'static KeySpec> {
    KEYS.iter().filter(|k| k.from_awake)
}
