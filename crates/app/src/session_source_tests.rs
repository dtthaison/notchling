use super::*;
use crate::testutil::Scratch;
#[cfg(unix)]
use notchling_engine::sessions::{AgentGroup, AgentTarget};
use std::sync::atomic::AtomicUsize;

fn quick() -> Backoff {
    Backoff {
        first: Duration::from_millis(20),
        max: Duration::from_millis(80),
    }
}

#[cfg(unix)]
fn agent(id: &str) -> Agent {
    Agent {
        id: id.into(),
        name: format!("name {id}"),
        project: "p".into(),
        cwd: "/w/p".into(),
        group: AgentGroup::Working,
        label: "Working".into(),
        detail: None,
        since_ms: 1_000,
        target: AgentTarget::Job(id.into()),
        suggested_reply: None,
        transcript_path: None,
        host: "Background".into(),
        headless: false,
        config_dir: None,
        command: None,
        resume_session_id: None,
        at_prompt: false,
    }
}

#[cfg(unix)]
fn ids(view: &View) -> Vec<String> {
    view.agents.iter().flatten().map(|a| a.id.clone()).collect()
}

fn wait_for(source: &SessionSource, what: &str, ok: impl Fn(&View) -> bool) -> View {
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let view = source.view();
        if ok(&view) {
            return view;
        }
        assert!(Instant::now() < end, "waited for {what}; saw {view:?}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn real_root(data_dir: &Path) -> DataRoot {
    DataRoot {
        claude_dir: data_dir.join("claude"),
        data_dir: data_dir.to_path_buf(),
        codex_dir: data_dir.join("codex"),
        antigravity_dir: data_dir.join("antigravity-cli"),
        home: data_dir.to_path_buf(),
        fixture: None,
    }
}

fn failing_starter(calls: &Arc<AtomicUsize>) -> Starter {
    let calls = calls.clone();
    Arc::new(move |_| {
        calls.fetch_add(1, Ordering::SeqCst);
        Err(ClientError::Timeout)
    })
}

#[test]
fn a_catalog_reply_is_read_for_its_models_and_an_empty_or_odd_one_is_ignored() {
    use notchling_engine::catalog::{Catalog, Tool};
    let models = Catalog::built_in().models(Tool::Claude, &[]);
    let reply = serde_json::json!({"tool": "claude", "models": models, "fetchedAt": 1.0});
    assert_eq!(parse_catalog(&reply), Some(models));
    assert_eq!(parse_catalog(&serde_json::json!({"models": []})), None);
    assert_eq!(parse_catalog(&serde_json::json!({"models": "x"})), None);
    assert_eq!(parse_catalog(&serde_json::json!({})), None);
}

#[test]
fn the_catalog_is_read_when_asked_and_at_least_hourly() {
    let last = Instant::now();
    assert!(!catalog_due(last, last, false));
    assert!(!catalog_due(last, last + Duration::from_secs(3599), false));
    assert!(catalog_due(last, last + Duration::from_secs(3600), false));
    assert!(catalog_due(last, last, true));
    assert!(!catalog_due(last + Duration::from_secs(5), last, false));
}

#[test]
fn backoff_doubles_and_never_passes_ten_seconds() {
    let b = Backoff::default();
    let secs: Vec<u64> = (0..6).map(|n| b.delay(n).as_secs()).collect();
    assert_eq!(secs, [1, 2, 4, 8, 10, 10]);
    assert_eq!(b.delay(40), Duration::from_secs(10));
    assert_eq!(b.delay(u32::MAX), Duration::from_secs(10));
}

#[test]
fn a_fixture_root_gets_no_daemon_configuration() {
    let exe = Path::new("/Applications/Notchling.app/Contents/MacOS/notchling-app");
    assert!(config_for(&DataRoot::fixture(Path::new("/f")), Some(exe)).is_none());
}

#[test]
fn the_real_root_names_the_socket_in_its_data_folder_and_the_binary_beside_the_app() {
    let root = real_root(Path::new("/data"));
    let exe = Path::new("/Applications/Notchling.app/Contents/MacOS/notchling-app");
    let config = config_for(&root, Some(exe)).expect("a configuration");
    assert_eq!(config.socket, Path::new("/data").join(SOCKET_NAME));
    assert!(config.start.is_some());
    assert_eq!(config.backoff, Backoff::default());
    assert!(config_for(&root, None)
        .expect("a configuration")
        .start
        .is_none());
}

#[test]
fn a_fixture_source_reads_files_and_says_nothing_about_a_daemon() {
    let source = SessionSource::for_run(
        Some(&DataRoot::fixture(Path::new("/f"))),
        Some(Path::new("/x/target/debug/notchling-app")),
    );
    let view = source.view();
    assert_eq!(view.link, Link::Files);
    assert!(view.agents.is_none() && !view.daemon_down());
    let none = SessionSource::for_run(None, None);
    assert_eq!(none.view().link, Link::Files);
}

#[test]
fn with_no_socket_it_falls_back_says_so_and_starts_the_daemon_only_once() {
    let scratch = Scratch::new("ss-none");
    let calls = Arc::new(AtomicUsize::new(0));
    let source = SessionSource::start(SourceConfig {
        socket: scratch.path().join(SOCKET_NAME),
        start: Some(failing_starter(&calls)),
        backoff: quick(),
        version: "test".into(),
        check_server: true,
    });
    let view = wait_for(&source, "the fallback", View::daemon_down);
    assert!(view.agents.is_none(), "files are read instead");
    std::thread::sleep(Duration::from_millis(400));
    assert!(source.view().daemon_down());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn without_a_starter_nothing_is_started_and_the_source_still_retries() {
    let scratch = Scratch::new("ss-nostart");
    let source = SessionSource::start(SourceConfig {
        socket: scratch.path().join(SOCKET_NAME),
        start: None,
        backoff: quick(),
        version: "test".into(),
        check_server: true,
    });
    wait_for(&source, "the fallback", View::daemon_down);
    assert!(!scratch.path().join(SOCKET_NAME).exists());
}

#[test]
fn the_ui_side_never_waits_on_the_socket() {
    let scratch = Scratch::new("ss-fast");
    let slow: Starter = Arc::new(|_| {
        std::thread::sleep(Duration::from_millis(800));
        Err(ClientError::Timeout)
    });
    let began = Instant::now();
    let source = SessionSource::start(SourceConfig {
        socket: scratch.path().join(SOCKET_NAME),
        start: Some(slow),
        backoff: quick(),
        version: "test".into(),
        check_server: true,
    });
    let view = source.view();
    assert!(began.elapsed() < Duration::from_millis(300));
    assert_eq!(view.link, Link::Connecting, "no note while still trying");
    assert!(!view.daemon_down());
}

#[cfg(unix)]
mod over_a_socket {
    use super::*;
    use notchling_local_api::{
        Allowlist, ApiError, Caller, Handler, Limits, OpSpec, Server, ServerConfig, ServerHandle,
    };
    use serde_json::Value;
    use std::thread::JoinHandle;

    type Running = (ServerHandle, JoinHandle<std::io::Result<()>>);

    struct Fake {
        list: Arc<Mutex<Value>>,
    }

    impl Handler for Fake {
        fn spec(&self, op: &str) -> Option<OpSpec> {
            (op == "sessions.list").then_some(OpSpec::AGENT)
        }

        fn call(&self, _: &Caller, _: &str, _: &Value) -> Result<Value, ApiError> {
            Ok(self.list.lock().unwrap_or_else(|e| e.into_inner()).clone())
        }

        fn topics(&self) -> &[&str] {
            &["sessions"]
        }
    }

    fn list_of(names: &[&str]) -> Arc<Mutex<Value>> {
        let agents: Vec<Agent> = names.iter().map(|n| agent(n)).collect();
        Arc::new(Mutex::new(serde_json::to_value(agents).expect("rows")))
    }

    fn serve(socket: &Path, list: &Arc<Mutex<Value>>) -> Running {
        let server = Server::bind(
            ServerConfig {
                socket: socket.to_path_buf(),
                allowlist: Allowlist::empty(),
                events: None,
                version: "test".into(),
                limits: Limits::default(),
            },
            Arc::new(Fake { list: list.clone() }),
        )
        .expect("bind");
        server.spawn().expect("serve")
    }

    fn stop((handle, thread): Running) {
        handle.shutdown();
        thread.join().expect("server thread").expect("server ended");
    }

    fn config(socket: &Path, start: Option<Starter>) -> SourceConfig {
        SourceConfig {
            socket: socket.to_path_buf(),
            start,
            backoff: quick(),
            version: "test".into(),
            check_server: false,
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn a_socket_served_by_something_else_is_not_talked_to_and_says_so() {
        let scratch = Scratch::new("ss-untrusted");
        let socket = scratch.path().join(SOCKET_NAME);
        let running = serve(&socket, &list_of(&["from-a-stranger"]));
        let calls = Arc::new(AtomicUsize::new(0));
        let source = SessionSource::start(SourceConfig {
            check_server: true,
            ..config(&socket, Some(failing_starter(&calls)))
        });
        let view = wait_for(&source, "the untrusted note", |v| v.link == Link::Untrusted);
        assert!(view.agents.is_none(), "files are read instead");
        assert_eq!(view.note(), Some(DAEMON_UNTRUSTED_NOTE));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_eq!(running.0.status().clients, Default::default());
        stop(running);
    }

    #[test]
    fn it_lists_then_follows_sessions_changed() {
        let scratch = Scratch::new("ss-follow");
        let socket = scratch.path().join(SOCKET_NAME);
        let running = serve(&socket, &list_of(&["a", "b"]));
        let source = SessionSource::start(config(&socket, None));

        let view = wait_for(&source, "the first list", |v| v.link == Link::Daemon);
        assert_eq!(ids(&view), ["a", "b"]);
        assert!(!view.daemon_down());

        let before = source.generation();
        let next = serde_json::to_value(vec![agent("c")]).expect("rows");
        running.0.publish("sessions", "sessions.changed", &next);
        let view = wait_for(&source, "the change", |v| ids(v) == ["c"]);
        assert!(view.generation > before);
        stop(running);
    }

    struct CatalogFake {
        list: Value,
        models: Arc<Mutex<Value>>,
        gets: Arc<AtomicUsize>,
    }

    impl Handler for CatalogFake {
        fn spec(&self, op: &str) -> Option<OpSpec> {
            matches!(op, "sessions.list" | "catalog.get").then_some(OpSpec::AGENT)
        }

        fn call(&self, _: &Caller, op: &str, _: &Value) -> Result<Value, ApiError> {
            Ok(match op {
                "catalog.get" => {
                    self.gets.fetch_add(1, Ordering::SeqCst);
                    serde_json::json!({
                    "tool": "claude",
                    "models": self.models.lock().unwrap_or_else(|e| e.into_inner()).clone(),
                    "fetchedAt": 1.0,
                    })
                }
                _ => self.list.clone(),
            })
        }

        fn topics(&self) -> &[&str] {
            &["sessions", "tools"]
        }
    }

    #[test]
    fn it_asks_for_the_claude_catalog_at_start_and_again_on_catalog_changed() {
        use notchling_engine::catalog::{Catalog, Tool};
        let built_in = Catalog::built_in().models(Tool::Claude, &[]);
        let scratch = Scratch::new("ss-catalog");
        let socket = scratch.path().join(SOCKET_NAME);
        let gets = Arc::new(AtomicUsize::new(0));
        let models = Arc::new(Mutex::new(
            serde_json::to_value(&built_in[..2]).expect("models"),
        ));
        let server = Server::bind(
            ServerConfig {
                socket: socket.clone(),
                allowlist: Allowlist::empty(),
                events: None,
                version: "test".into(),
                limits: Limits::default(),
            },
            Arc::new(CatalogFake {
                list: serde_json::to_value(vec![agent("a")]).expect("rows"),
                models: models.clone(),
                gets: gets.clone(),
            }),
        )
        .expect("bind");
        let running = server.spawn().expect("serve");
        let source = SessionSource::start(config(&socket, None));
        assert!(
            source.catalog(Tool::Claude).is_empty(),
            "nothing before it answers"
        );

        let end = Instant::now() + Duration::from_secs(10);
        while source.catalog(Tool::Claude).len() != 2 {
            assert!(Instant::now() < end, "waited for the first catalog");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(source.catalog(Tool::Claude), built_in[..2]);

        *models.lock().unwrap_or_else(|e| e.into_inner()) =
            serde_json::to_value(&built_in[..3]).expect("models");
        running.0.publish(
            "tools",
            "catalog.changed",
            &serde_json::json!({"tool": "claude"}),
        );
        while source.catalog(Tool::Claude).len() != 3 {
            assert!(Instant::now() < end, "waited for the changed catalog");
            std::thread::sleep(Duration::from_millis(10));
        }
        let asked_at = gets.load(Ordering::SeqCst);
        let rows = source.generation();
        *models.lock().unwrap_or_else(|e| e.into_inner()) =
            serde_json::to_value(&built_in[..4]).expect("models");
        let catalog_before = source.catalog_generation();
        source.ask_catalog();
        while source.catalog(Tool::Claude).len() != 4 {
            assert!(Instant::now() < end, "waited for the asked catalog");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(gets.load(Ordering::SeqCst) > asked_at);
        assert!(source.catalog_generation() > catalog_before);
        assert_eq!(source.generation(), rows, "the conversation isn't reloaded");
        assert!(source.catalog(Tool::Codex).is_empty());
        stop(running);
    }

    #[test]
    fn a_daemon_that_goes_away_falls_back_and_a_new_one_is_picked_up() {
        let scratch = Scratch::new("ss-redo");
        let socket = scratch.path().join(SOCKET_NAME);
        let first = serve(&socket, &list_of(&["a"]));
        let source = SessionSource::start(config(&socket, None));
        wait_for(&source, "the daemon", |v| ids(v) == ["a"]);

        stop(first);
        let down = wait_for(&source, "the fallback", View::daemon_down);
        assert!(down.agents.is_none());

        let second = serve(&socket, &list_of(&["b", "c"]));
        let up = wait_for(&source, "the new daemon", |v| v.link == Link::Daemon);
        assert_eq!(ids(&up), ["b", "c"]);
        stop(second);
    }

    #[test]
    fn a_missing_daemon_is_started_once_and_then_used() {
        let scratch = Scratch::new("ss-spawn");
        let socket = scratch.path().join(SOCKET_NAME);
        let calls = Arc::new(AtomicUsize::new(0));
        let started: Arc<Mutex<Option<Running>>> = Arc::new(Mutex::new(None));
        let starter: Starter = {
            let (calls, started) = (calls.clone(), started.clone());
            Arc::new(move |socket| {
                calls.fetch_add(1, Ordering::SeqCst);
                *started.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(serve(socket, &list_of(&["from-the-new-daemon"])));
                Ok(Started::Spawned)
            })
        };
        let source = SessionSource::start(config(&socket, Some(starter)));
        let view = wait_for(&source, "the started daemon", |v| v.link == Link::Daemon);
        assert_eq!(ids(&view), ["from-the-new-daemon"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let running = started.lock().unwrap_or_else(|e| e.into_inner()).take();
        stop(running.expect("the started daemon"));
        wait_for(&source, "the fallback", View::daemon_down);
        std::thread::sleep(Duration::from_millis(300));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn dropping_the_source_ends_its_connection() {
        let scratch = Scratch::new("ss-drop");
        let socket = scratch.path().join(SOCKET_NAME);
        let running = serve(&socket, &list_of(&["a"]));
        let source = SessionSource::start(config(&socket, None));
        wait_for(&source, "the daemon", |v| v.link == Link::Daemon);
        assert_eq!(running.0.connections(), 1);
        drop(source);
        let end = Instant::now() + Duration::from_secs(5);
        while running.0.connections() > 0 {
            assert!(Instant::now() < end, "the client stayed connected");
            std::thread::sleep(Duration::from_millis(20));
        }
        stop(running);
    }
}
