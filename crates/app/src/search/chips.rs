use std::collections::{BTreeMap, BTreeSet};

use notchling_engine::search::facets::FacetValue;
use notchling_engine::search::query::category::Category;
use notchling_engine::search::query::filter::Filter;
use notchling_engine::search::query::parse::{filter, Chip, Parsed};
use notchling_engine::search::query::when::When;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChipView {
    pub label: String,
    pub known: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Known {
    by_name: BTreeMap<String, Vec<FacetValue>>,
    capped: BTreeSet<String>,
    asked: BTreeSet<(String, String)>,
}

fn same(a: &str, b: &str) -> bool {
    a.to_lowercase() == b.to_lowercase()
}

fn alias<'a>(name: &str, typed: &'a str) -> &'a str {
    if name == "tool" && same(typed, "antigravity") {
        "agy"
    } else {
        typed
    }
}

fn parts(path: &str) -> Vec<&str> {
    path.split('/').filter(|p| !p.is_empty()).collect()
}

fn tail(path: &str, count: usize) -> String {
    let all = parts(path);
    all[all.len().saturating_sub(count)..].join("/")
}

fn path_has(value: &str, typed: &str) -> bool {
    let (have, want) = (parts(value), parts(typed));
    let fits = if typed.starts_with('/') {
        have.len() == want.len()
    } else {
        want.len() <= have.len()
    };
    !want.is_empty()
        && fits
        && have[have.len() - want.len()..]
            .iter()
            .zip(&want)
            .all(|(a, b)| same(a, b))
}

fn asking<'a>(name: &str, typed: &'a str) -> &'a str {
    let typed = alias(name, typed);
    if name == "project" && !typed.starts_with('/') {
        if let Some(last) = parts(typed).last() {
            return last;
        }
    }
    typed
}

impl Known {
    pub fn set(&mut self, name: &str, values: Vec<FacetValue>) {
        self.by_name.insert(name.to_owned(), values);
    }

    pub fn mark_capped(&mut self, name: &str) {
        self.capped.insert(name.to_owned());
    }

    pub fn mark_asked(&mut self, name: &str, typed: &str) {
        self.asked
            .insert((name.to_owned(), asking(name, typed).to_lowercase()));
    }

    fn settled(&self, name: &str, typed: &str) -> bool {
        !self.resolve(name, typed).is_empty()
            && (!self.capped.contains(name)
                || self
                    .asked
                    .contains(&(name.to_owned(), asking(name, typed).to_lowercase())))
    }

    pub fn has(&self, name: &str) -> bool {
        self.by_name.contains_key(name)
    }

    pub fn values(&self, name: &str) -> &[FacetValue] {
        self.by_name.get(name).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn add(&mut self, name: &str, more: Vec<FacetValue>) {
        let held = self.by_name.entry(name.to_owned()).or_default();
        for value in more {
            if !held.iter().any(|v| v.value == value.value) {
                held.push(value);
            }
        }
    }

    fn matching(&self, name: &str, typed: &str) -> Vec<&FacetValue> {
        let typed = alias(name, typed);
        self.values(name)
            .iter()
            .filter(|v| {
                same(&v.value, typed)
                    || same(&v.label, typed)
                    || (name == "project" && path_has(&v.value, typed))
            })
            .collect()
    }

    fn shown(&self, name: &str, found: &FacetValue) -> String {
        if name != "project" {
            return found.label.clone();
        }
        let rivals: Vec<&FacetValue> = self
            .values(name)
            .iter()
            .filter(|o| o.value != found.value && same(&o.label, &found.label))
            .collect();
        if rivals.is_empty() {
            return found.label.clone();
        }
        for count in 2..=parts(&found.value).len() {
            let mine = tail(&found.value, count);
            if rivals.iter().all(|o| !same(&tail(&o.value, count), &mine)) {
                return mine;
            }
        }
        found.value.clone()
    }

    fn chip_value(&self, name: &str, typed: &str, found: &FacetValue) -> String {
        if name == "project" && self.matching(name, typed).len() == 1 {
            self.shown(name, found)
        } else {
            value_label(name, found)
        }
    }

    pub fn resolve(&self, name: &str, typed: &str) -> Vec<String> {
        self.matching(name, typed)
            .into_iter()
            .map(|v| v.value.clone())
            .collect()
    }

    fn first(&self, name: &str, typed: &str) -> Option<&FacetValue> {
        let found = self.matching(name, typed);
        let typed = alias(name, typed);
        found
            .iter()
            .find(|v| same(&v.value, typed))
            .or(found.first())
            .copied()
    }
}

pub fn searchable(category: Category) -> bool {
    matches!(
        category,
        Category::Pages | Category::Vault | Category::Sessions
    )
}

pub fn corpora_for(name: &str) -> &'static [&'static str] {
    match name {
        "tool" | "account" => &["sessions"],
        "owner" => &["pages"],
        "project" => &["sessions", "pages"],
        _ => &[],
    }
}

