//! The `notchling` command line: its subcommands are modes (`daemon`) and clients (the rest).

use std::path::PathBuf;

/// What `notchling service` does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    /// Writes the service file and prints the command that loads it.
    Install,
    /// Removes the service file and prints the command that unloads it.
    Uninstall,
    /// Prints the service file.
    Print,
}

/// A parsed command line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Daemon {
        socket: Option<PathBuf>,
    },
    Status {
        socket: Option<PathBuf>,
    },
    Sessions {
        json: bool,
        socket: Option<PathBuf>,
    },
    Service {
        action: ServiceAction,
        dir: Option<PathBuf>,
    },
    /// Claude Code's hook: stdin in, the answer (or nothing) out, always exit 0.
    Hook {
        socket: Option<PathBuf>,
    },
    /// `notchling pages [words]`: the saved Artifact pages, one line each.
    Pages {
        query: String,
        socket: Option<PathBuf>,
    },
    /// `notchling page <link|id|words>`: where a saved page is and who can republish it.
    Page {
        query: String,
        socket: Option<PathBuf>,
    },
    Version,
    Help,
}

/// `notchling <version>`.
pub fn version_line() -> String {
    format!("notchling {}", env!("CARGO_PKG_VERSION"))
}

pub fn usage() -> &'static str {
    "usage:\n  \
     notchling daemon [--socket <path>]          serve the local API\n  \
     notchling status [--socket <path>]          the daemon's version, pid, uptime and clients\n  \
     notchling sessions [--json] [--socket <path>]  the sessions the daemon sees\n  \
     notchling pages [words] [--socket <path>]   the saved Artifact pages (id, title, account), newest first\n  \
     notchling page <link|id|words> [--socket <path>]  where a saved page is, and who can republish it\n  \
     notchling service install|uninstall|print [--dir <folder>]\n  \
     notchling hook [--socket <path>]            Claude Code's hook: asks the daemon, passes on any trouble\n  \
     notchling --version | --help\n\n\
     NOTCHLING_DATA_ROOT=<dir> reads <dir>/claude and <dir>/notchling instead of your own folders.\n\
     service install writes the file and prints the command that loads it; it never loads it."
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    match args.next() {
        Some(v) if !v.is_empty() && !v.starts_with("--") => Ok(PathBuf::from(v)),
        _ => Err(format!("{flag} needs a value")),
    }
}

/// Reads the arguments after the program name.
pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Help);
    };
    match first.as_str() {
        "--version" | "-V" | "version" => return Ok(Command::Version),
        "--help" | "-h" | "help" => return Ok(Command::Help),
        _ => {}
    }
    let action = if first == "service" {
        Some(match args.next().as_deref() {
            Some("install") => ServiceAction::Install,
            Some("uninstall") => ServiceAction::Uninstall,
            Some("print") => ServiceAction::Print,
            Some(other) => return Err(format!("unknown service action: {other}")),
            None => return Err("service needs install, uninstall or print".into()),
        })
    } else {
        None
    };
    let mut socket = None;
    let mut dir = None;
    let mut json = false;
    let mut words: Vec<String> = Vec::new();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            word if (first == "pages" || first == "page") && !word.starts_with("--") => {
                words.push(arg)
            }
            "--socket" if action.is_none() => socket = Some(value(&mut args, "--socket")?),
            "--dir" if action.is_some() => dir = Some(value(&mut args, "--dir")?),
            "--json" if first == "sessions" => json = true,
            other => return Err(format!("unknown argument for {first}: {other}")),
        }
    }
    match (first.as_str(), action) {
        ("daemon", _) => Ok(Command::Daemon { socket }),
        ("status", _) => Ok(Command::Status { socket }),
        ("sessions", _) => Ok(Command::Sessions { json, socket }),
        ("hook", _) => Ok(Command::Hook { socket }),
        ("pages", _) => Ok(Command::Pages {
            query: words.join(" "),
            socket,
        }),
        ("page", _) if words.is_empty() => {
            Err("page needs a link, an id or words from the title".into())
        }
        ("page", _) => Ok(Command::Page {
            query: words.join(" "),
            socket,
        }),
        ("service", Some(action)) => Ok(Command::Service { action, dir }),
        (other, _) => Err(format!("unknown command: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn p(args: &[&str]) -> Result<Command, String> {
        parse(args.iter().map(|s| s.to_string()))
    }

    #[test]
    fn modes_and_clients() {
        assert_eq!(p(&["daemon"]), Ok(Command::Daemon { socket: None }));
        assert_eq!(
            p(&["daemon", "--socket", "/tmp/s.sock"]),
            Ok(Command::Daemon {
                socket: Some("/tmp/s.sock".into())
            })
        );
        assert_eq!(p(&["status"]), Ok(Command::Status { socket: None }));
        assert_eq!(
            p(&["sessions", "--json"]),
            Ok(Command::Sessions {
                json: true,
                socket: None
            })
        );
        assert_eq!(p(&[]), Ok(Command::Help));
        assert_eq!(p(&["--version"]), Ok(Command::Version));
    }

    #[test]
    fn the_hook_takes_a_socket_only() {
        assert_eq!(p(&["hook"]), Ok(Command::Hook { socket: None }));
        assert_eq!(
            p(&["hook", "--socket", "/tmp/s.sock"]),
            Ok(Command::Hook {
                socket: Some("/tmp/s.sock".into())
            })
        );
        assert!(p(&["hook", "--json"]).is_err());
        assert!(usage().contains("notchling hook"));
    }

    #[test]
    fn pages_and_page_take_words() {
        assert_eq!(
            p(&["pages"]),
            Ok(Command::Pages {
                query: String::new(),
                socket: None
            })
        );
        assert_eq!(
            p(&["pages", "cost", "report", "--socket", "/tmp/s.sock"]),
            Ok(Command::Pages {
                query: "cost report".into(),
                socket: Some("/tmp/s.sock".into())
            })
        );
        assert_eq!(
            p(&["page", "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv"]),
            Ok(Command::Page {
                query: "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv".into(),
                socket: None
            })
        );
        assert!(p(&["page"]).is_err(), "a page needs something to look for");
        assert!(p(&["page", "x", "--json"]).is_err());
        assert!(p(&["status", "word"]).is_err());
        assert!(usage().contains("notchling page <link|id|words>"));
    }

    #[test]
    fn service_actions_take_a_dir() {
        assert_eq!(
            p(&["service", "install", "--dir", "/tmp/x"]),
            Ok(Command::Service {
                action: ServiceAction::Install,
                dir: Some("/tmp/x".into())
            })
        );
        assert_eq!(
            p(&["service", "print"]),
            Ok(Command::Service {
                action: ServiceAction::Print,
                dir: None
            })
        );
        assert!(p(&["service"]).is_err());
        assert!(p(&["service", "start"]).is_err());
        assert!(p(&["service", "install", "--socket", "/x"]).is_err());
    }

    #[test]
    fn bad_arguments_are_errors() {
        assert!(p(&["daemon", "--socket"]).is_err());
        assert!(p(&["status", "--json"]).is_err());
        assert!(p(&["fly"]).is_err());
        assert!(p(&["daemon", "--dir", "/x"]).is_err());
    }

    #[test]
    fn the_version_line_names_the_binary() {
        assert_eq!(
            version_line(),
            format!("notchling {}", env!("CARGO_PKG_VERSION"))
        );
        assert!(usage().contains("service install"));
    }
}
