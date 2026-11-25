use crate::embed_fetch::{
    progress_json, Converter, Fetcher, ModelNet, ProcessConvert, FLAT, TABLE, TABLE_HASH,
};
use crate::embed_ops_tests::rig_net;
use crate::embed_paths;
use crate::op_group::OpGroup;
use notchling_engine::search::models::fake::FakeFetch;
use notchling_engine::search::models::{
    manifest, Fetch, Head, ModelEntry, ModelFile, Receiver, CANCELLED,
};
use notchling_local_api::{ops, Caller, ClientKind, ErrorCode, OpSpec, Tier};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver as Inbox, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub(crate) const WAIT: Duration = Duration::from_secs(20);

fn app() -> Caller {
    Caller {
        tier: Tier::App,
        client: ClientKind::Cli,
        peer: None,
        pid: None,
    }
}

pub(crate) fn bytes(seed: u8, n: usize) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_add(seed)).collect()
}

fn sha256_hex(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

pub(crate) fn file(save_as: &str, data: &[u8]) -> ModelFile {
    ModelFile {
        repo: "o/m".into(),
        revision: "0123456789012345678901234567890123456789".into(),
        path: save_as.into(),
        save_as: save_as.into(),
        size: data.len() as u64,
        sha256: sha256_hex(data),
        license: "Apache-2.0".into(),
    }
}

pub(crate) fn url(f: &ModelFile) -> String {
    f.url()
}

pub(crate) fn entry(id: &str, folder: &str, files: Vec<ModelFile>) -> &'static ModelEntry {
    let total = files.iter().map(|f| f.size).sum();
    Box::leak(Box::new(ModelEntry {
        id: id.into(),
        folder: folder.into(),
        tier: "standard".into(),
        listed: true,
        recommended: false,
        message: "x".into(),
        download_bytes: total,
        kept_bytes: total,
        files,
    }))
}

pub(crate) struct Gate {
    pub(crate) inner: FakeFetch,
    entered: Mutex<Sender<()>>,
    release: Mutex<Inbox<()>>,
}

struct Pause<'a> {
    inner: &'a mut dyn Receiver,
    gate: &'a Gate,
    first: bool,
}

impl Receiver for Pause<'_> {
    fn head(&mut self, head: &Head) -> Result<(), String> {
        self.inner.head(head)
    }

    fn data(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.inner.data(bytes)?;
        if self.first {
            self.first = false;
            let _ = self.gate.entered.lock().unwrap().send(());
            let _ = self.gate.release.lock().unwrap().recv_timeout(WAIT);
        }
        Ok(())
    }
}

impl Fetch for Gate {
    fn get(&self, url: &str, from: u64, rx: &mut dyn Receiver) -> Result<Head, String> {
        let mut pause = Pause {
            inner: rx,
            gate: self,
            first: true,
        };
        self.inner.get(url, from, &mut pause)
    }
}

pub(crate) fn gated(fake: FakeFetch) -> (Arc<Gate>, Inbox<()>, Sender<()>) {
    let (entered, saw) = channel();
    let (release, wait) = channel();
    (
        Arc::new(Gate {
            inner: fake,
            entered: Mutex::new(entered),
            release: Mutex::new(wait),
        }),
        saw,
        release,
    )
}

pub(crate) struct FakeConvert;

impl Converter for FakeConvert {
    fn convert(&self, dir: &Path, _: &AtomicBool) -> Result<String, String> {
        assert!(dir.join(FLAT).is_file());
        std::fs::write(dir.join(TABLE), b"table").map_err(|e| e.to_string())?;
        Ok(sha256_hex(b"table"))
    }
}

struct Seen {
    events: Mutex<Vec<Value>>,
    done: Mutex<Inbox<String>>,
    outcomes: Mutex<Vec<(String, bool)>>,
}

struct GateConvert {
    calls: AtomicUsize,
    entered: Mutex<Sender<()>>,
    release: Mutex<Inbox<()>>,
}