fn facet_title(name: &str) -> &'static str {
    use notchling_locale::search as t;
    match name {
        "tool" => t::facet_tool(),
        "project" => t::facet_project(),
        "account" => t::facet_account(),
        "section" => t::facet_section(),
        "owner" => t::facet_owner(),
        "kind" => t::facet_kind(),
        _ => t::facet_state(),
    }
}

fn month_name(month: u32) -> &'static str {
    use notchling_locale::search as t;
    match month {
        1 => t::month_january(),
        2 => t::month_february(),
        3 => t::month_march(),
        4 => t::month_april(),
        5 => t::month_may(),
        6 => t::month_june(),
        7 => t::month_july(),
        8 => t::month_august(),
        9 => t::month_september(),
        10 => t::month_october(),
        11 => t::month_november(),
        _ => t::month_december(),
    }
}

fn tool_name(value: &str) -> Option<&'static str> {
    use notchling_locale::search as t;
    match value {
        "claude" => Some(t::tool_claude()),
        "codex" => Some(t::tool_codex()),
        "agy" => Some(t::tool_agy()),
        _ => None,
    }
}

fn value_label(name: &str, found: &FacetValue) -> String {
    match name {
        "tool" => tool_name(&found.value)
            .map(str::to_owned)
            .unwrap_or_else(|| found.label.clone()),
        _ => found.label.clone(),
    }
}

fn title_case(id: &str) -> String {
    let mut chars = id.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn category_title(category: Category) -> String {
    use notchling_locale::search as t;
    match category {
        Category::Pages => t::group_pages().to_owned(),
        Category::Vault => t::group_vault().to_owned(),
        Category::Sessions => t::group_sessions().to_owned(),
        other => title_case(other.id()),
    }
}

fn when_label(when: When) -> String {
    use notchling_locale::search as t;
    match when {
        When::Today => t::chip_today().to_owned(),
        When::Days(n) => t::chip_days(i64::from(n)),
        When::Month { year, month } => t::chip_month(month_name(month), &year.to_string()),
    }
}

pub fn chips(parsed: &Parsed, known: &Known) -> Vec<ChipView> {
    parsed
        .chips
        .iter()
        .map(|chip| match chip {
            Chip::Category(category) if searchable(*category) => ChipView {
                label: category_title(*category),
                known: true,
            },
            Chip::Category(category) => ChipView {
                label: notchling_locale::search::chip_not_searched(&category_title(*category)),
                known: false,
            },
            Chip::When(when) => ChipView {
                label: when_label(*when),
                known: true,
            },
            Chip::Facet { name, value } => match known.first(name, value) {
                Some(found) => ChipView {
                    label: format!(
                        "{}: {}",
                        facet_title(name),
                        known.chip_value(name, value, found)
                    ),
                    known: true,
                },
                None => ChipView {
                    label: notchling_locale::search::chip_unknown(facet_title(name)),
                    known: false,
                },
            },
        })
        .collect()
}

pub fn unresolved(parsed: &Parsed, known: &Known) -> Vec<(&'static str, String)> {
    let mut out: Vec<(&'static str, String)> = Vec::new();
    for chip in &parsed.chips {
        if let Chip::Facet { name, value } = chip {
            if corpora_for(name).is_empty() || known.settled(name, value) {
                continue;
            }
            let pair = (*name, asking(name, value).to_owned());
            if !out.contains(&pair) {
                out.push(pair);
            }
        }
    }
    out
}

pub fn wanted(parsed: &Parsed, known: &Known) -> Vec<&'static str> {
    let mut out: Vec<&'static str> = Vec::new();
    for chip in &parsed.chips {
        if let Chip::Facet { name, .. } = chip {
            if !known.has(name) && !out.contains(name) {
                out.push(name);
            }
        }
    }
    out
}

fn raws(list: &mut Option<Vec<String>>, name: &str, known: &Known) {
    if let Some(typed) = list.take() {
        let mut out: Vec<String> = Vec::new();
        for value in typed {
            for raw in known.resolve(name, &value) {
                if !out.contains(&raw) {
                    out.push(raw);
                }
            }
        }
        *list = (!out.is_empty()).then_some(out);
    }
}

pub fn sent_filter(parsed: &Parsed, known: &Known, now: f64, offset_secs: i64) -> Filter {
    let mut out = filter(parsed, now, offset_secs);
    raws(&mut out.tool, "tool", known);
    raws(&mut out.project, "project", known);
    raws(&mut out.account, "account", known);
    raws(&mut out.section, "section", known);
    raws(&mut out.owner, "owner", known);
    raws(&mut out.kind, "kind", known);
    raws(&mut out.state, "state", known);
    out
}

pub fn corpora(parsed: &Parsed, all: Vec<&'static str>) -> Vec<&'static str> {
    let category = parsed.chips.iter().find_map(|chip| match chip {
        Chip::Category(category) => Some(category.id()),
        _ => None,
    });
    match category {
        Some(id) => all.into_iter().filter(|c| *c == id).collect(),
        None => all,
    }
}

pub fn vault_allowed(parsed: &Parsed, sent: &Filter) -> bool {
    use notchling_engine::search::query::category::Category;
    sent.is_empty()
        && parsed.chips.iter().all(|chip| match chip {
            Chip::Category(category) => *category == Category::Vault,
            _ => true,
        })
}

fn facet_of(token: &str) -> Option<(&'static str, &str)> {
    let (head, partial) = token.split_once(':')?;
    let name = match head.to_lowercase().as_str() {
        "tool" => "tool",
        "project" | "in" => "project",
        "account" => "account",
        "section" => "section",
        "owner" => "owner",
        "kind" => "kind",
        "state" => "state",
        _ => return None,
    };
    Some((name, partial))
}

pub fn typing_partial(text: &str) -> Option<(&'static str, &str)> {
    let token = text.rsplit(char::is_whitespace).next()?;
    facet_of(token)
}

pub fn typing(text: &str) -> Option<&'static str> {
    typing_partial(text).map(|(name, _)| name)
}

fn common(words: &[String]) -> String {
    let Some(first) = words.first() else {
        return String::new();
    };
    let mut end = first.len();
    for word in &words[1..] {
        let shared = first
            .chars()
            .zip(word.chars())
            .take_while(|(a, b)| a.to_lowercase().eq(b.to_lowercase()))
            .map(|(a, _)| a.len_utf8())
            .sum::<usize>();
        end = end.min(shared);
    }
    first[..end].to_owned()
}

fn plain_done(known: &Known, name: &str, partial: &str) -> Option<String> {
    let folded = partial.to_lowercase();
    let mut options: Vec<String> = Vec::new();
    for v in known.values(name) {
        let shown = &v.value;
        if shown.chars().any(char::is_whitespace) {
            continue;
        }
        if shown.to_lowercase().starts_with(&folded) && !options.contains(shown) {
            options.push(shown.clone());
        }
    }
    if options.is_empty() {
        return None;
    }
    let mut done = common(&options);
    let single = options.len() == 1;
    if single && same(&done, partial) {
        return None;
    }
    if done.chars().count() <= partial.chars().count() && !single {
        return None;
    }
    if single {
        done.push(' ');
    }
    Some(done)
}

fn siblings<'a>(known: &'a Known, label: &str) -> Vec<(&'a FacetValue, String)> {
    known
        .values("project")
        .iter()
        .filter(|v| same(&v.label, label))
        .map(|v| (v, known.shown("project", v)))
        .filter(|(_, shown)| !shown.chars().any(char::is_whitespace))
        .collect()
}

