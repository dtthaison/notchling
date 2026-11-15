//! The user's `catalog.json` beside the daemon's socket: what Settings shows of it, and the
//! empty starter it creates so there is a file to open. JSON has no comments, so the starter
//! is `{}`, which the loaders read as "nothing overridden".

use std::fs;
use std::path::Path;

pub const STARTER: &str = "{}\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    Missing,
    /// A file with JSON that is an object (possibly empty).
    Found,
    /// A file the loaders skip: not JSON, or JSON that is not an object.
    Unreadable,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Missing => "missing",
            State::Found => "found",
            State::Unreadable => "unreadable",
        }
    }
}

pub fn state(path: &Path) -> State {
    match fs::read_to_string(path) {
        Err(_) if fs::symlink_metadata(path).is_err() => State::Missing,
        Err(_) => State::Unreadable,
        Ok(text) => match serde_json::from_str::<serde_json::Value>(&text) {
            Ok(v) if v.is_object() => State::Found,
            _ => State::Unreadable,
        },
    }
}

/// Makes the starter when nothing is at `path`; never touches what is there. True when it wrote.
pub fn ensure(path: &Path) -> Result<bool, String> {
    if fs::symlink_metadata(path).is_ok() {
        return Ok(false);
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("the folder: {e}"))?;
    }
    notchling_platform::files::write_private_no_follow(path, STARTER.as_bytes())
        .map_err(|e| e.to_string())?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(tag: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("nl-override-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_starter_is_made_once_and_reads_as_an_empty_override() {
        let d = dir("starter");
        let file = d.join("state").join("catalog.json");
        assert_eq!(state(&file), State::Missing);
        assert_eq!(ensure(&file), Ok(true));
        assert_eq!(fs::read_to_string(&file).unwrap(), STARTER);
        assert_eq!(state(&file), State::Found);
        assert_eq!(
            crate::catalog::roles::Roles::load(Some(&file)),
            crate::catalog::roles::Roles::built_in()
        );
        fs::write(&file, "{\"codex\": {}}").unwrap();
        assert_eq!(ensure(&file), Ok(false));
        assert_eq!(fs::read_to_string(&file).unwrap(), "{\"codex\": {}}");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_the_loaders_skip_is_called_unreadable_and_left_alone() {
        let d = dir("bad");
        fs::create_dir_all(&d).unwrap();
        let file = d.join("catalog.json");
        fs::write(&file, "{ not json").unwrap();
        assert_eq!(state(&file), State::Unreadable);
        fs::write(&file, "[1]").unwrap();
        assert_eq!(state(&file), State::Unreadable);
        assert_eq!(ensure(&file), Ok(false));
        assert_eq!(fs::read_to_string(&file).unwrap(), "[1]");
        let _ = fs::remove_dir_all(&d);
    }
}
