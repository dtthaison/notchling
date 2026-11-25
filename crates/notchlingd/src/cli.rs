use notchling_engine::cluster::build_id::build_id;
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
    HookFence,
    HookGuard,
    AgySidecar,
    AgyHook {
        event: String,
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
    Providers {
        json: bool,
    },
    Delegate {
        args: Vec<String>,
    },
    DelegateModels {
        args: Vec<String>,
    },
    Models {
        args: Vec<String>,
    },
    Cost {
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
    Switch {
        args: Vec<String>,
    },
    Settings {
        args: Vec<String>,
    },
    Cluster {
        args: Vec<String>,
    },
    Lanes {
        args: Vec<String>,
    },
    Machines {
        args: Vec<String>,
    },
    Network {
        args: Vec<String>,
    },
    Install {
        args: Vec<String>,
    },
    CheckUpdates {
        args: Vec<String>,
    },
    Session {
        args: Vec<String>,
    },
    Dispatch {
        args: Vec<String>,
    },
    Jobs {
        args: Vec<String>,
    },
    Job {
        args: Vec<String>,
    },
    Ask {
        args: Vec<String>,
    },
    Asks {
        args: Vec<String>,
    },
    Answer {
        args: Vec<String>,
    },
    Consent {
        args: Vec<String>,
    },
    Consents {
        args: Vec<String>,
    },
    Read {
        name: String,
        args: Vec<String>,
    },
    Watch {
        args: Vec<String>,
    },
    Mine {
        read: crate::mine_cli::Read,
        args: Vec<String>,
    },
    Shells {
        args: Vec<String>,
    },
    Version,
    Help {
        json: bool,
    },
    Capabilities {
        json: bool,
        socket: Option<PathBuf>,
    },
}

pub fn version_line() -> String {
    version_line_of(build_id())
}

fn version_line_of(build: Option<&str>) -> String {
    match build {
        Some(build) => format!("notchling {} ({build})", env!("CARGO_PKG_VERSION")),
        None => format!("notchling {}", env!("CARGO_PKG_VERSION")),
    }
}

pub fn usage() -> &'static str {
    static TEXT: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    TEXT.get_or_init(|| {
        format!(
            "usage:\n  notchling daemon [--socket <path>]\n      serve the local API\n{}\n  notchling service install|uninstall|print [--dir <folder>]\n  notchling hook [--socket <path>]            Claude Code's hook: asks the daemon, passes on any trouble\n  notchling hook --fence                      Claude Code's PreToolUse hook for cluster sessions: reads the fence file only\n  notchling hook --guard                      Claude Code's cost guard hook (log only): records what it would have done and never changes a call\n  notchling agy-sidecar                       Antigravity's sidecar: carries the messages you write in Notchling into Antigravity conversations\n  notchling agy-hook pre-tool|pre-invocation  Antigravity's hooks for the sidecar channel: asks the daemon, passes on any trouble\n  notchling --version | --help\n\nNOTCHLING_DATA_ROOT=<dir> reads <dir>/claude and <dir>/notchling instead of your own folders.\nservice install writes the file and prints the command that loads it; it never loads it.\nEvery command but the hooks and the sidecar exits by one table: run `notchling help --json`.",
            crate::commands::usage_lines().join("\n")
        )
    })
}

