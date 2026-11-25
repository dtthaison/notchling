use super::category::Category;
use super::filter::Filter;
use super::fold::words;
use super::when::{bounds, parse_when, When};

#[derive(Clone, Debug, PartialEq)]
pub enum Chip {
    Category(Category),
    Facet { name: &'static str, value: String },
    When(When),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Parsed {
    pub chips: Vec<Chip>,
    pub words: Vec<String>,
    pub rest: String,
}

impl Chip {
    pub fn token(&self) -> String {
        match self {
            Chip::Category(c) => format!("category:{}", c.id()),
            Chip::Facet { name, value } => format!("{name}:{value}"),
            Chip::When(When::Today) => "when:today".into(),
            Chip::When(When::Days(n)) => format!("when:{n}d"),
            Chip::When(When::Month { year, month }) => format!("when:{year:04}-{month:02}"),
        }
    }
}

const PREFIXES: [(&str, Category); 15] = [
    ("s:", Category::Settings),
    ("settings:", Category::Settings),
    ("p:", Category::Pages),
    ("pages:", Category::Pages),
    ("c:", Category::Sessions),
    ("sessions:", Category::Sessions),
    ("i:", Category::Inbox),
    ("inbox:", Category::Inbox),
    ("a:", Category::Actions),
    ("actions:", Category::Actions),
    ("v:", Category::Vault),
    ("vault:", Category::Vault),
    ("m:", Category::Machines),
    ("machines:", Category::Machines),
    ("activity:", Category::Activity),
];

const FACETS: [(&str, &str); 8] = [
    ("tool", "tool"),
    ("project", "project"),
    ("in", "project"),
    ("account", "account"),
    ("section", "section"),
    ("owner", "owner"),
    ("kind", "kind"),
    ("state", "state"),
];

fn facet_chip(token: &str) -> Option<Chip> {
    let (name, value) = token.split_once(':')?;
    if value.is_empty() {
        return None;
    }
    let name = name.to_ascii_lowercase();
    if name == "when" {
        return parse_when(value).map(Chip::When);
    }
    let (_, canonical) = FACETS.iter().find(|(n, _)| *n == name)?;
    let value = match *canonical {
        "tool" | "kind" | "state" | "section" => value.to_ascii_lowercase(),
        _ => value.to_owned(),
    };
    Some(Chip::Facet {
        name: canonical,
        value,
    })
}

fn category_prefix(token: &str) -> Option<(Category, &str)> {
    if let Some(rest) = token.strip_prefix('>') {
        return Some((Category::Actions, rest));
    }
    let lower = token.to_ascii_lowercase();
    PREFIXES
        .iter()
        .find(|(p, _)| *p == lower)
        .map(|(_, c)| (*c, ""))
}

pub fn parse(text: &str) -> Parsed {
    let mut chips = Vec::new();
    let mut kept: Vec<&str> = Vec::new();
    let mut has_category = false;
    for token in text.split_whitespace() {
        if let Some(chip) = facet_chip(token) {
            chips.push(chip);
            continue;
        }
        if !has_category {
            if let Some((category, tail)) = category_prefix(token) {
                has_category = true;
                chips.push(Chip::Category(category));
                if !tail.is_empty() {
                    kept.push(tail);
                }
                continue;
            }
        }
        kept.push(token);
    }
    let rest = kept.join(" ");
    Parsed {
        chips,
        words: words(&rest),
        rest,
    }
}

fn push(slot: &mut Option<Vec<String>>, value: &str) {
    let list = slot.get_or_insert_with(Vec::new);
    if !list.iter().any(|v| v == value) {
        list.push(value.to_owned());
    }
}

pub fn filter(parsed: &Parsed, now: f64, offset_secs: i64) -> Filter {
    let mut out = Filter::default();
    for chip in &parsed.chips {
        match chip {
            Chip::Facet { name, value } => match *name {
                "tool" => push(&mut out.tool, value),
                "project" => push(&mut out.project, value),
                "account" => push(&mut out.account, value),
                "section" => push(&mut out.section, value),
                "owner" => push(&mut out.owner, value),
                "kind" => push(&mut out.kind, value),
                _ => push(&mut out.state, value),
            },
            Chip::When(when) if out.after.is_none() => {
                let (after, before) = bounds(*when, now, offset_secs);
                out.after = Some(after);
                out.before = (before < now).then_some(before);
            }
            _ => {}
        }
    }
    out
}
