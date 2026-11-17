use crate::acting::Acting;
use crate::acting_ops_tests::{app, FakeRoom, SESSION};
use crate::ask_ops::AskOps;
use crate::inbox_ops::InboxOps;
use crate::late_emit::Events;
use crate::op_group::OpGroup;
use crate::vault_caller::Processes;
use notchling_engine::ask::{AskStore, Mode, New};
use notchling_engine::sessions::Roots;
use notchling_events::{MemorySink, Salt};
use notchling_local_api::{ops, topics, ApiError, Caller, ClientKind, Tier};
use serde_json::{json, Value};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, Mutex};

pub(crate) const ASKER: u32 = 1001;
pub(crate) const SHELL: u32 = 1002;
pub(crate) const CLAUDE: u32 = 1003;
pub(crate) const STRANGER: u32 = 2001;
pub(crate) const T0: i64 = 1_794_697_060_000;

#[derive(Default)]
pub(crate) struct Tree {
    parents: Mutex<HashMap<u32, u32>>,
    cwds: HashMap<u32, PathBuf>,
    starts: Mutex<HashMap<u32, u64>>,
    ended: Mutex<HashSet<u32>>,
    argvs: Mutex<HashMap<u32, Vec<String>>>,
    stdouts: Mutex<HashMap<u32, PathBuf>>,
    exes: Mutex<HashMap<u32, PathBuf>>,
}

impl Tree {
    pub(crate) fn child_of(&self, pid: u32, parent: u32) {
        self.parents.lock().unwrap().insert(pid, parent);
    }

    pub(crate) fn runs(&self, pid: u32, argv: &[&str]) {
        let words = argv.iter().map(|w| w.to_string()).collect();
        self.argvs.lock().unwrap().insert(pid, words);
    }

    pub(crate) fn writes_to(&self, pid: u32, path: &Path) {
        self.stdouts.lock().unwrap().insert(pid, path.to_path_buf());
    }

    pub(crate) fn binary(&self, pid: u32, path: &Path) {
        self.exes.lock().unwrap().insert(pid, path.to_path_buf());
    }

    pub(crate) fn end(&self, pid: u32) {
        self.ended.lock().unwrap().insert(pid);
        self.starts.lock().unwrap().remove(&pid);
    }

    pub(crate) fn restarted(&self, pid: u32, start: u64) {
        self.starts.lock().unwrap().insert(pid, start);
    }
}

impl Processes for Tree {
    fn parent(&self, pid: u32) -> Option<u32> {
        if self.ended.lock().unwrap().contains(&pid) {
            return None;
        }
        self.parents.lock().unwrap().get(&pid).copied()
    }
    fn name(&self, _pid: u32) -> Option<String> {
        None
    }
    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        self.cwds.get(&pid).cloned()
    }
    fn resolve(&self, path: &Path) -> Option<PathBuf> {
        Some(path.to_path_buf())
    }
    fn start_time(&self, pid: u32) -> Option<u64> {
        self.starts.lock().unwrap().get(&pid).copied()
    }
    fn argv(&self, pid: u32) -> Option<Vec<String>> {
        self.argvs.lock().unwrap().get(&pid).cloned()
    }
    fn stdout_path(&self, pid: u32) -> Option<PathBuf> {
        self.stdouts.lock().unwrap().get(&pid).cloned()
    }
    fn exe(&self, pid: u32) -> Option<PathBuf> {
        self.exes.lock().unwrap().get(&pid).cloned()
    }
}

pub(crate) struct Scratch(pub PathBuf);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn scratch(tag: &str) -> Scratch {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "notchlingd-ask-{tag}-{}-{nanos}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    Scratch(dir)
}

pub(crate) struct Rig {
    pub ops: Arc<AskOps>,
    pub room: Arc<FakeRoom>,
    pub sink: Arc<MemorySink>,
    pub store: Arc<AskStore>,
    pub inbox: Arc<InboxOps>,
    pub acting: Arc<Acting>,
    pub list: Arc<Mutex<Value>>,
    pub clock: Arc<AtomicI64>,
    pub tree: Arc<Tree>,
    pub dir: Scratch,
}

pub(crate) fn rig() -> Rig {
    rig_with(json!([]), None)
}

pub(crate) fn rig_with(list: Value, store: Option<(Arc<AskStore>, Scratch)>) -> Rig {
    rig_full(list, store, None)
}

