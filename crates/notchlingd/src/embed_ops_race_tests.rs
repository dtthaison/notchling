use super::*;
use crate::embed_fetch::STOPPING;
use crate::embed_fetch_tests::{bytes, entry, file, gated, url, FakeConvert, WAIT};
use notchling_engine::search::models::fake::FakeFetch;
use notchling_engine::search::models::manifest;
use notchling_engine::search::query::category::Category;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::channel;
use std::time::Instant;

struct GatedCorpus {
    docs: Vec<(String, String)>,
    hold: AtomicBool,
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Corpus for GatedCorpus {
    fn id(&self) -> &'static str {
        "sessions"
    }

    fn tier(&self) -> CorpusTier {
        CorpusTier::Agent
    }

    fn scan(&self, _window: Option<f64>, _now: f64) -> Vec<DocStamp> {
        if self.hold.swap(false, Ordering::SeqCst) {
            if let Some(entered) = self.entered.lock().unwrap().take() {
                let _ = entered.send(());
            }
            if let Some(release) = self.release.lock().unwrap().take() {
                let _ = release.recv_timeout(WAIT);
            }
        }
        self.docs
            .iter()
            .map(|(k, _)| DocStamp {
                key: k.clone(),
                stamp: "1".to_string(),
                at: 100.0,
            })
            .collect()
    }

    fn read(&self, doc: &DocStamp, _now: f64) -> Option<Doc> {
        let text = &self.docs.iter().find(|(k, _)| *k == doc.key)?.1;
        Some(Doc {
            key: doc.key.clone(),
            title: doc.key.clone(),
            at: doc.at,
            facets: vec![("tool".to_string(), "claude".to_string())],
            chunks: vec![ChunkText {
                kind: ChunkKind::Head,
                text: text.clone(),
            }],
        })
    }
}

#[test]
fn remove_during_open_rejects_the_stale_set_and_next_tick_reopens() {
    let (rig, dir) = rig(16, false, true);
    let embed = Arc::downgrade(&rig.ops);
    let fired = Arc::new(AtomicBool::new(false));
    let once = fired.clone();
    *rig.opening.lock().unwrap() = Some(Arc::new(move || {
        if !once.swap(true, Ordering::SeqCst) {
            if let Some(embed) = embed.upgrade() {
                let gone = json!({"model": embed_paths::GRANITE_ID});
                let _ = embed.call(&app(), ops::EMBED_REMOVE, &gone);
            }
        }
    }));
    rig.ops.tick();
    assert!(rig.ops.loaded_weak().upgrade().is_none());
    assert_eq!(rig.granite_vectors(&dir), 0);

    rig.ops.tick();
    assert!(rig.ops.loaded_weak().upgrade().is_some());
    assert_eq!(rig.granite_vectors(&dir), 16);
}

#[test]
fn turning_off_during_open_leaves_nothing_seated() {
    let (rig, dir) = rig(16, false, true);
    let on = rig.on.clone();
    *rig.opening.lock().unwrap() = Some(Arc::new(move || {
        on.lock().unwrap().on = false;
    }));
    rig.ops.tick();
    assert!(rig.ops.loaded_weak().upgrade().is_none());
    assert_eq!(rig.spawn.live(), 0);
    assert_eq!(rig.granite_vectors(&dir), 0);
    assert_eq!(rig.ops.status()["state"], "off");
}

#[test]
fn refresh_during_open_seats_skips_pass_and_next_tick_passes() {
    let (rig, dir) = rig(16, false, true);
    let embed = Arc::downgrade(&rig.ops);
    let fired = Arc::new(AtomicBool::new(false));
    let once = fired.clone();
    *rig.opening.lock().unwrap() = Some(Arc::new(move || {
        if !once.swap(true, Ordering::SeqCst) {
            if let Some(embed) = embed.upgrade() {
                embed.refresh();
            }
        }
    }));
    rig.ops.tick();
    assert!(rig.ops.loaded_weak().upgrade().is_some());
    assert_eq!(rig.statics_opened.load(Ordering::SeqCst), 1);
    assert_eq!(rig.ops.status()["stored"], 0);
    assert_eq!(rig.granite_vectors(&dir), 0);

    rig.ops.tick();
    assert_eq!(rig.statics_opened.load(Ordering::SeqCst), 1);
    assert_eq!(rig.granite_vectors(&dir), 16);
}