fn next_sibling(known: &Known, current: &str) -> Option<String> {
    let found = known
        .values("project")
        .iter()
        .find(|v| same(&known.shown("project", v), current))?;
    let all = siblings(known, &found.label);
    let at = all.iter().position(|(v, _)| v.value == found.value)?;
    (all.len() > 1).then(|| all[(at + 1) % all.len()].1.clone())
}

fn project_done(known: &Known, partial: &str) -> Option<String> {
    let folded = partial.to_lowercase();
    let all: Vec<(String, &FacetValue)> = known
        .values("project")
        .iter()
        .map(|v| (known.shown("project", v), v))
        .filter(|(shown, _)| !shown.chars().any(char::is_whitespace))
        .collect();
    let qualified = |shown: &str, v: &FacetValue| shown != v.label;
    let by_path = all
        .iter()
        .any(|(shown, v)| qualified(shown, v) && shown.to_lowercase().starts_with(&folded));
    let mut texts: Vec<String> = Vec::new();
    let mut labels: Vec<&str> = Vec::new();
    for (shown, v) in &all {
        let by_label =
            !by_path && qualified(shown, v) && v.label.to_lowercase().starts_with(&folded);
        if (shown.to_lowercase().starts_with(&folded) || by_label) && !texts.contains(shown) {
            texts.push(shown.clone());
            labels.push(&v.label);
        }
    }
    match texts.as_slice() {
        [] => None,
        [one] if same(one, partial) => next_sibling(known, one),
        [one] => Some(format!("{one} ")),
        _ => {
            let shared = common(&texts);
            if shared.chars().count() > partial.chars().count() {
                return Some(shared);
            }
            let label = labels[0];
            if !labels.iter().all(|l| same(l, label)) {
                return None;
            }
            if same(label, partial) {
                Some(texts[0].clone())
            } else {
                label
                    .to_lowercase()
                    .starts_with(&partial.to_lowercase())
                    .then(|| label.to_owned())
            }
        }
    }
}

pub fn complete(text: &str, known: &Known) -> Option<String> {
    let start = text
        .rfind(char::is_whitespace)
        .map(|i| i + text[i..].chars().next().map_or(1, char::len_utf8))
        .unwrap_or(0);
    let token = &text[start..];
    let (name, partial) = facet_of(token)?;
    let colon = token.find(':')? + 1;
    let done = if name == "project" {
        project_done(known, partial)?
    } else {
        plain_done(known, name, partial)?
    };
    Some(format!("{}{}{}", &text[..start], &token[..colon], done))
}