impl Converter for GateConvert {
    fn convert(&self, dir: &Path, cancel: &AtomicBool) -> Result<String, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let _ = self.entered.lock().unwrap().send(());
        let _ = self.release.lock().unwrap().recv_timeout(WAIT);
        if cancel.load(Ordering::SeqCst) {
            return Err(CANCELLED.to_string());
        }
        std::fs::write(dir.join(TABLE), b"table").map_err(|e| e.to_string())?;
        Ok(sha256_hex(b"table"))
    }
}

fn gate_convert() -> (Arc<GateConvert>, Inbox<()>, Sender<()>) {
    let (entered, saw) = channel();
    let (release, wait) = channel();
    (
        Arc::new(GateConvert {
            calls: AtomicUsize::new(0),
            entered: Mutex::new(entered),
            release: Mutex::new(wait),
        }),
        saw,
        release,
    )
}

type OnCancel = Box<dyn Fn() + Send>;

struct Live {
    inner: FakeFetch,
    entered: Mutex<Sender<()>>,
    cancelled: Mutex<Option<OnCancel>>,
}

struct Hold<'a> {
    inner: &'a mut dyn Receiver,
    live: &'a Live,
    first: bool,
}

impl Receiver for Hold<'_> {
    fn head(&mut self, head: &Head) -> Result<(), String> {
        self.inner.head(head)
    }

    fn data(&mut self, bytes: &[u8]) -> Result<(), String> {
        self.inner.data(bytes)?;
        if !self.first {
            return Ok(());
        }
        self.first = false;
        let _ = self.live.entered.lock().unwrap().send(());
        let started = Instant::now();
        while started.elapsed() < WAIT {
            if let Err(e) = self.inner.data(&[]) {
                let hook = self.live.cancelled.lock().unwrap().take();
                if let Some(hook) = hook {
                    hook();
                }
                return Err(e);
            }
            std::thread::yield_now();
        }
        Err("the test never cancelled the download".to_string())
    }
}

impl Fetch for Live {
    fn get(&self, url: &str, from: u64, rx: &mut dyn Receiver) -> Result<Head, String> {
        let mut hold = Hold {
            inner: rx,
            live: self,
            first: true,
        };
        self.inner.get(url, from, &mut hold)
    }
}

fn live(fake: FakeFetch) -> (Arc<Live>, Inbox<()>) {
    let (entered, saw) = channel();
    (
        Arc::new(Live {
            inner: fake,
            entered: Mutex::new(entered),
            cancelled: Mutex::new(None),
        }),
        saw,
    )
}

fn fetcher(dir: &Path, fetch: Arc<dyn Fetch>) -> (Arc<Fetcher>, Arc<Seen>) {
    fetcher_with(dir, fetch, Arc::new(FakeConvert))
}

fn last_state(seen: &Seen) -> Value {
    seen.events.lock().unwrap().last().unwrap()["state"].clone()
}

fn fetcher_with(
    dir: &Path,
    fetch: Arc<dyn Fetch>,
    convert: Arc<dyn Converter>,
) -> (Arc<Fetcher>, Arc<Seen>) {
    let (tx, rx) = channel();
    let seen = Arc::new(Seen {
        events: Mutex::new(Vec::new()),
        done: Mutex::new(rx),
        outcomes: Mutex::new(Vec::new()),
    });
    let log = seen.clone();
    let finished_log = seen.clone();
    let tx = Mutex::new(tx);
    let net = ModelNet { fetch, convert };
    let f = Fetcher::new(
        dir.to_path_buf(),
        net,
        Arc::new(move |v| log.events.lock().unwrap().push(v)),
        Arc::new(move |id, ok| {
            finished_log
                .outcomes
                .lock()
                .unwrap()
                .push((id.to_string(), ok));
            let _ = tx.lock().unwrap().send(id.to_string());
        }),
    );
    (Arc::new(f), seen)
}