#[test]
fn retire_stops_an_in_flight_helper_before_drop_and_records_no_error() {
    let (rig, dir) = rig(40, false, true);
    rig.spawn.delay(Duration::from_secs(30));
    let (entered_tx, entered_rx) = channel();
    let entered_tx = Mutex::new(Some(entered_tx));
    let embed = Arc::downgrade(&rig.ops);
    rig.at_request_one(move || {
        if let Some(embed) = embed.upgrade() {
            if let Some(tx) = entered_tx.lock().unwrap().take() {
                let _ = tx.send(embed.loaded_weak().upgrade().unwrap());
            }
        }
    });
    let ticking = rig.ops.clone();
    let worker = std::thread::spawn(move || ticking.tick());

    let held = entered_rx.recv_timeout(WAIT).unwrap();
    let gone = json!({"model": embed_paths::GRANITE_ID});
    rig.ops.call(&app(), ops::EMBED_REMOVE, &gone).unwrap();
    worker.join().unwrap();

    let started = Instant::now();
    while started.elapsed() < WAIT && rig.spawn.live() != 0 {
        std::thread::yield_now();
    }
    assert_eq!(rig.spawn.live(), 0);
    assert_eq!(Arc::strong_count(&held), 1);
    assert!(rig.ops.status()["error"].is_null());
    assert_eq!(rig.granite_vectors(&dir), 0);
    drop(held);
}

#[test]
fn remove_waits_for_an_in_flight_pass_before_dropping_vectors() {
    let dir = TempDir::new("embed-pass-race");
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let corpus = Arc::new(GatedCorpus {
        docs: (0..8)
            .map(|i| (format!("d{i}"), format!("session note {i}")))
            .collect(),
        hold: AtomicBool::new(true),
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(Some(release_rx)),
    });
    let current = Arc::new(Mutex::new(settings(true)));
    let read = current.clone();
    let ops = EmbedOps::with_parts(EmbedParts {
        data_dir: dir.root().to_path_buf(),
        room: Arc::new(Events::default()),
        settings: Arc::new(move || read.lock().unwrap().clone()),
        corpora: Arc::new(move |_| vec![corpus.clone() as Arc<dyn Corpus>]),
        spawn: Arc::new(FakeSpawn::new(&ModelId(GRANITE.to_string()))),
        granite_id: ModelId(GRANITE.to_string()),
        granite_present: false,
        statics: Arc::new(|| Ok(None)),
        fixture: true,
        power: Arc::new(PluggedIn),
        net: ModelNet::offline(),
        now: Arc::new(|| 1000.0),
    });

    let pass_ops = ops.clone();
    let pass_thread = std::thread::spawn(move || pass_ops.tick());
    entered_rx.recv_timeout(WAIT).unwrap();
    let held = ops.loaded_weak().upgrade().unwrap();
    assert!(held.passing());
    assert!(!held.retired());

    let rm_ops = ops.clone();
    let (rm_tx, rm_rx) = channel();
    std::thread::spawn(move || {
        let out = rm_ops.call(
            &app(),
            ops::EMBED_REMOVE,
            &json!({"model": embed_paths::STATIC_ID}),
        );
        let _ = rm_tx.send(out);
    });

    let started = Instant::now();
    while started.elapsed() < WAIT && ops.loaded_weak().upgrade().is_some() {
        std::thread::yield_now();
    }
    assert!(ops.loaded_weak().upgrade().is_none());
    assert!(rm_rx.try_recv().is_err());

    release_tx.send(()).unwrap();
    let removed = rm_rx.recv_timeout(WAIT).unwrap().unwrap();
    pass_thread.join().unwrap();
    assert!(removed.get("removed").is_some());
    assert!(held.retired());
    assert!(!held.passing());

    let db = VectorDb::open(&embed_paths::vectors_db(dir.root())).unwrap();
    let static_embedded: usize = db
        .coverage(&ModelId(embed_paths::STATIC_ID.to_string()))
        .iter()
        .map(|c| c.embedded)
        .sum();
    assert_eq!(static_embedded, 0);
}

