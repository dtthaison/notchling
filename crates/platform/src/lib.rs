#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
mod app_procs;
pub mod apps;
pub mod awake;
pub mod files;
pub mod locale;
pub mod notify;
pub mod paths;
#[cfg(target_os = "macos")]
mod sys_mac;
pub mod watch;

use std::fmt;
use std::io;

#[derive(Debug)]
pub enum Error {
    Unsupported(&'static str),
    System(String),
    Io(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Unsupported(what) => write!(f, "{what} is not supported on this system yet"),
            Error::System(what) => write!(f, "{what}"),
            Error::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn crate_name() -> &'static str {
    "platform"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_itself() {
        assert_eq!(crate_name(), "platform");
    }

    #[test]
    fn unsupported_says_so() {
        let text = Error::Unsupported("keeping awake").to_string();
        assert!(text.contains("keeping awake") && text.contains("not supported"));
    }
}