#[test]
fn fetch_is_app_tier() {
    let (rig, _dir) = rig_net(0, true, true, ModelNet::offline());
    assert_eq!(rig.ops.spec(ops::EMBED_FETCH), Some(OpSpec::APP));
    assert_eq!(rig.ops.spec(ops::EMBED_FETCH_CANCEL), Some(OpSpec::APP));
    assert_eq!(rig.ops.spec(ops::EMBED_REMOVE), Some(OpSpec::APP));
}

#[test]
fn fetch_is_refused_while_off() {
    let (rig, _dir) = rig_net(0, true, false, ModelNet::offline());
    let id = &manifest().models[0].id;
    let err = rig
        .ops
        .call(&app(), ops::EMBED_FETCH, &json!({"model": id}))
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::Forbidden);
    assert_eq!(err.message, "Turn on Semantic Search first.");
    rig.on.lock().unwrap().on = true;
    let bad = rig
        .ops
        .call(&app(), ops::EMBED_FETCH, &json!({"model": "nope"}))
        .unwrap_err();
    assert_eq!(bad.code, ErrorCode::BadRequest);
    assert!(rig.ops.call(&app(), ops::EMBED_FETCH, &json!({})).is_err());
}

#[test]
fn fetch_answers_at_once_and_reports_progress() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-progress");
    let data = bytes(1, 6000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (gate, saw, release) = gated(fake);
    let (fetcher, seen) = fetcher(tmp.root(), gate);
    let model = entry("t@1", "t-model", vec![f]);
    fetcher.start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    assert!(fetcher.running());
    let mid = fetcher.list();
    assert_eq!(mid[0]["state"], "downloading");
    assert_eq!(mid[0]["bytes"], 1000);
    release.send(()).unwrap();
    assert_eq!(seen.done.lock().unwrap().recv_timeout(WAIT).unwrap(), "t@1");
    assert!(fetcher.wait_idle(WAIT));
    let events = seen.events.lock().unwrap();
    assert_eq!(
        events[0],
        progress_json("t@1", "downloading", 0, 6000, None)
    );
    assert_eq!(events.last().unwrap()["state"], "ready");
    assert!(tmp.root().join("models/t-model/w.bin").is_file());
    assert!(!fetcher.running());
    assert_eq!(fetcher.list()[0]["state"], "ready");
}

#[test]
fn a_second_start_does_not_start_a_second_download() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-twice");
    let data = bytes(2, 3000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (gate, saw, release) = gated(fake);
    let (fetcher, seen) = fetcher(tmp.root(), gate.clone());
    let model = entry("t@2", "t-twice", vec![f]);
    fetcher.start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    fetcher.start(model).unwrap();
    release.send(()).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(gate.inner.requests().len(), 1);
}

#[test]
fn a_failed_download_says_so_in_plain_words() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-failed");
    let data = bytes(3, 2000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.status(&url(&f), 503);
    let (fetcher, seen) = fetcher(tmp.root(), Arc::new(fake));
    fetcher.start(entry("t@3", "t-failed", vec![f])).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    let last = seen.events.lock().unwrap().last().cloned().unwrap();
    assert_eq!(last["state"], "failed");
    assert!(last["error"].as_str().unwrap().contains("503"));
}