#[test]
fn failed_fetcher_remove_refreshes_so_the_next_tick_reopens() {
    let model = &manifest().models[0];
    let first = &model.files[0];
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&first.url(), &bytes(51, 4000));
    let (gate, saw, release) = gated(fake);
    let net = ModelNet {
        fetch: gate,
        convert: Arc::new(FakeConvert),
    };
    let (rig, _dir) = rig_net(4, true, true, net);
    rig.ops.tick();
    let seated = rig.ops.loaded_weak();
    assert!(seated.upgrade().is_some());

    let args = json!({"model": model.id});
    rig.ops.call(&app(), ops::EMBED_FETCH, &args).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    rig.ops.set_stop_wait(Duration::ZERO);
    let stirs_before = rig.ops.stirs();

    let err = rig.ops.call(&app(), ops::EMBED_REMOVE, &args).unwrap_err();
    assert_eq!(err.message, STOPPING);
    assert!(seated.upgrade().is_none());
    assert!(rig.ops.stirs() > stirs_before);

    let _ = rig.ops.call(&app(), ops::EMBED_FETCH_CANCEL, &args);
    release.send(()).unwrap();
    assert!(rig.ops.fetcher().wait_idle(WAIT));

    rig.ops.tick();
    assert!(rig.ops.loaded_weak().upgrade().is_some());
}

#[test]
fn cancelled_download_keeps_loaded_set_while_completed_download_reloads() {
    let model = &manifest().models[0];
    let first_file = &model.files[0];
    let ok_bytes = bytes(52, 2000);
    let ok_file = file("w.bin", &ok_bytes);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&first_file.url(), &bytes(53, 4000));
    fake.serve(&url(&ok_file), &ok_bytes);
    let (gate, saw, release) = gated(fake);
    let net = ModelNet {
        fetch: gate,
        convert: Arc::new(FakeConvert),
    };
    let (rig, _dir) = rig_net(16, false, true, net);
    rig.ops.tick();
    let initial = rig.ops.loaded_weak().upgrade().unwrap();
    let spawns = rig.spawn.spawns();
    assert_eq!(spawns, 1);

    let args = json!({"model": model.id});
    rig.ops.call(&app(), ops::EMBED_FETCH, &args).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    let cancelled = rig.ops.call(&app(), ops::EMBED_FETCH_CANCEL, &args);
    assert_eq!(cancelled.unwrap()["cancelled"], true);
    release.send(()).unwrap();
    assert!(rig.ops.fetcher().wait_idle(WAIT));

    rig.ops.tick();
    let after_cancel = rig.ops.loaded_weak().upgrade().unwrap();
    assert!(Arc::ptr_eq(&initial, &after_cancel));
    assert_eq!(rig.spawn.spawns(), spawns);

    let good = entry("t@ops-ok", "t-ops-ok", vec![ok_file]);
    let since = rig.ops.stirs();
    rig.ops.fetcher().start(good).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    release.send(()).unwrap();
    assert!(rig.ops.fetcher().wait_idle(WAIT));
    assert!(rig.ops.rest(since, WAIT));

    rig.ops.tick();
    let after_ok = rig.ops.loaded_weak().upgrade().unwrap();
    assert!(!Arc::ptr_eq(&initial, &after_ok));
}

#[test]
fn helper_recovery_clears_the_recorded_error() {
    let (rig, _dir) = rig(16, false, true);
    rig.spawn.die_after(0);
    rig.ops.tick();
    assert!(!rig.ops.status()["error"].is_null());

    rig.spawn.die_after(usize::MAX);
    rig.ops.tick();
    assert!(rig.ops.status()["error"].is_null());
    assert_eq!(rig.ops.status()["corpora"][0]["embedded"][GRANITE], 16);
}

