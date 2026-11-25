use crate::Result;

#[cfg(target_os = "macos")]
pub fn preferred() -> Result<Vec<String>> {
    use objc2_foundation::NSLocale;
    let list = NSLocale::preferredLanguages();
    Ok((0..list.count())
        .map(|i| list.objectAtIndex(i).to_string())
        .collect())
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn preferred() -> Result<Vec<String>> {
    Ok(from_env(&|key| std::env::var(key).ok()))
}

#[cfg(not(unix))]
pub fn preferred() -> Result<Vec<String>> {
    Err(crate::Error::Unsupported("reading the preferred languages"))
}

#[cfg(target_os = "macos")]
pub fn region() -> Option<String> {
    use objc2_foundation::NSLocale;
    let identifier = NSLocale::currentLocale().localeIdentifier().to_string();
    region_of(&posix_tag(&identifier)?)
}

#[cfg(not(target_os = "macos"))]
pub fn region() -> Option<String> {
    preferred().ok()?.first().and_then(|tag| region_of(tag))
}

pub fn from_env(var: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |tag: Option<String>| {
        if let Some(tag) = tag {
            if !out.contains(&tag) {
                out.push(tag);
            }
        }
    };
    if let Some(list) = var("LANGUAGE") {
        for item in list.split(':') {
            add(posix_tag(item));
        }
    }
    for key in ["LC_ALL", "LC_MESSAGES", "LANG"] {
        if let Some(value) = var(key) {
            add(posix_tag(&value));
        }
    }
    out
}

pub fn posix_tag(value: &str) -> Option<String> {
    let head = value.split(['.', '@']).next().unwrap_or("").trim();
    if head.is_empty() || head == "C" || head == "POSIX" {
        return None;
    }
    let mut parts = head.split(['_', '-']);
    let language = parts.next()?;
    if !(2..=3).contains(&language.len()) || !language.chars().all(|c| c.is_ascii_alphabetic()) {
        return None;
    }
    let mut tag = language.to_ascii_lowercase();
    for part in parts {
        if part.is_empty() || !part.chars().all(|c| c.is_ascii_alphanumeric()) {
            return None;
        }
        tag.push('-');
        match part.len() {
            2 => tag.push_str(&part.to_ascii_uppercase()),
            4 => {
                tag.push_str(&part[..1].to_ascii_uppercase());
                tag.push_str(&part[1..].to_ascii_lowercase());
            }
            _ => tag.push_str(part),
        }
    }
    Some(tag)
}

pub fn region_of(tag: &str) -> Option<String> {
    tag.split(['-', '_']).skip(1).find_map(|part| {
        let region = (part.len() == 2 && part.chars().all(|c| c.is_ascii_alphabetic()))
            || (part.len() == 3 && part.chars().all(|c| c.is_ascii_digit()));
        region.then(|| part.to_ascii_uppercase())
    })
}

#[cfg(test)]
#[path = "locale_tests.rs"]
mod locale_tests;
