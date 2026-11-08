use super::*;
use crate::delegate::outcome::{to_json, Outcome};
use crate::delegate::test_support::scratch;
use std::sync::atomic::{AtomicUsize, Ordering};

type Done = Box<dyn FnOnce(String) + Send>;

#[derive(Default)]
struct FakeHost {
    launched: Mutex<Vec<(Vec<String>, String)>>,
    waiting: Mutex<Vec<Done>>,
    terminated: Arc<AtomicUsize>,
    refuse: Mutex<Option<String>>,
    /// Calls `done` before `launch` returns.
    instant: Mutex<Option<String>>,
}

struct FakeJob(Arc<AtomicUsize>);

impl Job for FakeJob {
    fn terminate(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

impl Host for FakeHost {
    fn launch(&self, args: Vec<String>, folder: &str, done: Done) -> Result<Box<dyn Job>, String> {
        if let Some(why) = self.refuse.lock().unwrap().clone() {
            return Err(why);
        }
        self.launched.lock().unwrap().push((args, folder.into()));
        match self.instant.lock().unwrap().clone() {
            Some(output) => done(output),
            None => self.waiting.lock().unwrap().push(done),
        }
        Ok(Box::new(FakeJob(self.terminated.clone())))
    }
}

impl FakeHost {
    /// The oldest running command ends with this output.
    fn finish(&self, output: &str) {
        let done = self.waiting.lock().unwrap().remove(0);
        done(output.into());
    }
}

struct Rig {
    store: SecondOpinionStore,
    host: Arc<FakeHost>,
    dir: PathBuf,
}

impl Rig {
    fn file(&self) -> PathBuf {
        self.dir.join("data/second-opinions.json")
    }

    fn again(&self) -> SecondOpinionStore {
        make(self.host.clone(), &self.dir)
    }
}

impl Drop for Rig {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn make(host: Arc<FakeHost>, dir: &Path) -> SecondOpinionStore {
    static CLOCK: AtomicUsize = AtomicUsize::new(1000);
    static IDS: AtomicUsize = AtomicUsize::new(0);
    SecondOpinionStore::new(
        Some(dir.join("data/second-opinions.json")),
        dir.join("tmp"),
        host,
        Box::new(|| CLOCK.fetch_add(1, Ordering::SeqCst) as f64),
        Box::new(|| format!("run-{}", IDS.fetch_add(1, Ordering::SeqCst))),
    )
}

fn rig() -> Rig {
    let dir = scratch("so-store");
    let host = Arc::new(FakeHost::default());
    Rig {
        store: make(host.clone(), &dir),
        host,
        dir,
    }
}

fn request(anchor: &str) -> Request {
    Request {
        session: "s".into(),
        anchor: anchor.into(),
        subject: "Reply".into(),
        tool: Tool::Codex,
        model: Some("m".into()),
        effort: None,
        folder: "/w".into(),
        prompt: "Please review.".into(),
    }
}

fn answer(reply: &str) -> String {
    let mut o = Outcome::new(Tool::Codex);
    o.session = Some("conv-1".into());
    o.reply = reply.into();
    o.seconds = 3.0;
    o.tokens = Some(10);
    format!("{}\n", to_json(&o))
}

#[test]
fn an_opinion_runs_read_only_with_the_prompt_in_a_private_file() {
    let rig = rig();
    let run = rig.store.start(&request("a"));
    assert_eq!(run.status, Status::Running);
    let (args, folder) = rig.host.launched.lock().unwrap()[0].clone();
    assert_eq!(folder, "/w");
    assert_eq!(args[..4], ["delegate", "codex", "--model", "m"]);
    assert!(!args.contains(&"--write".to_string()));
    let file = PathBuf::from(args.last().unwrap());
    assert_eq!(
        file,
        rig.dir
            .join(format!("tmp/notchling-second-opinion-{}.txt", run.id))
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "Please review.");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }

    rig.host
        .finish(&format!("noise\n{}", answer("Line 3 looks off.")));
    let done = &rig.store.opinions()[0];
    assert_eq!(done.status, Status::Done);
    assert_eq!(done.reply, "Line 3 looks off.");
    assert_eq!(done.delegated_session.as_deref(), Some("conv-1"));
    assert_eq!(done.tokens, Some(10));
    assert!(!file.exists(), "the prompt file is removed");
}

#[test]
fn runs_are_the_sessions_opinions_under_the_asked_anchors_oldest_first() {
    let rig = rig();
    rig.store.start(&request("a"));
    rig.store.start(&request("b"));
    let mut other = request("a");
    other.session = "other".into();
    rig.store.start(&other);
    let anchors = |list: &[&str]| list.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    let both = rig.store.runs("s", &anchors(&["b", "a"]));
    assert_eq!(
        both.iter().map(|r| r.anchor.as_str()).collect::<Vec<_>>(),
        ["a", "b"]
    );
    assert_eq!(rig.store.runs("s", &anchors(&["a"])).len(), 1);
    assert!(rig.store.runs("s", &anchors(&["z"])).is_empty());
    assert_eq!(rig.store.runs("other", &anchors(&["a"])).len(), 1);
}

#[test]
fn a_command_that_ends_before_launch_returns_is_still_recorded() {
    let rig = rig();
    *rig.host.instant.lock().unwrap() = Some(answer("Quick."));
    rig.store.start(&request("a"));
    assert_eq!(rig.store.opinions()[0].status, Status::Done);
    assert_eq!(rig.store.opinions()[0].reply, "Quick.");
}

#[test]
fn a_command_that_will_not_start_is_a_failed_card_with_no_prompt_left_behind() {
    let rig = rig();
    *rig.host.refuse.lock().unwrap() = Some("no such program".into());
    let run = rig.store.start(&request("a"));
    let card = &rig.store.opinions()[0];
    assert_eq!(
        (card.status, card.error.as_deref()),
        (Status::Failed, Some("no such program"))
    );
    assert!(!rig
        .dir
        .join(format!("tmp/notchling-second-opinion-{}.txt", run.id))
        .exists());
}

#[test]
fn a_prompt_that_cannot_be_written_fails_the_card() {
    let dir = scratch("so-blocked");
    std::fs::write(dir.join("tmp"), "a file where the folder should be").unwrap();
    let host = Arc::new(FakeHost::default());
    let store = make(host.clone(), &dir);
    store.start(&request("a"));
    assert_eq!(
        store.opinions()[0].error.as_deref(),
        Some("Couldn't write the prompt.")
    );
    assert!(host.launched.lock().unwrap().is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cancel_marks_the_card_at_once_and_keeps_what_the_tool_said_so_far() {
    let rig = rig();
    let run = rig.store.start(&request("a"));
    rig.store.cancel(&run.id);
    rig.store.cancel(&run.id);
    assert_eq!(rig.host.terminated.load(Ordering::SeqCst), 1, "asked once");
    assert_eq!(rig.store.opinions()[0].status, Status::Cancelled);

    let mut partial = Outcome::new(Tool::Codex);
    partial.session = Some("conv-1".into());
    partial.reply = "Half an answer.".into();
    partial.error = Some("Codex was stopped.".into());
    rig.host.finish(&to_json(&partial));
    let card = &rig.store.opinions()[0];
    assert_eq!(card.status, Status::Cancelled);
    assert_eq!(card.reply, "Half an answer.");
    assert_eq!(card.delegated_session.as_deref(), Some("conv-1"));
    // Nothing to cancel any more.
    rig.store.cancel(&run.id);
    assert_eq!(rig.host.terminated.load(Ordering::SeqCst), 1);
}

#[test]
fn remove_cancels_and_forgets() {
    let rig = rig();
    let run = rig.store.start(&request("a"));
    rig.store.remove(&run.id);
    assert_eq!(rig.host.terminated.load(Ordering::SeqCst), 1);
    assert!(rig.store.opinions().is_empty());
    rig.host.finish("");
    assert!(
        rig.store.opinions().is_empty(),
        "the late end finds nothing to update"
    );
    assert_eq!(rig.again().opinions(), []);
}

#[test]
fn stop_all_stops_every_running_command() {
    let rig = rig();
    rig.store.start(&request("a"));
    rig.store.start(&request("b"));
    rig.store.stop_all();
    assert_eq!(rig.host.terminated.load(Ordering::SeqCst), 2);
}

#[test]
fn results_survive_a_relaunch_and_a_run_that_was_going_is_failed() {
    let rig = rig();
    rig.store.start(&request("a"));
    rig.store.start(&request("b"));
    rig.host.finish(&answer("First."));
    let again = rig.again();
    let mut cards = again.opinions();
    cards.sort_by(|a, b| a.anchor.cmp(&b.anchor));
    assert_eq!(cards[0].status, Status::Done);
    assert_eq!(cards[0].reply, "First.");
    assert_eq!(cards[1].status, Status::Failed);
    assert_eq!(
        cards[1].error.as_deref(),
        Some("Notchling quit before it finished.")
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(rig.file()).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
}

#[test]
fn a_damaged_file_is_an_empty_list() {
    let rig = rig();
    std::fs::create_dir_all(rig.dir.join("data")).unwrap();
    std::fs::write(rig.file(), "{ not json").unwrap();
    assert!(rig.again().opinions().is_empty());
}

#[test]
fn the_file_keeps_the_running_and_the_last_hundred_finished() {
    let rig = rig();
    for i in 0..105 {
        *rig.host.instant.lock().unwrap() = Some(answer(&format!("r{i}")));
        rig.store.start(&request("a"));
    }
    *rig.host.instant.lock().unwrap() = None;
    rig.store.start(&request("a"));
    assert_eq!(rig.store.opinions().len(), 106, "memory keeps them all");
    let saved: Vec<SecondOpinion> =
        serde_json::from_slice(&std::fs::read(rig.file()).unwrap()).unwrap();
    assert_eq!(saved.len(), 100);
    assert_eq!(
        saved.iter().filter(|o| o.status == Status::Running).count(),
        1
    );
    assert_eq!(
        saved[0].reply, "r6",
        "the oldest finished ones were left out"
    );
    assert_eq!(saved[98].reply, "r104");
}

#[test]
fn without_a_file_nothing_is_written() {
    let dir = scratch("so-memory");
    let host = Arc::new(FakeHost::default());
    let store = SecondOpinionStore::new(
        None,
        dir.join("tmp"),
        host.clone(),
        Box::new(|| 1.0),
        Box::new(|| "x".into()),
    );
    store.start(&request("a"));
    host.finish(&answer("ok"));
    assert_eq!(store.opinions()[0].status, Status::Done);
    assert!(!dir.join("data").exists());
    assert_eq!(file_in(&dir), dir.join("second-opinions.json"));
    let _ = std::fs::remove_dir_all(&dir);
}