#[test]
fn turning_off_clears_an_open_error_even_with_nothing_seated() {
    let (rig, dir) = rig(4, true, true);
    std::fs::create_dir_all(embed_paths::vectors_db(dir.root())).unwrap();
    rig.ops.tick();
    assert!(rig.ops.loaded_weak().upgrade().is_none());
    assert!(!rig.ops.status()["error"].is_null());

    rig.on.lock().unwrap().on = false;
    rig.ops.tick();
    assert_eq!(rig.ops.status()["state"], "off");
    assert!(rig.ops.status()["error"].is_null());
}

#[test]
fn coverage_change_inside_one_second_and_backwards_clock_both_publish() {
    let (rig, _dir) = rig(1, true, true);
    rig.on.lock().unwrap().categories.push(Category::Settings);
    rig.ops.tick();
    let base = count_changed(&rig);
    rig.ops.publish_changed();
    assert_eq!(count_changed(&rig), base);

    let docs = json!({"docs": [{"id": "k1", "text": "appearance theme"}]});
    rig.ops.call(&app(), ops::EMBED_SETTINGS, &docs).unwrap();
    rig.ops.tick();
    assert_eq!(count_changed(&rig), base + 1);
    rig.ops.publish_changed();
    assert_eq!(count_changed(&rig), base + 1);

    *rig.clock.lock().unwrap() -= 10.0;
    rig.ops.publish_changed();
    assert_eq!(count_changed(&rig), base + 2);
}

fn rig_statics(
    statics: crate::embed_ops::StaticOpen,
) -> (Arc<EmbedOps>, Arc<Mutex<EmbedSettings>>, TempDir) {
    let dir = TempDir::new("embed-statics-open");
    let current = Arc::new(Mutex::new(settings(true)));
    let read = current.clone();
    let ops = EmbedOps::with_parts(EmbedParts {
        data_dir: dir.root().to_path_buf(),
        room: Arc::new(Events::default()),
        settings: Arc::new(move || read.lock().unwrap().clone()),
        corpora: Arc::new(|_| {
            vec![Arc::new(Notes {
                docs: vec![("s0".into(), "hello world".into())],
            }) as Arc<dyn Corpus>]
        }),
        spawn: Arc::new(FakeSpawn::new(&ModelId(GRANITE.to_string()))),
        granite_id: ModelId(GRANITE.to_string()),
        granite_present: false,
        statics,
        fixture: false,
        power: Arc::new(PluggedIn),
        net: ModelNet::offline(),
        now: Arc::new(|| 1000.0),
    });
    (ops, current, dir)
}

#[test]
fn statics_open_error_is_reported_while_absent_is_null_and_off_clears_it() {
    let problem = "The static model could not be opened (bad header).";
    let (ops_err, on_err, _dir_err) = rig_statics(Arc::new(move || Err(problem.to_string())));
    ops_err.tick();
    let status = ops_err.status();
    assert_eq!(status["error"], problem);
    assert_ne!(status["state"], "off");

    let (ops_none, _on_none, _dir_none) = rig_statics(Arc::new(|| Ok(None)));
    ops_none.tick();
    assert!(ops_none.status()["error"].is_null());

    on_err.lock().unwrap().on = false;
    ops_err.tick();
    assert_eq!(ops_err.status()["state"], "off");
    assert!(ops_err.status()["error"].is_null());
    assert!(ops_err.loaded_weak().upgrade().is_none());
}