pub(crate) fn rig_full(
    list: Value,
    store: Option<(Arc<AskStore>, Scratch)>,
    inbox_dir: Option<PathBuf>,
) -> Rig {
    let (store, dir) =
        store.unwrap_or_else(|| (Arc::new(AskStore::in_memory_for_tests()), scratch("rig")));
    let main = dir.0.join("claude");
    std::fs::create_dir_all(main.join("sessions")).unwrap();
    std::fs::write(
        main.join("sessions").join(format!("{CLAUDE}.json")),
        format!("{{\"sessionId\":\"{SESSION}\"}}"),
    )
    .unwrap();
    let work = dir.0.join("work");
    std::fs::create_dir_all(work.join(".git")).unwrap();
    std::fs::write(work.join(".git/HEAD"), "ref: refs/heads/agent/ask\n").unwrap();
    let tree = Tree {
        parents: Mutex::new(HashMap::from([
            (ASKER, SHELL),
            (SHELL, CLAUDE),
            (CLAUDE, 1),
            (STRANGER, 1),
        ])),
        cwds: HashMap::from([(ASKER, work)]),
        starts: Mutex::new(HashMap::from([(ASKER, 4242)])),
        ..Tree::default()
    };
    let tree = Arc::new(tree);
    let roots = Roots {
        main_dir: main,
        lanes_file: PathBuf::from("/nonexistent/lanes.json"),
    };
    let room = Arc::new(FakeRoom::default());
    let acting = Arc::new(Acting::in_memory(true));
    let sink = Arc::new(MemorySink::new());
    let clock = Arc::new(AtomicI64::new(T0));
    let ticks = clock.clone();
    let list = Arc::new(Mutex::new(list));
    let inbox_dir = inbox_dir.unwrap_or_else(|| dir.0.join("inbox"));
    std::fs::create_dir_all(&inbox_dir).unwrap();
    let inbox = InboxOps::new(inbox_dir, room.clone());
    let ops = Arc::new(
        AskOps::new(
            store.clone(),
            list.clone(),
            roots,
            acting.clone(),
            room.clone(),
            Some(Events::fixed(sink.clone(), Salt([1; 32]))),
            tree.clone(),
        )
        .with_clock(Arc::new(move || ticks.load(Ordering::SeqCst))),
    );
    ops.attach(inbox.clone());
    Rig {
        ops,
        room,
        sink,
        store,
        inbox,
        acting,
        list,
        clock,
        tree,
        dir,
    }
}

pub(crate) fn asker_at(pid: u32) -> Caller {
    Caller {
        tier: Tier::Agent,
        client: ClientKind::Cli,
        peer: None,
        pid: Some(pid),
    }
}

pub(crate) fn base() -> Value {
    json!({"question": "Which one?", "choices": ["a", "b"], "key": "k1"})
}

pub(crate) fn open_with(r: &Rig, args: Value) -> Result<Value, ApiError> {
    r.ops.call(&asker_at(ASKER), ops::ASK_OPEN, &args)
}

pub(crate) fn open(r: &Rig) -> String {
    open_with(r, base()).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string()
}

pub(crate) fn person(r: &Rig, op: &str, args: Value) -> Result<Value, ApiError> {
    r.ops.call(&app(), op, &args)
}

pub(crate) fn person_on(ops: &Arc<AskOps>, op: &str, args: Value) -> Result<Value, ApiError> {
    ops.call(&app(), op, &args)
}

pub(crate) fn changes(r: &Rig) -> Vec<Value> {
    r.room
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|(name, _)| name == topics::ASK_CHANGED)
        .map(|(_, data)| data["row"].clone())
        .collect()
}

pub(crate) fn new_row(session: &str, key: &str, pid: u32) -> New {
    New {
        key: key.into(),
        session: session.into(),
        session_id: None,
        pid,
        pid_start: None,
        folder: None,
        branch: None,
        question: "q".into(),
        choices: Some(vec!["a".into(), "b".into()]),
        main_may_answer: false,
        mode: Mode::Background,
        deadline_ms: None,
    }
}

pub(crate) fn wait_as(r: &Rig, pid: u32, id: &str, ms: i64) -> Result<Value, ApiError> {
    r.ops.call(
        &asker_at(pid),
        ops::ASK_WAIT,
        &json!({"id": id, "waitMs": ms}),
    )
}

pub(crate) fn state_of(r: &Rig, id: &str) -> String {
    r.store.get(id).unwrap().unwrap().state.as_str().to_string()
}

pub(crate) fn until(what: &str, mut ready: impl FnMut() -> bool) {
    let stop = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !ready() {
        assert!(std::time::Instant::now() < stop, "{what}");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

pub(crate) fn item_of(r: &Rig, id: &str) -> notchling_engine::inbox::Item {
    let inbox_id = r.store.get(id).unwrap().unwrap().inbox_id.unwrap();
    r.inbox
        .items()
        .into_iter()
        .find(|item| item.id == inbox_id)
        .unwrap()
}

pub(crate) fn items(r: &Rig) -> Vec<notchling_engine::inbox::Item> {
    r.inbox.items()
}
