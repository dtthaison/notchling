use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServiceAction {
    Install,
    Uninstall,
    Print,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    Daemon {
        socket: Option<PathBuf>,
    },
    Status {
        json: bool,
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
    Hook {
        socket: Option<PathBuf>,
    },
    Pages {
        query: String,
        json: bool,
        socket: Option<PathBuf>,
    },
    Page {
        query: String,
        json: bool,
        socket: Option<PathBuf>,
    },
    Delegate {
        args: Vec<String>,
    },
    DelegateModels {
        args: Vec<String>,
    },
    Secrets {
        args: Vec<String>,
    },
    Secret {
        args: Vec<String>,
    },
    SecretRequest {
        args: Vec<String>,
    },
    Version,
    Help,
}

pub fn version_line() -> String {
    format!("notchling {}", env!("CARGO_PKG_VERSION"))
}

pub fn usage() -> &'static str {
    "usage:\n  \
     notchling daemon [--socket <path>]          serve the local API\n  \
     notchling status [--json] [--socket <path>]  the daemon's version, pid, uptime and clients\n  \
     notchling sessions [--json] [--socket <path>]  the sessions the daemon sees\n  \
     notchling pages [words] [--json] [--socket <path>]  the saved Artifact pages (id, title, account), newest first\n  \
     notchling page <link|id|words> [--json] [--socket <path>]  where a saved page is, and who can republish it\n  \
     notchling delegate <codex|agy> [--model --effort --folder --out --write --session --timeout --json] -- <prompt>\n  \
     \u{20}                                           hand a job to another tool and wait (read-only unless --write)\n  \
     notchling delegate-models [codex|agy] [--json]  the models and efforts each tool offers\n  \
     notchling secrets [words] [--json]    the saved secrets that fit a URL, host or words (never values)\n  \
     notchling secret <name>[,<name>…] -- <command> [args…]\n  \
     \u{20}                                           run a command with secrets, its output hidden\n  \
     notchling secret-request <name> [--kind token|login --url --note --how --expires YYYY-MM-DD --reason]\n  \
     \u{20}                                           ask the user to add or update a secret, and wait\n  \
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
    match first.as_str() {
        "delegate" => {
            return Ok(Command::Delegate {
                args: args.collect(),
            })
        }
        "delegate-models" => {
            return Ok(Command::DelegateModels {
                args: args.collect(),
            })
        }
        "secrets" => {
            return Ok(Command::Secrets {
                args: args.collect(),
            })
        }
        "secret" => {
            return Ok(Command::Secret {
                args: args.collect(),
            })
        }
        "secret-request" => {
            return Ok(Command::SecretRequest {
                args: args.collect(),
            })
        }
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
            "--json" if matches!(first.as_str(), "sessions" | "status" | "pages" | "page") => {
                json = true
            }
            other => return Err(format!("unknown argument for {first}: {other}")),
        }
    }
    match (first.as_str(), action) {
        ("daemon", _) => Ok(Command::Daemon { socket }),
        ("status", _) => Ok(Command::Status { json, socket }),
        ("sessions", _) => Ok(Command::Sessions { json, socket }),
        ("hook", _) => Ok(Command::Hook { socket }),
        ("pages", _) => Ok(Command::Pages {
            query: words.join(" "),
            json,
            socket,
        }),
        ("page", _) if words.is_empty() => {
            Err("page needs a link, an id or words from the title".into())
        }
        ("page", _) => Ok(Command::Page {
            query: words.join(" "),
            json,
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
        assert_eq!(
            p(&["status"]),
            Ok(Command::Status {
                json: false,
                socket: None
            })
        );
        assert_eq!(
            p(&["status", "--json", "--socket", "/tmp/s.sock"]),
            Ok(Command::Status {
                json: true,
                socket: Some("/tmp/s.sock".into())
            })
        );
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
                json: false,
                socket: None
            })
        );
        assert_eq!(
            p(&["pages", "cost", "report", "--socket", "/tmp/s.sock"]),
            Ok(Command::Pages {
                query: "cost report".into(),
                json: false,
                socket: Some("/tmp/s.sock".into())
            })
        );
        assert_eq!(
            p(&["page", "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv"]),
            Ok(Command::Page {
                query: "https://claude.ai/artifact/AbCdEfGhIjKlMnOpQrStUv".into(),
                json: false,
                socket: None
            })
        );
        assert!(p(&["page"]).is_err(), "a page needs something to look for");
        assert_eq!(
            p(&["pages", "--json", "cost"]),
            Ok(Command::Pages {
                query: "cost".into(),
                json: true,
                socket: None
            })
        );
        assert_eq!(
            p(&["page", "x", "--json"]),
            Ok(Command::Page {
                query: "x".into(),
                json: true,
                socket: None
            })
        );
        assert!(
            p(&["page", "--json"]).is_err(),
            "--json is not a search word"
        );
        assert!(p(&["status", "word"]).is_err());
        assert!(usage().contains("notchling page <link|id|words>"));
    }

    #[test]
    fn delegate_takes_everything_after_it_as_it_is() {
        assert_eq!(
            p(&["delegate", "codex", "--json", "--", "--help", "-x", "--socket"]),
            Ok(Command::Delegate {
                args: ["codex", "--json", "--", "--help", "-x", "--socket"]
                    .map(String::from)
                    .to_vec()
            })
        );
        assert_eq!(p(&["delegate"]), Ok(Command::Delegate { args: vec![] }));
        assert_eq!(
            p(&["delegate-models", "agy"]),
            Ok(Command::DelegateModels {
                args: vec!["agy".into()]
            })
        );
        assert_eq!(
            p(&["delegate-models", "--json"]),
            Ok(Command::DelegateModels {
                args: vec!["--json".into()]
            })
        );
        let text = usage();
        assert!(
            text.contains("notchling delegate <codex|agy>") && text.contains("delegate-models")
        );
    }

    #[test]
    fn the_vault_commands_take_everything_after_them() {
        assert_eq!(
            p(&["secret", "gh,npm", "--", "curl", "--socket", "x"]),
            Ok(Command::Secret {
                args: ["gh,npm", "--", "curl", "--socket", "x"]
                    .map(String::from)
                    .to_vec()
            })
        );
        assert_eq!(
            p(&["secrets", "api.github.com"]),
            Ok(Command::Secrets {
                args: vec!["api.github.com".into()]
            })
        );
        assert_eq!(
            p(&["secret-request", "gh", "--note", "--odd"]),
            Ok(Command::SecretRequest {
                args: ["gh", "--note", "--odd"].map(String::from).to_vec()
            })
        );
        assert_eq!(
            p(&["secrets", "--json", "github"]),
            Ok(Command::Secrets {
                args: ["--json", "github"].map(String::from).to_vec()
            })
        );
        let text = usage();
        assert!(
            text.contains("notchling secrets [words]") && text.contains("secret-request <name>")
        );
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
        assert!(p(&["daemon", "--json"]).is_err());
        assert!(p(&["service", "print", "--json"]).is_err());
        assert!(p(&["fly"]).is_err());
        assert!(p(&["daemon", "--dir", "/x"]).is_err());
    }

    #[test]
    fn every_client_command_shows_json_in_the_usage() {
        let lines: Vec<&str> = usage().lines().collect();
        for command in [
            "status",
            "sessions",
            "pages",
            "page",
            "delegate-models",
            "secrets",
        ] {
            let line = lines
                .iter()
                .find(|l| l.trim_start().starts_with(&format!("notchling {command} ")))
                .unwrap_or_else(|| panic!("no usage line for {command}"));
            assert!(line.contains("[--json]"), "{line}");
        }
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
