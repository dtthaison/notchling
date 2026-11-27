use crate::format::shipped;
use crate::state;

pub const LOCALE_ENV: &str = "NOTCHLING_LOCALE";
pub const MARK_FALLBACK_ENV: &str = "NOTCHLING_LOCALE_MARK_FALLBACK";
pub const PSEUDO: &str = "en-XA";

#[derive(Clone, Copy, Debug, Default)]
pub struct Inputs<'a> {
    pub env: Option<&'a str>,
    pub setting: Option<&'a str>,
    pub system: &'a [String],
    pub region: Option<&'a str>,
    pub mark_fallback: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Choice {
    pub tag: &'static str,
    pub pseudo: bool,
    pub region: Option<String>,
    pub mark_fallback: bool,
}

pub fn choose(inputs: &Inputs<'_>) -> Choice {
    choose_among(inputs, &shipped())
}

pub(crate) fn choose_among(inputs: &Inputs<'_>, shipped: &[&'static str]) -> Choice {
    let english = shipped.first().copied().unwrap_or("en");
    let mut pseudo = false;
    let mut tag = None;
    let mut matched_region = None;

    if let Some(env) = inputs.env.map(str::trim).filter(|v| !v.is_empty()) {
        if normalize(env).eq_ignore_ascii_case(PSEUDO) {
            pseudo = true;
            tag = Some(english);
        } else if let Some(found) = match_tag(env, shipped) {
            tag = Some(found);
            matched_region = region_of(env);
        }
    }
    if tag.is_none() {
        if let Some(setting) = inputs
            .setting
            .map(str::trim)
            .filter(|v| !v.is_empty() && !v.eq_ignore_ascii_case("system"))
        {
            if let Some(found) = match_tag(setting, shipped) {
                tag = Some(found);
                matched_region = region_of(setting);
            }
        }
    }
    if tag.is_none() {
        for system in inputs.system {
            if let Some(found) = match_tag(system, shipped) {
                tag = Some(found);
                matched_region = region_of(system);
                break;
            }
        }
    }
    let region = inputs
        .region
        .map(|r| r.trim().to_ascii_uppercase())
        .filter(|r| !r.is_empty())
        .or(matched_region);
    Choice {
        tag: tag.unwrap_or(english),
        pseudo,
        region,
        mark_fallback: inputs.mark_fallback,
    }
}

pub(crate) fn normalize(tag: &str) -> String {
    let cut = tag.split(['@', '.']).next().unwrap_or(tag);
    let cut = match cut.find("-u-").or_else(|| cut.find("_u_")) {
        Some(at) => &cut[..at],
        None => cut,
    };
    cut.replace('_', "-")
}

fn parts(tag: &str) -> Vec<String> {
    normalize(tag)
        .split('-')
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

pub(crate) fn region_of(tag: &str) -> Option<String> {
    parts(tag)
        .into_iter()
        .skip(1)
        .find(|p| {
            (p.len() == 2 && p.chars().all(|c| c.is_ascii_alphabetic()))
                || (p.len() == 3 && p.chars().all(|c| c.is_ascii_digit()))
        })
        .map(|p| p.to_ascii_uppercase())
}

fn find(shipped: &[&'static str], wanted: &str) -> Option<&'static str> {
    shipped
        .iter()
        .copied()
        .find(|s| s.eq_ignore_ascii_case(wanted))
}

pub(crate) fn match_tag(tag: &str, shipped: &[&'static str]) -> Option<&'static str> {
    let parts = parts(tag);
    let language = parts.first()?;
    let full = parts.join("-");
    if let Some(found) = find(shipped, &full) {
        return Some(found);
    }
    let script = parts
        .get(1)
        .filter(|p| p.len() == 4 && p.chars().all(|c| c.is_ascii_alphabetic()));
    if let Some(script) = script {
        if let Some(found) = find(shipped, &format!("{language}-{script}")) {
            return Some(found);
        }
    }
    find(shipped, language)
}

pub fn start(system: &[String], region: Option<&str>, setting: Option<&str>) -> Choice {
    let env = std::env::var(LOCALE_ENV).ok();
    let mark = std::env::var(MARK_FALLBACK_ENV)
        .map(|v| v == "1")
        .unwrap_or(false);
    let choice = choose(&Inputs {
        env: env.as_deref(),
        setting,
        system,
        region,
        mark_fallback: mark,
    });
    apply(&choice);
    choice
}

pub fn apply(choice: &Choice) -> bool {
    state::set(choice)
}

pub fn active() -> &'static str {
    state::current().tag()
}