#[test]
fn a_loaded_set_seated_between_retire_and_drop_model_is_retired_and_replaced() {
    let dir = TempDir::new("embed-remove-window");
    let static_folder = embed_paths::static_dir(dir.root());
    std::fs::create_dir_all(&static_folder).unwrap();
    std::fs::write(static_folder.join("marker"), b"1").unwrap();

    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let corpus = Arc::new(GatedCorpus {
        docs: (0..8)
            .map(|i| (format!("d{i}"), format!("session note {i}")))
            .collect(),
        hold: AtomicBool::new(false),
        entered: Mutex::new(Some(entered_tx)),
        release: Mutex::new(Some(release_rx)),
    });
    let current = Arc::new(Mutex::new(settings(true)));
    let read = current.clone();
    let root = dir.root().to_path_buf();
    let ops = EmbedOps::with_parts(EmbedParts {
        data_dir: dir.root().to_path_buf(),
        room: Arc::new(Events::default()),
        settings: Arc::new(move || read.lock().unwrap().clone()),
        corpora: Arc::new({
            let corpus = corpus.clone();
            move |_| vec![corpus.clone() as Arc<dyn Corpus>]
        }),
        spawn: Arc::new(FakeSpawn::new(&ModelId(GRANITE.to_string()))),
        granite_id: ModelId(GRANITE.to_string()),
        granite_present: true,
        statics: Arc::new(move || {
            if embed_paths::static_dir(&root).exists() {
                let embedder = HashEmbedder::new(embed_paths::STATIC_ID);
                Ok(Some(Arc::new(embedder) as Arc<dyn Embedder>))
            } else {
                Ok(None)
            }
        }),
        fixture: false,
        power: Arc::new(PluggedIn),
        net: ModelNet::offline(),
        now: Arc::new(|| 1000.0),
    });

    ops.tick();
    assert_eq!(
        ops.status()["corpora"][0]["embedded"][embed_paths::STATIC_ID],
        8
    );

    corpus.hold.store(true, Ordering::SeqCst);
    let pass_ops = ops.clone();
    let pass_thread = std::thread::spawn(move || pass_ops.tick());
    entered_rx.recv_timeout(WAIT).unwrap();
    let l1 = ops.loaded_weak().upgrade().unwrap();
    assert!(l1.passing());

    let rm_ops = ops.clone();
    let (rm_tx, rm_rx) = channel();
    std::thread::spawn(move || {
        let out = rm_ops.call(
            &app(),
            ops::EMBED_REMOVE,
            &json!({"model": embed_paths::STATIC_ID}),
        );
        let _ = rm_tx.send(out);
    });

    let started = Instant::now();
    for _ in 0..10_000_000 {
        if ops.epoch() >= 2 || started.elapsed() >= WAIT {
            break;
        }
        std::thread::yield_now();
    }
    assert_eq!(ops.epoch(), 2);
    assert!(ops.loaded_weak().upgrade().is_none());
    assert!(!static_folder.exists());
    assert!(rm_rx.try_recv().is_err());

    ops.tick();
    let l3 = ops.loaded_weak().upgrade().unwrap();
    assert!(!Arc::ptr_eq(&l1, &l3));

    release_tx.send(()).unwrap();
    let removed = rm_rx.recv_timeout(WAIT).unwrap().unwrap();
    pass_thread.join().unwrap();
    assert_eq!(removed["removed"], true);
    assert!(l3.retired());
    assert!(ops.loaded_weak().upgrade().is_none());

    ops.tick();
    let l4 = ops.loaded_weak().upgrade().unwrap();
    assert!(!Arc::ptr_eq(&l3, &l4));
    assert!(ops.status()["corpora"][0]["embedded"]
        .get(embed_paths::STATIC_ID)
        .is_none_or(|n| n == 0));
    let db = VectorDb::open(&embed_paths::vectors_db(dir.root())).unwrap();
    let static_embedded: usize = db
        .coverage(&ModelId(embed_paths::STATIC_ID.to_string()))
        .iter()
        .map(|c| c.embedded)
        .sum();
    assert_eq!(static_embedded, 0);
}

