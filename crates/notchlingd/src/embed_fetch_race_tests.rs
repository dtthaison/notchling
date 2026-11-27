use super::*;
use crate::embed_fetch::{REMOVING, STOPPING};

#[derive(Default)]
struct CountConvert {
    calls: AtomicUsize,
}

impl Converter for CountConvert {
    fn convert(&self, dir: &Path, _: &AtomicBool) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(dir.join(FLAT).is_file());
        std::fs::write(dir.join(TABLE), b"table").map_err(|e| e.to_string())?;
        Ok(sha256_hex(b"table"))
    }
}

#[cfg(unix)]
#[test]
fn models_symlink_is_refused_without_touching_outside_or_converting() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-models-link");
    let flat = bytes(31, 4000);
    let tok = bytes(32, 800);
    let (a, b) = (file(FLAT, &flat), file("tokenizer.json", &tok));
    let outside = tmp.root().join("outside");
    let target = outside.join("static-mrl");
    std::fs::create_dir_all(&target).unwrap();
    std::fs::write(target.join(FLAT), &flat).unwrap();
    std::fs::write(target.join("tokenizer.json"), &tok).unwrap();
    std::fs::write(target.join(TABLE), b"table").unwrap();
    std::os::unix::fs::symlink(&outside, tmp.root().join("models")).unwrap();
    assert!(!embed_paths::static_present(tmp.root()));
    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&a), &flat).serve(&url(&b), &tok);
    let convert = Arc::new(CountConvert::default());
    let (fetcher, seen) = fetcher_with(tmp.root(), fake.clone(), convert.clone());
    fetcher
        .start(entry("s@m-link", "static-mrl", vec![a, b]))
        .unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "failed");
    assert_eq!(convert.calls.load(Ordering::SeqCst), 0);
    assert!(fake.requests().is_empty());
    assert_eq!(std::fs::read(target.join(FLAT)).unwrap(), flat);
    assert!(!target.join(TABLE_HASH).exists());
}

#[cfg(unix)]
#[test]
fn linked_model_folder_with_matching_files_is_refused_before_convert() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-folder-link");
    let flat = bytes(33, 4000);
    let tok = bytes(34, 800);
    let (a, b) = (file(FLAT, &flat), file("tokenizer.json", &tok));
    let outside = tmp.root().join("outside-folder");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join(FLAT), &flat).unwrap();
    std::fs::write(outside.join("tokenizer.json"), &tok).unwrap();
    std::fs::write(outside.join("config.json"), b"{}").unwrap();
    std::fs::write(outside.join("model.safetensors"), b"weights").unwrap();
    std::fs::create_dir_all(tmp.root().join("models")).unwrap();
    std::os::unix::fs::symlink(&outside, tmp.root().join("models/static-mrl")).unwrap();
    std::os::unix::fs::symlink(&outside, tmp.root().join("models/granite-97m-r2")).unwrap();
    assert!(!embed_paths::static_present(tmp.root()));
    assert!(!embed_paths::granite_present(tmp.root()));
    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&a), &flat).serve(&url(&b), &tok);
    let convert = Arc::new(CountConvert::default());
    let (fetcher, seen) = fetcher_with(tmp.root(), fake.clone(), convert.clone());
    fetcher
        .start(entry("s@f-link", "static-mrl", vec![a, b]))
        .unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "failed");
    assert_eq!(convert.calls.load(Ordering::SeqCst), 0);
    assert!(fake.requests().is_empty());
    assert_eq!(std::fs::read(outside.join(FLAT)).unwrap(), flat);
    assert!(!outside.join(TABLE).exists());
    assert!(!outside.join(TABLE_HASH).exists());
}

#[cfg(unix)]
#[test]
fn remove_with_linked_models_or_folder_never_touches_outside() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-remove-models-link");
    let outside = tmp.root().join("outside-models");
    std::fs::create_dir_all(outside.join("t-keep")).unwrap();
    std::fs::write(outside.join("t-keep/keep.txt"), b"keep").unwrap();
    std::os::unix::fs::symlink(&outside, tmp.root().join("models")).unwrap();
    let (fetcher, _seen) = fetcher(tmp.root(), Arc::new(FakeFetch::new()));
    let model = entry("t@rm-m", "t-keep", Vec::new());
    assert!(!fetcher.remove(model, WAIT).unwrap());
    assert_eq!(
        std::fs::read(outside.join("t-keep/keep.txt")).unwrap(),
        b"keep"
    );
    assert!(std::fs::symlink_metadata(tmp.root().join("models"))
        .unwrap()
        .file_type()
        .is_symlink());

    std::fs::remove_file(tmp.root().join("models")).unwrap();
    std::fs::create_dir_all(tmp.root().join("models")).unwrap();
    let link = tmp.root().join("models/t-keep");
    std::os::unix::fs::symlink(outside.join("t-keep"), &link).unwrap();
    assert!(fetcher.remove(model, WAIT).unwrap());
    assert!(std::fs::symlink_metadata(&link).is_err());
    assert_eq!(
        std::fs::read(outside.join("t-keep/keep.txt")).unwrap(),
        b"keep"
    );
}

