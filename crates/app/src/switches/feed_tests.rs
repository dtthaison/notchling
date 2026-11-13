use super::*;
use crate::switches::restart::{turn_back_on, Area, RestartNotice, TurnOn};
use crate::testutil::Scratch;
use notchling_local_api::{
    Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle, Tier,
};
use std::path::Path;
use std::thread::JoinHandle;

type Running = (ServerHandle, JoinHandle<std::io::Result<()>>);
type Calls = Arc<Mutex<Vec<(String, Value)>>>;

struct Daemon {
    calls: Calls,
    on: Mutex<Switches>,
}

impl Handler for Daemon {
    fn spec(&self, op: &str) -> Option<OpSpec> {
        match op {
            ops::SETTINGS_SWITCHES => Some(OpSpec::AGENT),
            ops::SETTINGS_SET_SWITCH | ops::VAULT_PENDING => Some(OpSpec::APP),
            _ => None,
        }
    }

    fn call(&self, _: &Caller, op: &str, args: &Value) -> Result<Value, ApiError> {
        self.calls
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push((op.to_owned(), args.clone()));
        let mut on = self.on.lock().unwrap_or_else(|e| e.into_inner());
        match op {
            ops::SETTINGS_SWITCHES => Ok(json!({
                "accounts": on.accounts, "vault": on.vault, "install": on.install
            })),
            ops::VAULT_PENDING => Ok(json!({
                "approvals": [{"id": "ap-early", "names": ["github"], "command": "gh api /user"}],
                "requests": []
            })),
            _ => {
                on.apply(args);
                Ok(args.clone())
            }
        }
    }

    fn topics(&self) -> &[&str] {
        &["settings", "accounts", "providers", "limits", "vault"]
    }

    fn topic_tier(&self, _: &str) -> Tier {
        Tier::App
    }
}

fn serve(socket: &Path, calls: &Calls) -> Running {
    Server::bind(
        ServerConfig {
            socket: socket.to_path_buf(),
            allowlist: Allowlist::new([std::env::current_exe().expect("this test binary")]),
            audit: None,
            version: "test".into(),
            limits: Limits::default(),
        },
        Arc::new(Daemon {
            calls: calls.clone(),
            on: Mutex::new(Switches::all_off()),
        }),
    )
    .expect("bind")
    .spawn()
    .expect("serve")
}

struct Direct {
    socket: std::path::PathBuf,
}

impl Transport for Direct {
    fn call(&self, op: &str, args: Value) -> Result<Value, ClientError> {
        let options = ConnectOptions {
            check_server: false,
            ..ConnectOptions::default()
        };
        Client::connect_with(&self.socket, ClientKind::App, "test", &options)?.call(op, args)
    }
}

struct Chosen(Mutex<Vec<Switch>>);

impl SwitchChoice for Chosen {
    fn get(&self, switch: Switch) -> bool {
        self.0.lock().unwrap().contains(&switch)
    }
    fn set(&self, switch: Switch, on: bool) -> Result<(), String> {
        let mut c = self.0.lock().unwrap();
        c.retain(|s| *s != switch);
        if on {
            c.push(switch);
        }
        Ok(())
    }
}

fn start(socket: &Path, chosen: Vec<Switch>) -> SwitchFeed {
    start_with(socket, chosen, Arc::default())
}

fn start_with(socket: &Path, chosen: Vec<Switch>, restart: Arc<RestartNotice>) -> SwitchFeed {
    let config = FeedConfig {
        socket: socket.to_path_buf(),
        version: "test".into(),
        check_server: false,
        backoff: Backoff {
            first: Duration::from_millis(20),
            max: Duration::from_millis(80),
        },
    };
    SwitchFeed::start(
        config,
        Arc::new(Direct {
            socket: socket.to_path_buf(),
        }),
        Arc::new(Chosen(Mutex::new(chosen))),
        restart,
    )
}