#[test]
fn a_successful_query_clears_a_recorded_helper_error_without_a_tick() {
    let (rig, _dir) = rig(16, false, true);
    rig.spawn.die_after(0);
    rig.ops.tick();
    assert!(!rig.ops.status()["error"].is_null());

    rig.spawn.die_after(usize::MAX);
    let _ = rig
        .ops
        .query_lists("note about indexing", &Filter::default(), &["sessions"]);
    let started = Instant::now();
    for _ in 0..10_000_000 {
        if rig.ops.state() != Semantic::Warming || started.elapsed() >= WAIT {
            break;
        }
        std::thread::yield_now();
    }
    assert_ne!(rig.ops.state(), Semantic::Warming);
    assert!(!rig.ops.status()["error"].is_null());

    let (_, lists) = rig
        .ops
        .query_lists("note about indexing", &Filter::default(), &["sessions"]);
    assert!(lists["sessions"].granite.is_some());
    assert!(rig.ops.status()["error"].is_null());
}

#[test]
fn an_idle_tick_with_a_ready_helper_clears_a_recorded_error() {
    let (rig, _dir) = rig(4, false, true);
    rig.ops.tick();
    *rig.clock.lock().unwrap() += 301.0;
    rig.spawn.die_after(0);
    rig.ops.tick();
    assert!(!rig.ops.status()["error"].is_null());

    rig.spawn.die_after(usize::MAX);
    let _ = rig
        .ops
        .query_lists("warming probe", &Filter::default(), &["sessions"]);
    let started = Instant::now();
    for _ in 0..10_000_000 {
        if rig.ops.state() != Semantic::Warming || started.elapsed() >= WAIT {
            break;
        }
        std::thread::yield_now();
    }
    assert_ne!(rig.ops.state(), Semantic::Warming);
    assert!(!rig.ops.status()["error"].is_null());

    let requests_before = rig.spawn.requests();
    rig.ops.tick();
    assert_eq!(rig.spawn.requests(), requests_before);
    assert!(rig.ops.status()["error"].is_null());
}

fn wait_while_warming(rig: &Rig) {
    let started = Instant::now();
    for _ in 0..10_000_000 {
        if rig.ops.state() != Semantic::Warming || started.elapsed() >= WAIT {
            break;
        }
        std::thread::yield_now();
    }
    assert_ne!(rig.ops.state(), Semantic::Warming);
}

fn wait_for_ready_helper(rig: &Rig) {
    let started = Instant::now();
    while !rig.ops.helper_ready() && started.elapsed() < WAIT {
        std::thread::yield_now();
    }
    assert!(rig.ops.helper_ready());
}

#[test]
fn a_cached_query_does_not_clear_a_helper_error_while_the_helper_is_down() {
    let (rig, _dir) = rig(4, false, true);
    rig.ops.tick();
    let probe = |rig: &Rig| {
        rig.ops
            .query_lists("cached probe", &Filter::default(), &["sessions"])
    };
    let _ = probe(&rig);
    wait_while_warming(&rig);
    let (_, lists) = probe(&rig);
    assert!(lists["sessions"].granite.is_some());

    rig.spawn.exit_all();
    rig.spawn.die_after(0);
    *rig.clock.lock().unwrap() += 301.0;
    rig.ops.tick();
    assert!(!rig.ops.status()["error"].is_null());

    let (_, lists) = probe(&rig);
    assert!(lists["sessions"].granite.is_some());
    assert!(!rig.ops.status()["error"].is_null());
}

#[test]
fn a_paused_tick_with_a_ready_helper_clears_a_recorded_error() {
    let (rig, _dir) = rig(4, false, true);
    rig.ops.tick();
    *rig.clock.lock().unwrap() += 301.0;
    rig.spawn.die_after(0);
    rig.ops.tick();
    assert!(!rig.ops.status()["error"].is_null());

    rig.on.lock().unwrap().paused = true;
    rig.spawn.die_after(usize::MAX);
    let _ = rig
        .ops
        .query_lists("warming probe", &Filter::default(), &["sessions"]);
    wait_for_ready_helper(&rig);
    assert!(!rig.ops.status()["error"].is_null());

    rig.ops.tick();
    assert_eq!(rig.ops.status()["state"], "paused");
    assert!(rig.ops.status()["error"].is_null());
}