#[cfg(unix)]
#[test]
fn planted_table_symlink_is_unlinked_and_outside_file_untouched() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-table-link");
    let flat = bytes(35, 4000);
    let tok = bytes(36, 800);
    let (a, b) = (file(FLAT, &flat), file("tokenizer.json", &tok));
    let dir = tmp.root().join("models/static-mrl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tokenizer.json"), &tok).unwrap();
    let outside = tmp.root().join("outside-table.bin");
    std::fs::write(&outside, b"untouched").unwrap();
    std::os::unix::fs::symlink(&outside, dir.join(TABLE)).unwrap();
    std::fs::write(dir.join(TABLE_HASH), sha256_hex(b"untouched")).unwrap();
    assert!(!embed_paths::static_present(tmp.root()));

    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&a), &flat).serve(&url(&b), &tok);
    let convert = Arc::new(CountConvert::default());
    let (fetcher, seen) = fetcher_with(tmp.root(), fake.clone(), convert.clone());
    fetcher
        .start(entry("s@t-link", "static-mrl", vec![a, b]))
        .unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "ready");
    assert_eq!(convert.calls.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(&outside).unwrap(), b"untouched");
    assert!(!std::fs::symlink_metadata(dir.join(TABLE))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(dir.join(TABLE)).unwrap(), b"table");
    assert!(embed_paths::static_present(tmp.root()));
}

#[test]
fn corrupt_table_with_flat_on_disk_reconverts_and_updates_hash() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-table-corrupt");
    let flat = bytes(37, 4000);
    let tok = bytes(38, 800);
    let (a, b) = (file(FLAT, &flat), file("tokenizer.json", &tok));
    let dir = tmp.root().join("models/static-mrl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(FLAT), &flat).unwrap();
    std::fs::write(dir.join("tokenizer.json"), &tok).unwrap();
    std::fs::write(dir.join(TABLE), b"corrupt-bytes").unwrap();
    std::fs::write(dir.join(TABLE_HASH), sha256_hex(b"table")).unwrap();

    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&a), &flat).serve(&url(&b), &tok);
    let convert = Arc::new(CountConvert::default());
    let (fetcher, seen) = fetcher_with(tmp.root(), fake.clone(), convert.clone());
    fetcher
        .start(entry("s@corrupt", "static-mrl", vec![a, b]))
        .unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "ready");
    assert!(fake.requests().is_empty());
    assert_eq!(convert.calls.load(Ordering::SeqCst), 1);
    assert_eq!(std::fs::read(dir.join(TABLE)).unwrap(), b"table");
    assert_eq!(
        std::fs::read_to_string(dir.join(TABLE_HASH)).unwrap(),
        sha256_hex(b"table")
    );
    assert!(!dir.join(FLAT).exists());
}

#[test]
fn table_without_hash_file_fetches_flat_and_reconverts() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-table-nohash");
    let flat = bytes(39, 4000);
    let tok = bytes(40, 800);
    let (a, b) = (file(FLAT, &flat), file("tokenizer.json", &tok));
    let dir = tmp.root().join("models/static-mrl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tokenizer.json"), &tok).unwrap();
    std::fs::write(dir.join(TABLE), b"table").unwrap();

    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&a), &flat).serve(&url(&b), &tok);
    let convert = Arc::new(CountConvert::default());
    let (fetcher, seen) = fetcher_with(tmp.root(), fake.clone(), convert.clone());
    fetcher
        .start(entry("s@nohash", "static-mrl", vec![a, b]))
        .unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "ready");
    assert_eq!(fake.requests().len(), 1);
    assert_eq!(convert.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        std::fs::read_to_string(dir.join(TABLE_HASH)).unwrap(),
        sha256_hex(b"table")
    );
    assert!(!dir.join(FLAT).exists());
}

