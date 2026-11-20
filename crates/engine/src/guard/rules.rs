use super::event::Event;
use super::shell::{has_glob_or_var, is_flag, loops_on_sleep, program, seconds, words};
use std::path::{Path, PathBuf};

pub const READ_BYTES: u64 = 60_000;
pub const SLEEP_SECS: f64 = 25.0;
pub const SIZE_CALLS: u64 = 80;
pub const BUILDER_CAP: u64 = 5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Rule {
    Model,
    Review,
    Cap,
    Read,
    Tree,
    Poll,
    Size,
}

impl Rule {
    pub const ALL: [Rule; 7] = [
        Rule::Model,
        Rule::Review,
        Rule::Cap,
        Rule::Read,
        Rule::Tree,
        Rule::Poll,
        Rule::Size,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Rule::Model => "model",
            Rule::Review => "review",
            Rule::Cap => "cap",
            Rule::Read => "read",
            Rule::Tree => "tree",
            Rule::Poll => "poll",
            Rule::Size => "size",
        }
    }

    pub fn parse(name: &str) -> Option<Rule> {
        Rule::ALL.into_iter().find(|r| r.name() == name)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Note,
    Correct,
    Block,
}

impl Action {
    pub fn name(self) -> &'static str {
        match self {
            Action::Note => "note",
            Action::Correct => "correct",
            Action::Block => "block",
        }
    }

    pub fn parse(name: &str) -> Option<Action> {
        [Action::Note, Action::Correct, Action::Block]
            .into_iter()
            .find(|a| a.name() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Would {
    pub rule: Rule,
    pub action: Action,
    pub reason: &'static str,
}

fn would(rule: Rule, action: Action, reason: &'static str) -> Would {
    Would {
        rule,
        action,
        reason,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    pub read_bytes: u64,
    pub sleep_secs: f64,
    pub size_calls: u64,
    pub cap: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            read_bytes: READ_BYTES,
            sleep_secs: SLEEP_SECS,
            size_calls: SIZE_CALLS,
            cap: BUILDER_CAP,
        }
    }
}

const ROLES: [&str; 8] = [
    "review-secrets",
    "review",
    "read",
    "code",
    "fix",
    "judge",
    "hardest",
    "image",
];

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tags {
    pub role: Option<&'static str>,
    pub up: bool,
}

pub fn tags(description: &str) -> Tags {
    let mut found = Tags::default();
    let mut rest = description;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(']') else {
            break;
        };
        let inside = after[..close].trim().to_lowercase();
        rest = &after[close + 1..];
        if inside.contains("up:") {
            found.up = true;
        }
        let first = inside.split_whitespace().next().unwrap_or("");
        if found.role.is_none() {
            found.role = ROLES.iter().copied().find(|r| *r == first);
        }
    }
    found
}

fn premium(model: &str) -> bool {
    let model = model.to_lowercase();
    model.contains("opus") || model.contains("fable")
}

fn named_type(event: &Event) -> String {
    event
        .text("subagent_type")
        .unwrap_or("")
        .trim()
        .to_lowercase()
}

pub fn spawn_rules(event: &Event) -> Vec<Would> {
    let mut out = Vec::new();
    if !event.spawns() {
        return out;
    }
    let tag = tags(event.text("description").unwrap_or(""));
    let kind = named_type(event);
    if tag.role == Some("review") || kind.contains("review") {
        out.push(would(Rule::Review, Action::Correct, "review-spawn"));
    }
    let model = event.text("model").unwrap_or("");
    let fork = kind == "fork";
    let routine = matches!(tag.role, Some("code" | "read" | "fix"));
    let exempt = tag.up || matches!(tag.role, Some("judge" | "hardest" | "review-secrets"));
    let reviewing = tag.role == Some("review") || kind.contains("review");
    let names_premium = premium(model);
    let inherits_premium = fork && model.is_empty() && routine;
    if (names_premium || inherits_premium) && !exempt && !reviewing {
        let untagged = tag.role.is_none();
        if routine || untagged {
            let action = if kind.starts_with("notchling-") {
                Action::Correct
            } else {
                Action::Block
            };
            let reason = if untagged {
                "premium-untagged"
            } else {
                "premium-routine"
            };
            out.push(would(Rule::Model, action, reason));
        }
    }
    out
}