fn value(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    match args.next() {
        Some(v) if !v.is_empty() && !v.starts_with("--") => Ok(PathBuf::from(v)),
        _ => Err(format!("{flag} needs a value")),
    }
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let args: Vec<String> = args.into_iter().collect();
    if args == ["hook", "--fence"] {
        return Ok(Command::HookFence);
    }
    if args == ["hook", "--guard"] {
        return Ok(Command::HookGuard);
    }
    if args == ["agy-sidecar"] {
        return Ok(Command::AgySidecar);
    }
    if let [name, event] = args.as_slice() {
        if name == "agy-hook" {
            return if crate::agy_hook_run::known(event) {
                Ok(Command::AgyHook {
                    event: event.clone(),
                })
            } else {
                Err("agy-hook takes pre-tool or pre-invocation".into())
            };
        }
    }
    let mut args = args.into_iter();
    let Some(first) = args.next() else {
        return Ok(Command::Help { json: false });
    };
    match first.as_str() {
        "--version" | "-V" | "version" => return Ok(Command::Version),
        "--help" | "-h" | "help" => {
            let rest: Vec<String> = args.collect();
            return match rest.as_slice() {
                [] => Ok(Command::Help { json: false }),
                [flag] if flag == "--json" => Ok(Command::Help { json: true }),
                _ => Err("help takes only --json".into()),
            };
        }
        "capabilities" => {
            let (mut json, mut socket) = (false, None);
            while let Some(arg) = args.next() {
                match arg.as_str() {
                    "--json" => json = true,
                    "--socket" => socket = Some(value(&mut args, "--socket")?),
                    other => return Err(format!("unknown argument for capabilities: {other}")),
                }
            }
            return Ok(Command::Capabilities { json, socket });
        }
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
        "models" => {
            return Ok(Command::Models {
                args: args.collect(),
            })
        }
        "cost" => {
            return Ok(Command::Cost {
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
        "switch" => {
            return Ok(Command::Switch {
                args: args.collect(),
            })
        }
        "settings" => {
            return Ok(Command::Settings {
                args: args.collect(),
            })
        }
        "cluster" => {
            return Ok(Command::Cluster {
                args: args.collect(),
            })
        }
        "machines" => {
            return Ok(Command::Machines {
                args: args.collect(),
            })
        }
        "network" => {
            return Ok(Command::Network {
                args: args.collect(),
            })
        }
        "install" => {
            return Ok(Command::Install {
                args: args.collect(),
            })
        }
        "check-updates" => {
            return Ok(Command::CheckUpdates {
                args: args.collect(),
            })
        }
        "lanes" => {
            return Ok(Command::Lanes {
                args: args.collect(),
            })
        }
        "session" => {
            return Ok(Command::Session {
                args: args.collect(),
            })
        }
        "dispatch" => {
            return Ok(Command::Dispatch {
                args: args.collect(),
            })
        }
        "jobs" => {
            return Ok(Command::Jobs {
                args: args.collect(),
            })
        }
        "job" => {
            return Ok(Command::Job {
                args: args.collect(),
            })
        }
        "ask" => {
            return Ok(Command::Ask {
                args: args.collect(),
            })
        }
        "asks" => {
            return Ok(Command::Asks {
                args: args.collect(),
            })
        }
        "answer" => {
            return Ok(Command::Answer {
                args: args.collect(),
            })
        }
        "consent" => {
            return Ok(Command::Consent {
                args: args.collect(),
            })
        }
        "consents" => {
            return Ok(Command::Consents {
                args: args.collect(),
            })
        }
        name if crate::reads_cli::NAMES.contains(&name) => {
            return Ok(Command::Read {
                name: name.to_string(),
                args: args.collect(),
            })
        }
        "watch" => {
            return Ok(Command::Watch {
                args: args.collect(),
            })
        }
        "outbox" | "receipts" | "activity" | "prompts" => {
            let read = match first.as_str() {
                "outbox" => crate::mine_cli::Read::Outbox,
                "receipts" => crate::mine_cli::Read::Receipts,
                "activity" => crate::mine_cli::Read::Activity,
                _ => crate::mine_cli::Read::Prompts,
            };
            return Ok(Command::Mine {
                read,
                args: args.collect(),
            });
        }
        "shells" => {
            return Ok(Command::Shells {
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
            "--socket" if action.is_none() && first != "providers" => {
                socket = Some(value(&mut args, "--socket")?)
            }
            "--dir" if action.is_some() => dir = Some(value(&mut args, "--dir")?),
            "--json"
                if matches!(
                    first.as_str(),
                    "sessions" | "status" | "pages" | "page" | "providers"
                ) =>
            {
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
        ("providers", _) => Ok(Command::Providers { json }),
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
        assert_eq!(p(&[]), Ok(Command::Help { json: false }));
        assert_eq!(p(&["--version"]), Ok(Command::Version));
    }

    #[test]
    fn providers_parses_and_shows_json_in_the_usage() {
        assert_eq!(p(&["providers"]), Ok(Command::Providers { json: false }));
        assert_eq!(
            p(&["providers", "--json"]),
            Ok(Command::Providers { json: true })
        );
        assert!(p(&["providers", "--socket", "/tmp/s.sock"]).is_err());
        assert!(p(&["providers", "word"]).is_err());
        let line = usage()
            .lines()
            .find(|l| l.trim_start().starts_with("notchling providers "))
            .expect("a usage line for providers");
        assert!(line.contains("[--json]"), "{line}");
    }

    #[test]
    fn accounts_change_words_and_lanes_route_to_accounts_cli() {
        for words in [
            &["accounts", "switch", "acct-2222222222222222"][..],
            &["accounts", "add", "codex"],
            &["accounts", "move-all", "main"],
            &["accounts", "move", "agent-7", "main"],
        ] {
            assert_eq!(
                p(words),
                Ok(Command::Read {
                    name: "accounts".into(),
                    args: words[1..].iter().map(|s| s.to_string()).collect()
                }),
                "{words:?}"
            );
        }
        assert_eq!(
            p(&["accounts"]),
            Ok(Command::Read {
                name: "accounts".into(),
                args: vec![]
            })
        );
        assert_eq!(
            p(&["lanes", "list", "--json"]),
            Ok(Command::Lanes {
                args: vec!["list".into(), "--json".into()]
            })
        );
        assert_eq!(
            p(&[
                "lanes",
                "switch",
                "lane-4444444444444444",
                "acct-2222222222222222"
            ]),
            Ok(Command::Lanes {
                args: vec![
                    "switch".into(),
                    "lane-4444444444444444".into(),
                    "acct-2222222222222222".into()
                ]
            })
        );
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
    fn the_antigravity_sidecar_and_hooks_are_commands_of_their_own() {
        assert_eq!(p(&["agy-sidecar"]), Ok(Command::AgySidecar));
        assert!(p(&["agy-sidecar", "--socket", "/tmp/s.sock"]).is_err());
        assert_eq!(
            p(&["agy-hook", "pre-tool"]),
            Ok(Command::AgyHook {
                event: "pre-tool".into()
            })
        );
        assert_eq!(
            p(&["agy-hook", "pre-invocation"]),
            Ok(Command::AgyHook {
                event: "pre-invocation".into()
            })
        );
        for bad in [
            vec!["agy-hook"],
            vec!["agy-hook", "post-tool"],
            vec!["agy-hook", "pre-tool", "--x"],
        ] {
            assert!(p(&bad).is_err(), "{bad:?}");
        }
        assert!(usage().contains("notchling agy-sidecar"));
        assert!(usage().contains("notchling agy-hook"));
    }

    #[test]
    fn the_guard_flag_is_a_command_of_its_own() {
        assert_eq!(p(&["hook", "--guard"]), Ok(Command::HookGuard));
        assert!(p(&["hook", "--guard", "--socket", "/tmp/s.sock"]).is_err());
        assert!(p(&["hook", "--socket", "/tmp/s.sock", "--guard"]).is_err());
        assert!(p(&["hook", "--guard", "--fence"]).is_err());
        assert!(usage().contains("notchling hook --guard"));
    }

    #[test]
    fn the_fence_flag_is_a_command_of_its_own() {
        assert_eq!(p(&["hook", "--fence"]), Ok(Command::HookFence));
        assert!(p(&["hook", "--fence", "--socket", "/tmp/s.sock"]).is_err());
        assert!(p(&["hook", "--socket", "/tmp/s.sock", "--fence"]).is_err());
        assert!(p(&["hook", "--fence", "--fence"]).is_err());
        assert!(usage().contains("notchling hook --fence"));
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
        assert_eq!(
            p(&["models", "update", "--json"]),
            Ok(Command::Models {
                args: vec!["update".into(), "--json".into()]
            })
        );
        assert_eq!(
            p(&["cost", "setup", "--apply"]),
            Ok(Command::Cost {
                args: vec!["setup".into(), "--apply".into()]
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
            "models",
            "cost",
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
            version_line_of(None),
            format!("notchling {}", env!("CARGO_PKG_VERSION"))
        );
        assert_eq!(version_line(), version_line_of(build_id()));
        assert!(usage().contains("service install"));
    }

    #[test]
    fn version_line_names_build_when_set() {
        assert_eq!(
            version_line_of(Some("0a32aaca1234-dirty")),
            format!(
                "notchling {} (0a32aaca1234-dirty)",
                env!("CARGO_PKG_VERSION")
            )
        );
        assert_eq!(
            version_line_of(None),
            format!("notchling {}", env!("CARGO_PKG_VERSION"))
        );
    }
}