#[test]
fn valid_table_and_matching_hash_skip_fetch_and_convert_and_remove_flat() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-table-ready");
    let flat = bytes(41, 4000);
    let tok = bytes(42, 800);
    let (a, b) = (file(FLAT, &flat), file("tokenizer.json", &tok));
    let dir = tmp.root().join("models/static-mrl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(FLAT), &flat).unwrap();
    std::fs::write(dir.join("tokenizer.json"), &tok).unwrap();
    std::fs::write(dir.join(TABLE), b"table").unwrap();
    let hash_line = format!("{}\n", sha256_hex(b"table"));
    std::fs::write(dir.join(TABLE_HASH), hash_line).unwrap();

    let fake = Arc::new(FakeFetch::new());
    let convert = Arc::new(CountConvert::default());
    let (fetcher, seen) = fetcher_with(tmp.root(), fake.clone(), convert.clone());
    fetcher
        .start(entry("s@ready", "static-mrl", vec![a, b]))
        .unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "ready");
    assert!(fake.requests().is_empty());
    assert_eq!(convert.calls.load(Ordering::SeqCst), 0);
    assert!(!dir.join(FLAT).exists());
}

#[test]
fn a_timed_out_remove_keeps_start_blocked_for_a_second_remover() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-two-removers");
    let data = bytes(43, 4000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (live, saw) = live(fake);
    let (cancelled_tx, cancelled_rx) = channel();
    let (release_tx, release_rx) = channel();
    let release_rx = Mutex::new(release_rx);
    *live.cancelled.lock().unwrap() = Some(Box::new(move || {
        let _ = cancelled_tx.send(());
        let _ = release_rx.lock().unwrap().recv_timeout(WAIT);
    }));
    let (fetcher, seen) = fetcher(tmp.root(), live);
    let model = entry("t@two-rm", "t-two-rm", vec![f]);
    fetcher.start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();

    let long_fetcher = fetcher.clone();
    let (long_tx, long_rx) = channel();
    std::thread::spawn(move || {
        let _ = long_tx.send(long_fetcher.remove(model, WAIT));
    });
    cancelled_rx.recv_timeout(WAIT).unwrap();

    let short = fetcher.remove(model, Duration::ZERO).unwrap_err();
    assert_eq!(short.message, STOPPING);
    let blocked = fetcher.start(model).unwrap_err();
    assert_eq!(blocked.message, REMOVING);
    assert!(seen.outcomes.lock().unwrap().is_empty());
    assert!(seen
        .events
        .lock()
        .unwrap()
        .iter()
        .all(|e| e["state"] != "cancelled"));

    release_tx.send(()).unwrap();
    assert!(long_rx.recv_timeout(WAIT).unwrap().unwrap());
    assert!(!tmp.root().join("models/t-two-rm").exists());
    assert!(seen.outcomes.lock().unwrap().is_empty());
    assert!(seen
        .events
        .lock()
        .unwrap()
        .iter()
        .all(|e| e["state"] != "cancelled"));
}

#[test]
fn finished_passes_true_only_when_the_download_succeeded() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-finished-flag");
    let data = bytes(44, 3000);
    let f_cancel = file("w.bin", &data);
    let fake_cancel = FakeFetch::new();
    fake_cancel.chunk(1000).serve(&url(&f_cancel), &data);
    let (gate, saw, release) = gated(fake_cancel);
    let (fetcher_c, seen_c) = fetcher(tmp.root(), gate);
    let m_cancel = entry("t@fin-c", "t-fin-c", vec![f_cancel]);
    fetcher_c.start(m_cancel).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    assert!(fetcher_c.cancel("t@fin-c"));
    release.send(()).unwrap();
    seen_c.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(
        *seen_c.outcomes.lock().unwrap(),
        vec![("t@fin-c".to_string(), false)]
    );

    let f_fail = file("w.bin", &data);
    let fake_fail = FakeFetch::new();
    fake_fail.status(&url(&f_fail), 503);
    let (fetcher_f, seen_f) = fetcher(tmp.root(), Arc::new(fake_fail));
    fetcher_f
        .start(entry("t@fin-f", "t-fin-f", vec![f_fail]))
        .unwrap();
    seen_f.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(
        *seen_f.outcomes.lock().unwrap(),
        vec![("t@fin-f".to_string(), false)]
    );

    let f_ok = file("w.bin", &data);
    let fake_ok = FakeFetch::new();
    fake_ok.serve(&url(&f_ok), &data);
    let (fetcher_ok, seen_ok) = fetcher(tmp.root(), Arc::new(fake_ok));
    fetcher_ok
        .start(entry("t@fin-ok", "t-fin-ok", vec![f_ok]))
        .unwrap();
    seen_ok.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(
        *seen_ok.outcomes.lock().unwrap(),
        vec![("t@fin-ok".to_string(), true)]
    );
}

