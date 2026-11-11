//! What the phone bridge says about models on the daemon: the list the phone shows (the catalog's
//! current Claude models) and `session.new`'s `modelId`, started as the catalog's own id with the
//! catalog's own check. Nothing is started: the launcher is `FakeLauncher`.

use crate::acting_ops::ActingOps;
use crate::acting_ops_tests::{setup_in, PROMPT};
use crate::bridge_backend::{Approvals, DaemonActing, DaemonSources};
use crate::catalog_ops_tests::seeded;
use notchling_engine::bridge::live::{Acting as _, Sources as _};
use notchling_engine::catalog::{LiveModel, Tier, Tool};
use notchling_engine::control::{Command, FakeLauncher, LaunchOutput, Launcher};
use notchling_engine::sessions::Roots;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn live(id: &str) -> LiveModel {
    LiveModel {
        id: id.into(),
        ..LiveModel::default()
    }
}

fn ops(live: &[LiveModel], tag: &str) -> (Arc<ActingOps>, Arc<FakeLauncher>, impl Drop) {
    let fake = Arc::new(FakeLauncher::new());
    let launcher: Arc<dyn Launcher> = fake.clone();
    let roots = Roots {
        main_dir: PathBuf::from("/nonexistent/claude"),
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let s = setup_in(true, PROMPT, json!([]), roots, Some(launcher));
    let (tmp, service) = seeded(tag, live, None);
    s.ops.attach_catalog(service);
    (Arc::new(s.ops), fake, tmp)
}

fn sources(ops: &Arc<ActingOps>) -> DaemonSources {
    DaemonSources {
        sessions: Arc::new(Mutex::new(json!([]))),
        acting: ops.clone(),
        approvals: Arc::new(Approvals::new(
            Arc::new(crate::acting_ops_tests::FakeRoom::default()),
            Duration::from_secs(1),
        )),
        projects_dir: PathBuf::from("/nonexistent/projects"),
    }
}

#[test]
fn the_phone_is_offered_the_current_claude_models_only() {
    let (ops, _fake, _tmp) = ops(&[live("claude-fable-5-3")], "models-list");
    let offered = sources(&ops).models();
    let catalog = ops.catalog().models(Tool::Claude);
    let current: Vec<(String, String)> = catalog
        .iter()
        .filter(|m| m.tier == Tier::Current)
        .map(|m| (m.id.clone(), m.name.clone()))
        .collect();
    assert!(!current.is_empty());
    assert_eq!(
        offered
            .iter()
            .map(|m| (m.id.clone(), m.name.clone()))
            .collect::<Vec<_>>(),
        current
    );
    // A model the tool lists now is offered; an older one is not.
    assert!(offered.iter().any(|m| m.id == "claude-fable-5-3"));
    assert!(catalog.iter().any(|m| m.tier != Tier::Current));
    assert!(!offered.iter().any(|m| m.id == "claude-opus-4-6"));
}

#[test]
fn model_allowed_is_the_catalogs_check() {
    let (ops, _fake, _tmp) = ops(&[], "models-allowed");
    let s = sources(&ops);
    for ok in [
        "claude-opus-5-5",
        "claude-opus-5-5[1m]",
        "claude-sonnet-5-5-20261101",
    ] {
        assert!(s.model_allowed(ok), "{ok}");
    }
    for bad in [
        "",
        "opus",
        "claude-evil-1",
        "claude-opus-5-5 --flag",
        "--model",
    ] {
        assert!(!s.model_allowed(bad), "{bad:?}");
    }
}

#[test]
fn the_phones_context_window_is_the_catalogs() {
    let mut window = live("claude-sonnet-6");
    window.context_window = Some(300_000);
    let (ops, _fake, _tmp) = ops(&[live("claude-opus-5-6"), window], "models-window");
    let s = sources(&ops);
    let catalog = ops.catalog().catalog();
    // The data file's, and the family's for a newer Opus the live list reports.
    for model in ["claude-opus-5-5", "claude-opus-5-5[1m]", "claude-opus-5-6"] {
        assert_eq!(
            s.context_window(model),
            catalog.context_window(Tool::Claude, model),
            "{model}"
        );
        assert_eq!(s.context_window(model), Some(400_000), "{model}");
    }
    // The catalog has none for these; the live list's own is used, else none.
    assert_eq!(
        catalog.context_window(Tool::Claude, "claude-sonnet-6"),
        None
    );
    assert_eq!(s.context_window("claude-sonnet-6"), Some(300_000));
    assert_eq!(s.context_window("claude-sonnet-5-5"), None);
}

#[test]
fn a_model_id_starts_a_session_on_exactly_that_model() {
    let (ops, fake, _tmp) = ops(&[], "models-start");
    let acting = DaemonActing {
        ops: ops.clone(),
        sessions: Arc::new(Mutex::new(json!([]))),
    };
    fake.push_output(Ok(LaunchOutput::ok(
        "backgrounded · 6ce4c19c · api (idle)\n",
    )));
    acting
        .new_session_with_model(Path::new("/w/api"), "claude-fable-5-2[1m]")
        .expect("started");
    assert_eq!(
        fake.commands(),
        [
            Command::new(&["claude", "--model", "claude-fable-5-2[1m]", "--bg"])
                .in_dir("/w/api")
                .for_config_dir(None)
        ]
    );

    // The daemon's own check still refuses an id the catalog doesn't list.
    let refused = acting
        .new_session_with_model(Path::new("/w/api"), "claude-evil-1")
        .unwrap_err();
    assert!(refused.message.contains("claude-opus-5-5"), "{refused:?}");
    assert_eq!(fake.commands().len(), 1);
}
