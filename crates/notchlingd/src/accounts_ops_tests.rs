//! The accounts and lanes ops called directly: off, nothing reaches the Keychain, the network or a
//! file; on, they run over an in-memory Keychain and scripted HTTP. Every value is a fake one.

use crate::accounts_ops::{AccountsOps, GatedHttp, GatedSecurity, ACCOUNT_OPS};
use crate::acting_ops_tests::{app, sessions, setup_in, FakeRoom, PROMPT};
use crate::op_group::OpGroup;
use crate::switches::{SwitchFlag, ACCOUNTS, ACCOUNTS_OFF};
use notchling_engine::accounts::claude::{ClaudeAccounts, ClaudeEnv, LIVE_SERVICE, SAVED_SERVICE};
use notchling_engine::accounts::fake::{MemoryKeychain, PanicHttp, PanicKeychain, ScriptedHttp};
use notchling_engine::accounts::http::{HttpClient, PROFILE_URL, TOKEN_URL};
use notchling_engine::accounts::keychain::{Keys, SecurityRunner};
use notchling_engine::control::{FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::lanes::LaneStore;
use notchling_engine::sessions::Roots;
use notchling_local_api::{ops, ErrorCode, OpSpec};
use serde_json::json;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;

pub(crate) struct Dir(pub(crate) PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(tag: &str) -> Dir {
    let path = std::env::temp_dir().join(format!("nl-acc-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&path);
    std::fs::create_dir_all(path.join("home/.claude")).unwrap();
    Dir(path)
}

pub(crate) const ID1: &str = "a1a1:0101";
pub(crate) const ID2: &str = "a2a2:0202";

pub(crate) fn profile(id: &str, email: &str) -> String {
    let (a, o) = id.split_once(':').unwrap();
    json!({"account": {"uuid": a, "email": email}, "organization": {"uuid": o, "organization_type": "claude_pro"}})
        .to_string()
}

pub(crate) fn slot(id: &str, email: &str, n: &str) -> String {
    let (a, o) = id.split_once(':').unwrap();
    json!({"claudeAiOauth": {"accessToken": format!("test-access-{n}"), "refreshToken": format!("test-refresh-{n}")},
           "oauthAccount": {"accountUuid": a, "organizationUuid": o, "emailAddress": email}})
    .to_string()
}

pub(crate) struct World {
    pub(crate) d: Dir,
    pub(crate) flag: Arc<SwitchFlag>,
    pub(crate) room: Arc<FakeRoom>,
    pub(crate) ops: AccountsOps,
}

pub(crate) fn world(
    tag: &str,
    on: bool,
    runner: Arc<dyn SecurityRunner>,
    http: Arc<dyn HttpClient>,
) -> World {
    let d = dir(tag);
    let flag = SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, on);
    let room = Arc::new(FakeRoom::default());
    let env = ClaudeEnv {
        keys: Keys {
            runner: Arc::new(GatedSecurity {
                flag: flag.clone(),
                inner: runner.clone(),
            }),
            user: "me".into(),
        },
        undo_keys: Keys {
            runner,
            user: "me".into(),
        },
        http: Arc::new(GatedHttp {
            flag: flag.clone(),
            inner: http,
        }),
        now_ms: Arc::new(|| 1_800_000_000_000),
        config_file: d.0.join("home/.claude.json"),
        list_file: d.0.join("data/daemon/accounts.json"),
        lanes: LaneStore::new(&d.0.join("home"), &d.0.join("data")),
    };
    let random = Arc::new(|| Some([7u8; 64]));
    let ops = AccountsOps::new(
        flag.clone(),
        Arc::new(ClaudeAccounts::new(env)),
        room.clone(),
        random,
    );
    World { d, flag, room, ops }
}

fn files_under(path: &std::path::Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for entry in std::fs::read_dir(path).into_iter().flatten().flatten() {
        let p = entry.path();
        if p.is_dir() {
            out.extend(files_under(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn while_off_every_op_but_the_lists_is_refused_and_nothing_is_touched() {
    let w = world("off", false, Arc::new(PanicKeychain), Arc::new(PanicHttp));
    std::fs::write(w.d.0.join("home/.claude.json"), "{}").unwrap();
    for op in ACCOUNT_OPS {
        assert_eq!(w.ops.spec(op), Some(OpSpec::APP), "{op}");
        let args = json!({"id": ID1, "account": ID1, "name": "x", "login": "signin-1", "target": ID1, "sync": true});
        let result = w.ops.call(&app(), op, &args);
        if op == ops::ACCOUNTS_LIST || op == ops::LANES_LIST {
            assert!(result.is_ok(), "{op}: {result:?}");
        } else {
            let e = result.unwrap_err();
            assert_eq!(
                (e.code, e.message.as_str()),
                (ErrorCode::Forbidden, ACCOUNTS_OFF),
                "{op}"
            );
        }
    }
    let files = files_under(&w.d.0);
    assert_eq!(
        files,
        vec![w.d.0.join("home/.claude.json")],
        "nothing written"
    );
    assert!(w.room.events.lock().unwrap().is_empty());
}

#[test]
fn while_off_the_lists_come_from_the_daemons_own_files() {
    let w = world(
        "off-lists",
        false,
        Arc::new(PanicKeychain),
        Arc::new(PanicHttp),
    );
    std::fs::create_dir_all(w.d.0.join("data/daemon")).unwrap();
    std::fs::write(
        w.d.0.join("data/daemon/accounts.json"),
        format!(r#"[{{"id":"{ID1}","email":"one@example.com","plan":"Pro"}}]"#),
    )
    .unwrap();
    std::fs::write(
        w.d.0.join("data/lanes.json"),
        format!(r#"[{{"name":"one","path":"/lanes/one","accountID":"{ID1}","email":"one@example.com"}}]"#),
    )
    .unwrap();
    let list = w
        .ops
        .call(&app(), ops::ACCOUNTS_LIST, &json!({"sync": true}))
        .unwrap();
    assert_eq!(
        list,
        json!({"on": false, "active": null, "accounts": [
            {"id": ID1, "email": "one@example.com", "plan": "Pro", "needsLogin": false, "lane": "one"}]})
    );
    let lanes = w.ops.call(&app(), ops::LANES_LIST, &json!({})).unwrap();
    assert_eq!(
        lanes["lanes"][0]["startCommand"],
        "CLAUDE_CONFIG_DIR='/lanes/one' claude"
    );
}

#[test]
fn the_gates_hold_even_for_a_path_that_skipped_the_check() {
    let chain = Arc::new(MemoryKeychain::new());
    let flag = SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, false);
    let gated = GatedSecurity {
        flag: flag.clone(),
        inner: chain.clone(),
    };
    assert!(gated.run(&["find-generic-password".into()], None).is_err());
    assert!(chain.calls().is_empty());
    let gated = GatedHttp {
        flag,
        inner: Arc::new(PanicHttp),
    };
    let answer = gated.send(&notchling_engine::accounts::http::HttpRequest::post(
        TOKEN_URL,
        json!({}),
    ));
    assert_eq!(answer.status, 0);
}

#[test]
fn on_a_switch_changes_the_login_and_says_so() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("switch", true, chain.clone(), http.clone());
    std::fs::write(
        w.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\", \"emailAddress\": \"one@example.com\"}}",
    )
    .unwrap();
    chain.put(
        LIVE_SERVICE,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-access-1"}}"#,
    );
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
    let got = w
        .ops
        .call(&app(), ops::ACCOUNTS_SWITCH, &json!({"id": ID2}))
        .unwrap();
    assert_eq!(got["active"], ID2);
    assert!(chain
        .get(LIVE_SERVICE, "me")
        .unwrap()
        .contains("test-access-2"));
    let events = w.room.named("accounts.changed");
    assert_eq!(events.len(), 1);
    let printed = format!("{got} {events:?}");
    assert!(!printed.contains("test-access"), "{printed}");
    // A refusal is `failed`, in words.
    let e = w
        .ops
        .call(&app(), ops::ACCOUNTS_SWITCH, &json!({"id": "a9a9:0909"}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Failed);
    assert!(e.message.starts_with("No saved login"), "{}", e.message);
}

#[test]
fn lanes_are_made_listed_and_closed() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("lanes", true, chain.clone(), http.clone());
    std::fs::write(w.d.0.join("home/.claude.json"), "{}").unwrap();
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
    std::fs::create_dir_all(w.d.0.join("data/daemon")).unwrap();
    std::fs::write(
        w.d.0.join("data/daemon/accounts.json"),
        format!(r#"[{{"id":"{ID2}","email":"two@example.com","plan":"Pro"}}]"#),
    )
    .unwrap();
    let made = w
        .ops
        .call(&app(), ops::LANES_CREATE, &json!({"account": ID2}))
        .unwrap();
    assert_eq!(made["lane"]["name"], "two");
    let listed = w.ops.call(&app(), ops::LANES_LIST, &json!({})).unwrap();
    assert_eq!(listed["lanes"][0]["accountID"], ID2);
    let closed = w
        .ops
        .call(&app(), ops::LANES_REMOVE, &json!({"name": "two"}))
        .unwrap();
    assert_eq!(closed, json!({"removed": true}));
    assert_eq!(w.room.named("accounts.changed").len(), 2);
}

/// The sign-in page's `state`, from its URL.
fn state_of(url: &str) -> String {
    url.split('&')
        .find_map(|p| p.strip_prefix("state="))
        .unwrap_or("")
        .to_string()
}

fn get(port: u16, path: &str) -> String {
    let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes())
        .unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[test]
fn a_browser_sign_in_saves_the_account() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("signin", true, chain.clone(), http.clone());
    http.answer(
        TOKEN_URL,
        200,
        r#"{"access_token":"test-access-3","refresh_token":"test-refresh-3","expires_in":3600}"#,
    );
    http.answer(PROFILE_URL, 200, &profile("a3a3:0303", "three@example.com"));
    let begun = w
        .ops
        .call(&app(), ops::ACCOUNTS_ADD_BEGIN, &json!({}))
        .unwrap();
    let url = begun["url"].as_str().unwrap().to_string();
    let port: u16 = url
        .split("localhost%3A")
        .nth(1)
        .unwrap()
        .split('%')
        .next()
        .unwrap()
        .parse()
        .unwrap();
    let login = begun["login"].as_str().unwrap().to_string();
    let waiting = w
        .ops
        .call(&app(), ops::ACCOUNTS_ADD_FINISH, &json!({"login": login}))
        .unwrap();
    assert_eq!(waiting["state"], "waiting");
    assert!(get(port, "/callback?code=c&state=wrong").starts_with("HTTP/1.1 404"));
    let answer = get(
        port,
        &format!("/callback?code=test-code&state={}", state_of(&url)),
    );
    assert!(answer.starts_with("HTTP/1.1 302"), "{answer}");
    let done = w
        .ops
        .call(&app(), ops::ACCOUNTS_ADD_FINISH, &json!({"login": login}))
        .unwrap();
    assert_eq!(done["state"], "added", "{done}");
    assert_eq!(done["account"]["email"], "three@example.com");
    assert!(chain
        .get(SAVED_SERVICE, "a3a3:0303")
        .unwrap()
        .contains("test-access-3"));
    assert_eq!(w.room.named("accounts.add.ended")[0]["state"], "added");
    assert!(!format!("{done}").contains("test-"), "{done}");
}

#[test]
fn turning_accounts_off_stops_a_sign_in() {
    let w = world(
        "signin-off",
        true,
        Arc::new(PanicKeychain),
        Arc::new(PanicHttp),
    );
    let begun = w
        .ops
        .call(&app(), ops::ACCOUNTS_ADD_BEGIN, &json!({}))
        .unwrap();
    w.flag.set(false);
    let login = begun["login"].as_str().unwrap();
    // `finish` itself is refused while off; the sign-in ended on its own.
    assert!(w
        .ops
        .call(&app(), ops::ACCOUNTS_ADD_FINISH, &json!({"login": login}))
        .is_err());
    let start = std::time::Instant::now();
    while w.room.named("accounts.add.ended").is_empty() && start.elapsed().as_secs() < 5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert_eq!(w.room.named("accounts.add.ended")[0]["state"], "cancelled");
}

#[test]
fn move_all_needs_acting_for_sessions_and_closes_the_lane_then_switches() {
    let chain = Arc::new(MemoryKeychain::new());
    let http = Arc::new(ScriptedHttp::new());
    let w = world("move-all", true, chain.clone(), http.clone());
    std::fs::write(
        w.d.0.join("home/.claude.json"),
        "{\"oauthAccount\": {\"accountUuid\": \"a1a1\", \"organizationUuid\": \"0101\"}}",
    )
    .unwrap();
    chain.put(
        LIVE_SERVICE,
        "me",
        r#"{"claudeAiOauth":{"accessToken":"test-access-1"}}"#,
    );
    chain.put(SAVED_SERVICE, ID2, &slot(ID2, "two@example.com", "2"));
    std::fs::create_dir_all(w.d.0.join("data")).unwrap();
    // The sessions' rows run in the lane `/lanes/b`, which holds account 2.
    std::fs::write(
        w.d.0.join("data/lanes.json"),
        format!(
            r#"[{{"name":"b","path":"/lanes/b","accountID":"{ID2}","email":"two@example.com"}}]"#
        ),
    )
    .unwrap();
    let roots = Roots {
        main_dir: w.d.0.join("home/.claude"),
        lanes_file: w.d.0.join("data/lanes.json"),
    };
    let fake = Arc::new(FakeLauncher::new());
    let launcher: Arc<dyn Launcher> = fake.clone();
    let acting_off = setup_in(
        false,
        PROMPT,
        sessions(),
        roots.clone(),
        Some(launcher.clone()),
    );
    w.ops.attach_acting(Arc::new(acting_off.ops));
    for _ in 0..2 {
        http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
    }
    let dry = w
        .ops
        .call(
            &app(),
            ops::ACCOUNTS_MOVE_ALL,
            &json!({"target": ID2, "dryRun": true}),
        )
        .unwrap();
    assert_eq!(
        dry,
        json!({"plan": {"jobs": ["job-a"], "closeLane": "b", "switchMain": ID2, "signedOut": ["n"]}})
    );
    let e = w
        .ops
        .call(&app(), ops::ACCOUNTS_MOVE_ALL, &json!({"target": ID2}))
        .unwrap_err();
    assert_eq!(e.code, ErrorCode::Forbidden);
    assert!(fake.calls().is_empty(), "nothing stopped");
    assert!(
        chain
            .get(LIVE_SERVICE, "me")
            .unwrap()
            .contains("test-access-1"),
        "nothing switched"
    );

    // With acting on (a fresh ops, since `attach_acting` takes one).
    let w2 = world("move-all-on", true, chain.clone(), http.clone());
    std::fs::write(
        w2.d.0.join("home/.claude.json"),
        std::fs::read(w.d.0.join("home/.claude.json")).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(w2.d.0.join("data")).unwrap();
    std::fs::copy(
        w.d.0.join("data/lanes.json"),
        w2.d.0.join("data/lanes.json"),
    )
    .unwrap();
    let roots2 = Roots {
        main_dir: w2.d.0.join("home/.claude"),
        lanes_file: w2.d.0.join("data/lanes.json"),
    };
    let acting_on = setup_in(true, PROMPT, sessions(), roots2, Some(launcher));
    w2.ops.attach_acting(Arc::new(acting_on.ops));
    fake.push_output(Ok(LaunchOutput::ok("")));
    fake.push_output(Ok(LaunchOutput::ok("backgrounded · 9f8e7d6c (idle)\n")));
    http.answer(PROFILE_URL, 200, &profile(ID1, "one@example.com"));
    let done = w2
        .ops
        .call(&app(), ops::ACCOUNTS_MOVE_ALL, &json!({"target": ID2}))
        .unwrap();
    assert_eq!(done["closedLane"], "b", "{done}");
    assert_eq!(done["switchedTo"], ID2, "{done}");
    assert_eq!(done["moved"], json!(["job-a"]), "{done}");
    let commands = fake.commands();
    assert_eq!(commands[0].argv, vec!["claude", "stop", "abcd1234"]);
    assert_eq!(
        commands[1].env.get("CLAUDE_CONFIG_DIR"),
        Some(&None),
        "resumed on the main login"
    );
    assert!(chain
        .get(LIVE_SERVICE, "me")
        .unwrap()
        .contains("test-access-2"));
}

#[test]
fn the_daemon_routes_the_groups_ops_and_keeps_their_topic_for_the_app() {
    use crate::daemon::Ops;
    use crate::switches::{SwitchOps, Switches};
    use notchling_local_api::{Handler, Tier};
    let w = world(
        "routes",
        false,
        Arc::new(PanicKeychain),
        Arc::new(PanicHttp),
    );
    let switches = Arc::new(Switches::off());
    let daemon = Ops::new(Arc::new(std::sync::Mutex::new(json!([]))))
        .with_group(Arc::new(SwitchOps::new(switches.clone(), w.room.clone())))
        .with_group(Arc::new(w.ops));
    assert_eq!(daemon.spec(ops::SETTINGS_SET_SWITCH), Some(OpSpec::APP));
    assert_eq!(daemon.spec(ops::SETTINGS_SWITCHES), Some(OpSpec::AGENT));
    assert_eq!(daemon.spec(ops::LANES_SWITCH), Some(OpSpec::APP));
    assert_eq!(daemon.topics(), &["sessions", "settings", "accounts"]);
    assert_eq!(daemon.topic_tier("accounts"), Tier::App);
    let e = daemon
        .call(&app(), ops::ACCOUNTS_SWITCH, &json!({"id": ID1}))
        .unwrap_err();
    assert_eq!(e.message, ACCOUNTS_OFF);
    let on = daemon
        .call(
            &app(),
            ops::SETTINGS_SET_SWITCH,
            &json!({"switch": "vault", "on": true}),
        )
        .unwrap();
    assert_eq!(on, json!({"switch": "vault", "on": true}));
    assert!(switches.vault.is_on() && !switches.accounts.is_on());
}
