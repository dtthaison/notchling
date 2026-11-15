//! A model's family: the codename word (hydrogen, argon, sol, flash) with what comes
//! before the version and what follows the codename. `gemini-4-hydrogen` and
//! `gemini-4.1-hydrogen` are one family; `gemini-4-hydrogen-xhigh` is another variant.

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Family {
    pub prefix: String,
    pub codename: String,
    pub variant: String,
    pub version: Vec<u64>,
}

fn version_of(segment: &str) -> Option<Vec<u64>> {
    if !segment.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    segment.split('.').map(|n| n.parse::<u64>().ok()).collect()
}

pub fn parse(id: &str) -> Option<Family> {
    let parts: Vec<&str> = id.split('-').collect();
    let at = parts.iter().position(|p| version_of(p).is_some())?;
    let codename = parts.get(at + 1)?;
    if !codename.starts_with(|c: char| c.is_ascii_lowercase())
        || !codename.chars().all(|c| c.is_ascii_lowercase())
    {
        return None;
    }
    Some(Family {
        prefix: parts[..at].join("-"),
        codename: codename.to_string(),
        variant: parts[at + 2..].join("-"),
        version: version_of(parts[at])?,
    })
}

impl Family {
    pub fn same(&self, other: &Family) -> bool {
        (&self.prefix, &self.codename, &self.variant)
            == (&other.prefix, &other.codename, &other.variant)
    }
}

pub enum Newest<'a> {
    Nothing,
    /// The newest live model of the family, and whether it is newer than the marked one.
    One(&'a str, bool),
}

/// The newest live model of `marked`'s family other than `marked`, skipping `skip`ped ones.
pub fn newest<'a>(marked: &str, live: &'a [String], skip: impl Fn(&str) -> bool) -> Newest<'a> {
    let Some(family) = parse(marked) else {
        return Newest::Nothing;
    };
    let mut best: Option<(&str, Vec<u64>)> = None;
    for id in live.iter().filter(|id| *id != marked && !skip(id)) {
        let Some(f) = parse(id).filter(|f| f.same(&family)) else {
            continue;
        };
        if best.as_ref().is_none_or(|(_, v)| f.version > *v) {
            best = Some((id, f.version));
        }
    }
    match best {
        None => Newest::Nothing,
        Some((id, v)) => Newest::One(id, v > family.version),
    }
}

#[cfg(test)]
#[path = "family_tests.rs"]
mod tests;
