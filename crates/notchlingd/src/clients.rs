//! `notchling status` and `notchling sessions`: clients of a running daemon. They never start one.

use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_local_api::{Client, ClientError, ClientKind, DaemonStatus};
use std::path::Path;

/// Exit code when no daemon listens.
pub const NOT_RUNNING: i32 = 3;

/// One line about the daemon.
pub fn status_line(s: &DaemonStatus) -> String {
    let acting = match s.acting {
        Some(true) => "; acting on",
        Some(false) => "; acting off",
        None => "",
    };
    format!(
        "notchlingd {}, pid {}, up {}s; clients: app {}, cli {}, hook {}{acting}",
        s.version, s.pid, s.uptime_secs, s.clients.app, s.clients.cli, s.clients.hook
    )
}

fn group_name(g: AgentGroup) -> &'static str {
    match g {
        AgentGroup::NeedsInput => "needs input",
        AgentGroup::Working => "working",
        AgentGroup::Completed => "completed",
    }
}

/// One line per session: group, label, name, project.
pub fn session_lines(agents: &[Agent]) -> Vec<String> {
    if agents.is_empty() {
        return vec!["No sessions.".into()];
    }
    agents
        .iter()
        .map(|a| {
            format!(
                "{:<11}  {:<12}  {}  ({})",
                group_name(a.group),
                a.label,
                a.name,
                a.project
            )
        })
        .collect()
}

fn connect(socket: &Path) -> Result<Client, ClientError> {
    Client::connect(socket, ClientKind::Cli, env!("CARGO_PKG_VERSION"))
}

fn fail(e: ClientError) -> i32 {
    match e {
        ClientError::NotRunning(_) => {
            eprintln!(
                "notchling: the daemon isn't running. Start it with `notchling daemon`, or install the service with `notchling service install`."
            );
            NOT_RUNNING
        }
        e => {
            eprintln!("notchling: {e}");
            1
        }
    }
}

pub fn status(socket: &Path) -> i32 {
    match connect(socket).and_then(|mut c| c.status()) {
        Ok(s) => {
            println!("{}", status_line(&s));
            0
        }
        Err(e) => fail(e),
    }
}

pub fn sessions(socket: &Path, json: bool) -> i32 {
    let mut client = match connect(socket) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    if json {
        return match client.call(
            notchling_local_api::ops::SESSIONS_LIST,
            serde_json::json!({}),
        ) {
            Ok(v) => {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&v).unwrap_or_else(|_| v.to_string())
                );
                0
            }
            Err(e) => fail(e),
        };
    }
    match client.sessions::<Agent>() {
        Ok(agents) => {
            for line in session_lines(&agents) {
                println!("{line}");
            }
            0
        }
        Err(e) => fail(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::AgentTarget;
    use notchling_local_api::wire::ClientCounts;

    #[test]
    fn the_status_line() {
        let mut s = DaemonStatus {
            version: "0.1.0".into(),
            pid: 42,
            uptime_secs: 7,
            clients: ClientCounts {
                app: 1,
                cli: 2,
                hook: 0,
            },
            acting: None,
        };
        assert_eq!(
            status_line(&s),
            "notchlingd 0.1.0, pid 42, up 7s; clients: app 1, cli 2, hook 0"
        );
        s.acting = Some(false);
        assert_eq!(
            status_line(&s),
            "notchlingd 0.1.0, pid 42, up 7s; clients: app 1, cli 2, hook 0; acting off"
        );
        s.acting = Some(true);
        assert!(status_line(&s).ends_with("; acting on"));
    }

    #[test]
    fn session_lines_name_group_label_name_and_project() {
        let a = Agent {
            id: "x".into(),
            name: "fix the bug".into(),
            project: "notchling".into(),
            cwd: "/w".into(),
            group: AgentGroup::NeedsInput,
            label: "Question".into(),
            detail: None,
            since_ms: 0,
            target: AgentTarget::Job("ab".into()),
            suggested_reply: None,
            transcript_path: None,
            host: "Background".into(),
            headless: false,
            config_dir: None,
            command: None,
            resume_session_id: None,
        };
        let lines = session_lines(&[a]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("needs input"));
        assert!(lines[0].contains("Question") && lines[0].ends_with("fix the bug  (notchling)"));
        assert_eq!(session_lines(&[]), vec!["No sessions.".to_string()]);
    }
}