#[test]
fn the_static_model_is_converted_and_the_f32_file_deleted() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-static");
    let table = bytes(4, 5000);
    let tok = bytes(5, 800);
    let (a, b) = (file(FLAT, &table), file("tokenizer.json", &tok));
    let fake = FakeFetch::new();
    fake.serve(&url(&a), &table).serve(&url(&b), &tok);
    let (fetcher, seen) = fetcher(tmp.root(), Arc::new(fake));
    fetcher.start(entry("s@1", "s-model", vec![a, b])).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    let dir = tmp.root().join("models/s-model");
    assert!(dir.join(TABLE).is_file());
    assert!(dir.join("tokenizer.json").is_file());
    assert!(!dir.join(FLAT).exists());
    assert_eq!(
        std::fs::read_to_string(dir.join(TABLE_HASH)).unwrap(),
        sha256_hex(b"table")
    );
    assert_eq!(
        seen.events.lock().unwrap().last().unwrap()["state"],
        "ready"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(dir.join(TABLE_HASH))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn turning_off_cancels_and_keeps_the_part() {
    let data = bytes(6, 8000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (gate, saw, release) = gated(fake);
    let net = ModelNet {
        fetch: gate,
        convert: Arc::new(FakeConvert),
    };
    let (rig, dir) = rig_net(0, true, true, net);
    rig.ops.tick();
    let model = entry("t@6", "t-off", vec![f]);
    rig.ops.fetcher().start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    rig.on.lock().unwrap().on = false;
    rig.ops.tick();
    release.send(()).unwrap();
    assert!(rig.ops.fetcher().wait_idle(WAIT));
    assert!(!rig.ops.fetcher().running());
    let folder = embed_paths::models_dir(dir.root()).join("t-off");
    assert!(folder.join("w.bin.part").is_file());
    assert!(!folder.join("w.bin").exists());
    assert_eq!(rig.ops.fetcher().list()[0]["state"], "cancelled");
}

#[test]
fn cancel_reports_whether_anything_was_running() {
    let (rig, _dir) = rig_net(0, true, true, ModelNet::offline());
    let id = &manifest().models[0].id;
    let out = rig
        .ops
        .call(&app(), ops::EMBED_FETCH_CANCEL, &json!({"model": id}))
        .unwrap();
    assert_eq!(out["cancelled"], false);
}

#[test]
fn remove_deletes_files_and_vectors() {
    let (rig, dir) = rig_net(4, true, true, ModelNet::offline());
    rig.ops.tick();
    let folder = embed_paths::models_dir(dir.root()).join("static-mrl");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join(TABLE), b"table").unwrap();
    let before = rig.ops.call(&app(), ops::EMBED_STATUS, &json!({})).unwrap();
    assert!(before["stored"].as_u64().unwrap() > 0);
    let gone = rig
        .ops
        .call(
            &app(),
            ops::EMBED_REMOVE,
            &json!({"model": embed_paths::STATIC_ID}),
        )
        .unwrap();
    assert_eq!(gone["removed"], true);
    assert!(!folder.exists());
    let after = rig.ops.call(&app(), ops::EMBED_STATUS, &json!({})).unwrap();
    assert!(after["corpora"][0]["embedded"]
        .get(embed_paths::STATIC_ID)
        .is_none_or(|n| n == 0));
}

#[test]
fn a_file_of_the_right_size_but_the_wrong_bytes_is_fetched_again() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-kept");
    let data = bytes(8, 3000);
    let f = file("w.bin", &data);
    let folder = tmp.root().join("models/t-kept");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("w.bin"), vec![0u8; 3000]).unwrap();
    let fake = FakeFetch::new();
    fake.serve(&url(&f), &data);
    let (fetcher, seen) = fetcher(tmp.root(), Arc::new(fake));
    fetcher.start(entry("t@8", "t-kept", vec![f])).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(std::fs::read(folder.join("w.bin")).unwrap(), data);
}

#[test]
fn a_file_that_is_already_right_is_not_fetched_again() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-kept-right");
    let data = bytes(9, 3000);
    let f = file("w.bin", &data);
    let folder = tmp.root().join("models/t-right");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("w.bin"), &data).unwrap();
    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&f), &data);
    let (fetcher, seen) = fetcher(tmp.root(), fake.clone());
    fetcher.start(entry("t@9", "t-right", vec![f])).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert!(fake.requests().is_empty());
    assert_eq!(
        seen.events.lock().unwrap().last().unwrap()["state"],
        "ready"
    );
}

