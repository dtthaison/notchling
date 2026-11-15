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
    let mut parts = segment
        .split('.')
        .map(|n| n.parse::<u64>().ok())
        .collect::<Option<Vec<u64>>>()?;
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    Some(parts)
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
    One(&'a str, bool),
}

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
