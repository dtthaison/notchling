use notchling_engine::sessions::{Agent, AgentGroup};
use notchling_local_api::{Client, ClientError, ClientKind, DaemonStatus};
use serde_json::Value;
use std::path::Path;

pub const NOT_RUNNING: i32 = 3;

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

pub fn status(socket: &Path, json: bool) -> i32 {
    match connect(socket).and_then(|mut c| c.status()) {
        Ok(s) if json => match serde_json::to_string_pretty(&s) {
            Ok(text) => {
                println!("{text}");
                0
            }
            Err(e) => {
                eprintln!("notchling: {e}");
                1
            }
        },
        Ok(s) => {
            println!("{}", status_line(&s));
            0
        }
        Err(e) => fail(e),
    }
}

pub fn list_every_tool(client: &mut Client) -> Result<Value, ClientError> {
    client.call(
        notchling_local_api::ops::SESSIONS_LIST,
        serde_json::json!({"tools": "all"}),
    )
}

pub fn sessions(socket: &Path, json: bool) -> i32 {
    let mut client = match connect(socket) {
        Ok(c) => c,
        Err(e) => return fail(e),
    };
    let list = match list_every_tool(&mut client) {
        Ok(list) => list,
        Err(e) => return fail(e),
    };
    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&list).unwrap_or_else(|_| list.to_string())
        );
        return 0;
    }
    match serde_json::from_value::<Vec<Agent>>(list) {
        Ok(agents) => {
            for line in session_lines(&agents) {
                println!("{line}");
            }
            0
        }
        Err(e) => fail(ClientError::Protocol(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::sessions::AgentTarget;
    use notchling_local_api::wire::ClientCounts;

    #[cfg(unix)]
    #[test]
    fn sessions_asks_for_every_tool() {
        use notchling_local_api::{
            ops, Allowlist, ApiError, Caller, ConnectOptions, Handler, Limits, OpSpec, Server,
            ServerConfig,
        };
        use serde_json::json;
        use std::sync::{Arc, Mutex};

        struct Recorder(Arc<Mutex<Vec<Value>>>);

        impl Handler for Recorder {
            fn spec(&self, op: &str) -> Option<OpSpec> {
                (op == ops::SESSIONS_LIST).then_some(OpSpec::AGENT)
            }

            fn call(&self, _: &Caller, _: &str, args: &Value) -> Result<Value, ApiError> {
                self.0
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .push(args.clone());
                Ok(json!([]))
            }
        }

        let folder = std::path::PathBuf::from(format!("/tmp/nl-cs-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let socket = folder.join("s.sock");
        let asked = Arc::new(Mutex::new(Vec::new()));
        let server = Server::bind(
            ServerConfig {
                socket: socket.clone(),
                allowlist: Allowlist::new([std::env::current_exe().unwrap()]),
                events: None,
                version: "9.9.9".into(),
                limits: Limits::default(),
            },
            Arc::new(Recorder(asked.clone())),
        )
        .unwrap();
        let (handle, thread) = server.spawn().unwrap();
        let options = ConnectOptions {
            check_server: false,
            ..ConnectOptions::default()
        };
        let mut client = Client::connect_with(&socket, ClientKind::Cli, "0.1.0", &options).unwrap();
        let listed = list_every_tool(&mut client).unwrap();
        handle.shutdown();
        let _ = thread.join();
        let _ = std::fs::remove_dir_all(&folder);
        assert_eq!(listed, json!([]));
        assert_eq!(*asked.lock().unwrap(), vec![json!({"tools": "all"})]);
    }

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
            at_prompt: false,
        };
        let lines = session_lines(&[a]);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("needs input"));
        assert!(lines[0].contains("Question") && lines[0].ends_with("fix the bug  (notchling)"));
        assert_eq!(session_lines(&[]), vec!["No sessions.".to_string()]);
    }
}