fn wait_until(what: &str, ok: impl Fn() -> bool) {
    let end = Instant::now() + Duration::from_secs(10);
    while !ok() {
        assert!(Instant::now() < end, "waited for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn a_remembered_choice_sends_nothing_and_only_the_confirmed_notice_turns_them_on() {
    let scratch = Scratch::new("sf-rs");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let daemon = serve(&socket, &calls);
    let notice = Arc::new(RestartNotice::default());
    let feed = start_with(&socket, Switch::ALL.to_vec(), notice.clone());
    wait_until("the follow", || {
        feed.following() && !notice.state().offer().is_empty()
    });
    let sent: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    assert_eq!(
        sent,
        ["settings.switches", "vault.pending"],
        "no switch is turned on"
    );
    let waiting = feed.approvals();
    assert_eq!(waiting.len(), 1);
    assert_eq!(waiting[0]["id"], "ap-early");
    let s = feed.switches();
    assert!(s.attached && !s.vault && !s.accounts && !s.install, "{s:?}");
    let confirm = notice.state().confirm().unwrap();
    assert_eq!(confirm.areas, [Area::Accounts, Area::Vault, Area::Install]);
    let failed = turn_back_on(&confirm.areas, &FeedOnly(&feed));
    assert!(failed.is_empty(), "{failed:?}");
    let sets: Vec<Value> = calls
        .lock()
        .unwrap()
        .iter()
        .filter(|c| c.0 == "settings.set_switch")
        .map(|c| c.1.clone())
        .collect();
    assert_eq!(
        sets,
        [
            json!({"switch": "accounts", "on": true}),
            json!({"switch": "vault", "on": true}),
            json!({"switch": "install", "on": true})
        ]
    );
    assert_eq!(notice.state().offer(), []);
    daemon.0.shutdown();
    let _ = daemon.1.join();
}

struct FeedOnly<'a>(&'a SwitchFeed);

impl TurnOn for FeedOnly<'_> {
    fn turn_on(&self, area: Area) -> Result<(), String> {
        let switch = match area {
            Area::Accounts => Switch::Accounts,
            Area::Vault => Switch::Vault,
            Area::Install => Switch::Install,
            _ => return Err("not the feed's".into()),
        };
        self.0.choose(switch, true).map(|_| ())
    }
}

#[test]
fn the_app_reads_the_switches_and_follows_the_events() {
    let scratch = Scratch::new("switch-feed");
    let socket = scratch.path().join(SOCKET_NAME);
    let calls: Calls = Arc::default();
    let daemon = serve(&socket, &calls);
    let feed = start(&socket, vec![]);
    wait_until("the follow", || {
        feed.following() && daemon.0.app_following("vault")
    });
    let s = feed.switches();
    assert!(s.attached && !s.vault && !s.accounts, "{s:?}");
    assert_eq!(
        feed.control_of(Switch::Accounts).tooltip,
        Some(super::super::ACCOUNTS_OFF)
    );
    let sent: Vec<String> = calls.lock().unwrap().iter().map(|c| c.0.clone()).collect();
    assert_eq!(sent, ["settings.switches", "vault.pending"]);
    wait_until("the early approval", || feed.approvals().len() == 1);
    daemon.0.publish(
        "vault",
        "vault.approval.resolved",
        &json!({"id": "ap-early", "decision": "deny"}),
    );
    wait_until("early resolved", || feed.approvals().is_empty());

    daemon.0.publish(
        "settings",
        "settings.switch",
        &json!({"switch": "accounts", "on": true}),
    );
    wait_until("accounts on", || feed.switches().accounts);
    assert!(feed.control_of(Switch::Accounts).enabled);

    daemon.0.publish(
        "vault",
        "vault.approval.pending",
        &json!({"id": "ap-1", "names": ["github"], "command": "gh api", "folder": "/w"}),
    );
    wait_until("the approval", || !feed.approvals().is_empty());
    assert_eq!(feed.approvals()[0]["id"], "ap-1");
    daemon.0.publish(
        "vault",
        "vault.approval.resolved",
        &json!({"id": "ap-1", "decision": "deny"}),
    );
    wait_until("resolved", || feed.approvals().is_empty());

    let after = feed.choose(Switch::Install, true).unwrap();
    assert!(after.install);
    assert!(feed.chosen(Switch::Install));
    assert_eq!(
        calls.lock().unwrap().last().unwrap().1,
        json!({"switch": "install", "on": true})
    );
    daemon.0.shutdown();
    let _ = daemon.1.join();
    wait_until("the loss", || !feed.following());
    assert!(
        !feed.switches().attached,
        "detached when the daemon is gone"
    );
}

impl SwitchFeed {
    fn control_of(&self, switch: Switch) -> crate::acting::Control {
        self.switches().control(switch)
    }
}