#[cfg(unix)]
#[test]
fn a_linked_file_is_never_taken_as_the_download() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-kept-link");
    let data = bytes(10, 3000);
    let f = file("w.bin", &data);
    let folder = tmp.root().join("models/t-link");
    std::fs::create_dir_all(&folder).unwrap();
    let outside = tmp.root().join("outside.bin");
    std::fs::write(&outside, &data).unwrap();
    std::os::unix::fs::symlink(&outside, folder.join("w.bin")).unwrap();
    let fake = Arc::new(FakeFetch::new());
    fake.serve(&url(&f), &data);
    let (fetcher, seen) = fetcher(tmp.root(), fake.clone());
    fetcher.start(entry("t@10", "t-link", vec![f])).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(fake.requests().len(), 1);
    assert!(!std::fs::symlink_metadata(folder.join("w.bin"))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(std::fs::read(&outside).unwrap(), data);
}

#[cfg(unix)]
#[test]
fn the_checksum_is_never_written_through_a_link() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-hash-link");
    let table = bytes(11, 5000);
    let (a, b) = (file(FLAT, &table), file("tokenizer.json", &bytes(12, 800)));
    let fake = FakeFetch::new();
    fake.serve(&url(&a), &table)
        .serve(&url(&b), &bytes(12, 800));
    let folder = tmp.root().join("models/s-link");
    std::fs::create_dir_all(&folder).unwrap();
    let outside = tmp.root().join("outside.txt");
    std::fs::write(&outside, b"keep").unwrap();
    std::os::unix::fs::symlink(&outside, folder.join(TABLE_HASH)).unwrap();
    let (fetcher, seen) = fetcher(tmp.root(), Arc::new(fake));
    fetcher.start(entry("s@2", "s-link", vec![a, b])).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(std::fs::read(&outside).unwrap(), b"keep");
    assert!(!std::fs::symlink_metadata(folder.join(TABLE_HASH))
        .unwrap()
        .file_type()
        .is_symlink());
    assert_eq!(
        seen.events.lock().unwrap().last().unwrap()["state"],
        "ready"
    );
}

#[test]
fn a_start_after_a_cancel_downloads_again_when_the_old_run_ends() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-restart");
    let data = bytes(13, 3000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (gate, saw, release) = gated(fake);
    let (fetcher, seen) = fetcher(tmp.root(), gate.clone());
    let model = entry("t@13", "t-restart", vec![f]);
    fetcher.start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    assert!(fetcher.cancel("t@13"));
    fetcher.start(model).unwrap();
    release.send(()).unwrap();
    release.send(()).unwrap();
    assert_eq!(
        seen.done.lock().unwrap().recv_timeout(WAIT).unwrap(),
        "t@13"
    );
    assert_eq!(last_state(&seen), "ready");
    assert!(seen
        .events
        .lock()
        .unwrap()
        .iter()
        .all(|e| e["state"] != "cancelled"));
    assert!(tmp.root().join("models/t-restart/w.bin").is_file());
    assert_eq!(gate.inner.requests().len(), 2);
}

#[test]
fn a_cancel_after_the_second_start_still_cancels() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-recancel");
    let data = bytes(20, 3000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (gate, saw, release) = gated(fake);
    let (fetcher, seen) = fetcher(tmp.root(), gate.clone());
    let model = entry("t@20", "t-recancel", vec![f]);
    fetcher.start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    assert!(fetcher.cancel("t@20"));
    fetcher.start(model).unwrap();
    assert!(fetcher.cancel("t@20"));
    release.send(()).unwrap();
    release.send(()).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "cancelled");
    assert_eq!(gate.inner.requests().len(), 1);
}