fn gated_finished_fetcher(
    dir: &Path,
    fetch: Arc<dyn Fetch>,
) -> (Arc<Fetcher>, Arc<AtomicUsize>, Inbox<()>, Sender<()>) {
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let (entered_tx, entered_rx) = channel();
    let (release_tx, release_rx) = channel();
    let entered = Mutex::new(Some(entered_tx));
    let release = Mutex::new(Some(release_rx));
    let net = ModelNet {
        fetch,
        convert: Arc::new(FakeConvert),
    };
    let fetcher = Arc::new(Fetcher::new(
        dir.to_path_buf(),
        net,
        Arc::new(|_| {}),
        Arc::new(move |_, _| {
            counted.fetch_add(1, Ordering::SeqCst);
            if let Some(tx) = entered.lock().unwrap().take() {
                let _ = tx.send(());
            }
            if let Some(rx) = release.lock().unwrap().take() {
                let _ = rx.recv_timeout(WAIT);
            }
        }),
    ));
    (fetcher, calls, entered_rx, release_tx)
}

#[test]
fn wait_idle_waits_for_announce_and_finished() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-wait-finished");
    let data = bytes(45, 2000);
    let f = file("w.bin", &data);
    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&f), &data);
    let (fetcher, calls, entered, release) = gated_finished_fetcher(tmp.root(), fake);
    let model = entry("t@wait-cb", "t-wait-cb", vec![f]);

    fetcher.start(model).unwrap();
    entered.recv_timeout(WAIT).unwrap();
    assert!(!fetcher.wait_idle(Duration::ZERO));
    assert!(fetcher.running());
    assert_eq!(fetcher.start(model).unwrap_err().message, STOPPING);

    release.send(()).unwrap();
    assert!(fetcher.wait_idle(WAIT));
    assert!(!fetcher.running());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(fetcher.list()[0]["state"], "ready");
}

#[test]
fn a_remover_arriving_while_the_job_announces_keeps_start_blocked_until_it_deleted() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-remove-announcing");
    let data = bytes(46, 2000);
    let f = file("w.bin", &data);
    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&f), &data);
    let (fetcher, _calls, entered, release) = gated_finished_fetcher(tmp.root(), fake.clone());
    let model = entry("t@rm-cb", "t-rm-cb", vec![f]);

    fetcher.start(model).unwrap();
    entered.recv_timeout(WAIT).unwrap();

    let rm_fetcher = fetcher.clone();
    let (rm_tx, rm_rx) = channel();
    std::thread::spawn(move || {
        let _ = rm_tx.send(rm_fetcher.remove(model, WAIT));
    });

    let started = Instant::now();
    let mut registered = false;
    for _ in 0..10_000_000 {
        match fetcher.start(model) {
            Err(e) if e.message == REMOVING => {
                registered = true;
                break;
            }
            Err(e) => assert_eq!(e.message, STOPPING),
            Ok(()) => panic!("start succeeded while job was announcing"),
        }
        if started.elapsed() >= WAIT {
            break;
        }
        std::thread::yield_now();
    }
    assert!(registered);
    let requests_before = fake.requests().len();

    release.send(()).unwrap();
    assert!(rm_rx.recv_timeout(WAIT).unwrap().unwrap());
    assert!(!tmp.root().join("models/t-rm-cb").exists());
    assert_eq!(fake.requests().len(), requests_before);

    fetcher.start(model).unwrap();
    assert!(fetcher.wait_idle(WAIT));
    assert!(tmp.root().join("models/t-rm-cb/w.bin").is_file());
}

#[test]
fn static_present_checks_the_table_checksum_and_rechecks_when_the_stamp_changes() {
    use std::io::{Seek, SeekFrom, Write};
    let tmp = crate::limits_ops_tests::TempDir::new("static-present-stamp");
    let dir = tmp.root().join("models/static-mrl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("tokenizer.json"), b"{}").unwrap();
    let table_path = dir.join(TABLE);
    let hash_path = dir.join(TABLE_HASH);
    std::fs::write(&table_path, b"good-table-1").unwrap();

    assert!(!embed_paths::static_present(tmp.root()));

    std::fs::write(&hash_path, sha256_hex(b"other-bytes!")).unwrap();
    assert!(!embed_paths::static_present(tmp.root()));

    std::fs::write(&hash_path, sha256_hex(b"good-table-1")).unwrap();
    assert!(embed_paths::static_present(tmp.root()));
    assert!(embed_paths::static_present(tmp.root()));

    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&table_path)
        .unwrap();
    let before = file.metadata().unwrap().modified().unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    file.write_all(b"bad!-table-1").unwrap();
    file.flush().unwrap();
    file.set_modified(before + Duration::from_secs(60)).unwrap();
    drop(file);
    assert!(!embed_paths::static_present(tmp.root()));

    let hash_line = format!("{}\n", sha256_hex(b"bad!-table-1"));
    std::fs::write(&hash_path, hash_line).unwrap();
    assert!(embed_paths::static_present(tmp.root()));
}