fn resolve(path: &str, cwd: Option<&str>) -> Option<PathBuf> {
    let path = Path::new(path);
    if path.is_absolute() {
        return Some(path.to_path_buf());
    }
    cwd.map(|c| Path::new(c).join(path))
}

const MEDIA: [&str; 10] = [
    "png", "jpg", "jpeg", "gif", "webp", "bmp", "heic", "pdf", "svg", "ico",
];

fn is_media(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| MEDIA.contains(&e.to_lowercase().as_str()))
}

pub fn read_tool(
    event: &Event,
    limits: &Limits,
    size_of: &dyn Fn(&Path) -> Option<u64>,
) -> Option<Would> {
    if event.tool != "Read" {
        return None;
    }
    let file = event.text("file_path")?;
    let bounded = event.input.get("limit").is_some_and(|v| !v.is_null())
        || event.input.get("pages").is_some_and(|v| !v.is_null());
    if bounded || is_media(file) {
        return None;
    }
    let path = resolve(file, event.cwd.as_deref())?;
    (size_of(&path)? > limits.read_bytes)
        .then(|| would(Rule::Read, Action::Note, "read-whole-file"))
}

fn cat_target(words: &[String]) -> Option<&str> {
    if program(words.first()?) != "cat" {
        return None;
    }
    let mut files = words[1..].iter().filter(|w| !is_flag(w));
    let file = files.next()?;
    if files.next().is_some() || words[1..].iter().any(|w| w == "-" || w == "--") {
        return None;
    }
    (!has_glob_or_var(file)).then_some(file.as_str())
}

fn tree_dump(words: &[String]) -> bool {
    let args = &words[1..];
    match program(&words[0]) {
        "ls" => args.iter().any(|w| {
            w == "--recursive" || (w.starts_with('-') && !w.starts_with("--") && w.contains('R'))
        }),
        "tree" => !args.iter().any(|w| w == "-L"),
        "find" => {
            let side_effects = ["-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint"];
            !args
                .iter()
                .any(|w| w == "-maxdepth" || side_effects.contains(&w.as_str()))
        }
        "git" => {
            args.first().is_some_and(|w| w == "ls-files") && args.iter().skip(1).all(|w| is_flag(w))
        }
        _ => false,
    }
}

fn long_sleep(words: &[String], limits: &Limits) -> bool {
    program(&words[0]) == "sleep"
        && words.len() == 2
        && seconds(&words[1]).is_some_and(|s| s >= limits.sleep_secs)
}

pub fn bash_rules(
    event: &Event,
    limits: &Limits,
    size_of: &dyn Fn(&Path) -> Option<u64>,
) -> Vec<Would> {
    let mut out = Vec::new();
    if event.tool != "Bash" {
        return out;
    }
    let Some(command) = event.text("command") else {
        return out;
    };
    if loops_on_sleep(command) {
        out.push(would(Rule::Poll, Action::Block, "sleep-loop"));
        return out;
    }
    let Some(words) = words(command) else {
        return out;
    };
    if long_sleep(&words, limits) {
        out.push(would(Rule::Poll, Action::Block, "long-sleep"));
    } else if tree_dump(&words) {
        out.push(would(Rule::Tree, Action::Note, "tree-dump"));
    } else if let Some(file) = cat_target(&words) {
        let big = resolve(file, event.cwd.as_deref())
            .and_then(|p| size_of(&p))
            .is_some_and(|n| n > limits.read_bytes);
        if big {
            out.push(would(Rule::Read, Action::Note, "cat-whole-file"));
        }
    }
    out
}

pub fn stateless(
    event: &Event,
    limits: &Limits,
    size_of: &dyn Fn(&Path) -> Option<u64>,
) -> Vec<Would> {
    let mut out = spawn_rules(event);
    out.extend(bash_rules(event, limits, size_of));
    out.extend(read_tool(event, limits, size_of));
    out
}