#[test]
fn a_cancel_before_the_conversion_never_starts_it() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-convert-skip");
    let table = bytes(14, 5000);
    let a = file(FLAT, &table);
    let fake = FakeFetch::new();
    fake.chunk(5000).serve(&url(&a), &table);
    let (gate, saw, release) = gated(fake);
    let (convert, _entered, free) = gate_convert();
    drop(free);
    let (fetcher, seen) = fetcher_with(tmp.root(), gate, convert.clone());
    fetcher.start(entry("s@14", "s-skip", vec![a])).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    assert!(fetcher.cancel("s@14"));
    release.send(()).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(convert.calls.load(Ordering::SeqCst), 0);
    assert_eq!(last_state(&seen), "cancelled");
    let dir = tmp.root().join("models/s-skip");
    assert!(dir.join(FLAT).is_file());
    assert!(!dir.join(TABLE).exists());
}

#[test]
fn a_cancel_reaches_a_conversion_that_is_running() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-convert-cancel");
    let table = bytes(15, 5000);
    let a = file(FLAT, &table);
    let fake = FakeFetch::new();
    fake.serve(&url(&a), &table);
    let (convert, entered, free) = gate_convert();
    let (fetcher, seen) = fetcher_with(tmp.root(), Arc::new(fake), convert);
    fetcher.start(entry("s@15", "s-cancel", vec![a])).unwrap();
    entered.recv_timeout(WAIT).unwrap();
    assert!(fetcher.cancel("s@15"));
    free.send(()).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(last_state(&seen), "cancelled");
    let dir = tmp.root().join("models/s-cancel");
    assert!(dir.join(FLAT).is_file());
    assert!(!dir.join(TABLE).exists());
}

#[cfg(unix)]
#[test]
fn a_cancel_kills_the_helper_that_is_converting() {
    use std::os::unix::fs::PermissionsExt;
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-convert-kill");
    let dir = tmp.root().to_path_buf();
    let helper = dir.join("helper.sh");
    let script = "#!/bin/sh\necho up > \"$2/gate\"\nexec sleep 30\n";
    std::fs::write(&helper, script).unwrap();
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    let gate = dir.join("gate");
    let made = std::process::Command::new("mkfifo").arg(&gate).status();
    assert!(made.unwrap().success());
    let (up_tx, up) = channel();
    std::thread::spawn(move || {
        let _ = up_tx.send(std::fs::read_to_string(gate));
    });
    let cancel = Arc::new(AtomicBool::new(false));
    let (out_tx, out) = channel();
    let (flag, folder) = (cancel.clone(), dir.clone());
    std::thread::spawn(move || {
        let _ = out_tx.send(ProcessConvert { exe: helper }.convert(&folder, &flag));
    });
    assert_eq!(up.recv_timeout(WAIT).unwrap().unwrap(), "up\n");
    cancel.store(true, Ordering::SeqCst);
    assert_eq!(
        out.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(CANCELLED.to_string())
    );
}

#[test]
fn remove_deletes_nothing_while_the_download_is_still_running() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-remove-busy");
    let data = bytes(16, 3000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (gate, saw, release) = gated(fake);
    let (fetcher, seen) = fetcher(tmp.root(), gate);
    let model = entry("t@16", "t-busy", vec![f]);
    fetcher.start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    let folder = tmp.root().join("models/t-busy");
    assert!(fetcher.remove(model, Duration::ZERO).is_err());
    assert!(folder.join("w.bin.part").is_file());
    assert!(fetcher.running());
    release.send(()).unwrap();
    seen.done.lock().unwrap().recv_timeout(WAIT).unwrap();
    assert_eq!(fetcher.list()[0]["state"], "cancelled");
    assert!(fetcher.remove(model, WAIT).unwrap());
    assert!(!folder.exists());
    assert!(fetcher.list().is_empty());
}

#[test]
fn remove_waits_for_the_download_and_refuses_a_start_meanwhile() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-remove-wait");
    let data = bytes(17, 3000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (live, saw) = live(fake);
    let (fetcher, seen) = fetcher(tmp.root(), live.clone());
    let model = entry("t@17", "t-wait", vec![f]);
    let (tx, refused) = channel();
    let again = fetcher.clone();
    *live.cancelled.lock().unwrap() = Some(Box::new(move || {
        let _ = tx.send(again.start(model).is_err());
    }));
    fetcher.start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    assert!(fetcher.remove(model, WAIT).unwrap());
    assert!(refused.recv_timeout(WAIT).unwrap());
    assert!(!fetcher.running());
    assert!(!tmp.root().join("models/t-wait").exists());
    assert!(fetcher.list().is_empty());
    assert!(seen.done.lock().unwrap().try_recv().is_err());
    assert!(seen
        .events
        .lock()
        .unwrap()
        .iter()
        .all(|e| e["state"] != "cancelled"));
}

#[test]
fn embed_remove_stops_the_download_before_it_deletes() {
    let model = &manifest().models[0];
    let first = &model.files[0];
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&first.url(), &bytes(18, 4000));
    let (live, saw) = live(fake);
    let net = ModelNet {
        fetch: live.clone(),
        convert: Arc::new(FakeConvert),
    };
    let (rig, dir) = rig_net(0, true, true, net);
    rig.ops.tick();
    let (tx, refused) = channel();
    let again = rig.ops.fetcher().clone();
    *live.cancelled.lock().unwrap() = Some(Box::new(move || {
        let _ = tx.send(again.start(model).is_err());
    }));
    rig.ops.fetcher().start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    let folder = embed_paths::models_dir(dir.root()).join(&model.folder);
    assert!(folder.join(format!("{}.part", first.save_as)).is_file());
    let gone = rig
        .ops
        .call(&app(), ops::EMBED_REMOVE, &json!({"model": model.id}))
        .unwrap();
    assert_eq!(gone["removed"], true);
    assert!(refused.recv_timeout(WAIT).unwrap());
    assert!(!rig.ops.fetcher().running());
    assert!(!folder.exists());
    assert!(rig.ops.fetcher().list().is_empty());
    let states = rig.fetch_states();
    assert_eq!(states.last().map(String::as_str), Some("absent"));
    assert!(!states.iter().any(|s| s == "cancelled"));
}

#[test]
fn closing_stops_the_downloads() {
    let data = bytes(19, 4000);
    let f = file("w.bin", &data);
    let fake = FakeFetch::new();
    fake.chunk(1000).serve(&url(&f), &data);
    let (live, saw) = live(fake);
    let net = ModelNet {
        fetch: live,
        convert: Arc::new(FakeConvert),
    };
    let (rig, dir) = rig_net(0, true, true, net);
    rig.ops.tick();
    let model = entry("t@19", "t-close", vec![f]);
    rig.ops.fetcher().start(model).unwrap();
    saw.recv_timeout(WAIT).unwrap();
    rig.ops.close();
    assert!(rig.ops.fetcher().wait_idle(Duration::from_secs(5)));
    assert_eq!(rig.ops.fetcher().list()[0]["state"], "cancelled");
    let folder = embed_paths::models_dir(dir.root()).join("t-close");
    assert!(folder.join("w.bin.part").is_file());
}

#[cfg(unix)]
#[test]
fn remove_never_follows_a_linked_model_folder() {
    let tmp = crate::limits_ops_tests::TempDir::new("fetch-remove-link");
    let outside = tmp.root().join("outside");
    std::fs::create_dir_all(&outside).unwrap();
    std::fs::write(outside.join("keep.txt"), b"keep").unwrap();
    std::fs::create_dir_all(tmp.root().join("models")).unwrap();
    std::os::unix::fs::symlink(&outside, tmp.root().join("models/t-linked")).unwrap();
    let (fetcher, _seen) = fetcher(tmp.root(), Arc::new(FakeFetch::new()));
    let _ = fetcher.remove(entry("t@21", "t-linked", Vec::new()), WAIT);
    assert_eq!(std::fs::read(outside.join("keep.txt")).unwrap(), b"keep");
}

#[path = "embed_fetch_race_tests.rs"]
mod race_tests;
