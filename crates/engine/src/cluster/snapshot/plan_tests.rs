use super::{
    carry, due, plan_set, quiet, smallest_acked, Movable, Observed, RepoFacts, SessionState,
    SetPlan, Trigger, MAX_FILE, QUIET_MS, TIMER_MS,
};
use crate::cluster::id::Tool;
use crate::cluster::main_sessions::MainSession;
use crate::cluster::testkit::id;
use crate::sessions::testutil::TempDir;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const NOW: i64 = 10_000_000;

fn observed(states: &[(&str, SessionState)]) -> Observed {
    Observed {
        now_ms: NOW,
        session_state: states.iter().map(|(k, v)| (k.to_string(), *v)).collect(),
        git_locks: BTreeMap::new(),
        last_write_ms: BTreeMap::new(),
        last_set_ms: Some(NOW - 1_000),
    }
}

fn turn_ended() -> (Observed, Observed) {
    (
        observed(&[("a", SessionState::Working)]),
        observed(&[("a", SessionState::Idle)]),
    )
}

#[test]
fn turn_end_triggers() {
    let (prev, now) = turn_ended();
    assert_eq!(due(&prev, &now), Some(Trigger::TurnEnd("a".to_string())));
    let ended = observed(&[("a", SessionState::Ended)]);
    assert_eq!(due(&prev, &ended), Some(Trigger::TurnEnd("a".to_string())));
    assert_eq!(due(&now, &now), None);
    let working = observed(&[("a", SessionState::Working)]);
    assert_eq!(due(&working, &working), None);
    let started = observed(&[("a", SessionState::Working)]);
    assert_eq!(due(&now, &started), None);
}

#[test]
fn timer_backstops_after_five_minutes() {
    let idle = observed(&[("a", SessionState::Idle)]);
    let mut now = idle.clone();
    now.last_set_ms = Some(NOW - TIMER_MS + 1);
    assert_eq!(due(&idle, &now), None);
    now.last_set_ms = Some(NOW - TIMER_MS);
    assert_eq!(due(&idle, &now), Some(Trigger::Timer));
    now.last_set_ms = None;
    assert_eq!(due(&idle, &now), Some(Trigger::Timer));
    let empty = Observed {
        now_ms: NOW,
        last_set_ms: None,
        ..Observed::default()
    };
    assert_eq!(due(&empty, &empty), Some(Trigger::Timer));
}

#[test]
fn ready_machine_gets_a_set_at_once() {
    let (prev, mut now) = turn_ended();
    now.git_locks.insert("a".to_string(), true);
    now.last_write_ms.insert("a".to_string(), NOW - 1);
    assert!(!quiet(&now));
    assert_eq!(due(&prev, &now), None);
    let home = TempDir::new("plan-ready");
    let data = home.path("data/Notchling");
    let plan = plan_set(
        &[],
        &BTreeMap::new(),
        home.root(),
        &data,
        3,
        Trigger::Ready(id("b")),
        &BTreeMap::new(),
    );
    assert_eq!(plan.trigger, Trigger::Ready(id("b")));
    assert_eq!(plan.id, "s-3");
    let last = plan_set(
        &[],
        &BTreeMap::new(),
        home.root(),
        &data,
        4,
        Trigger::Final,
        &BTreeMap::new(),
    );
    assert_eq!(last.trigger, Trigger::Final);
    assert_eq!(last.id, "s-4");
}

#[test]
fn git_lock_blocks_quiet() {
    let (prev, mut now) = turn_ended();
    now.git_locks.insert("a".to_string(), true);
    assert_eq!(due(&prev, &now), None);
    now.last_set_ms = None;
    assert_eq!(due(&prev, &now), None);
    now.git_locks.insert("a".to_string(), false);
    assert_eq!(due(&prev, &now), Some(Trigger::TurnEnd("a".to_string())));
}

#[test]
fn working_session_waits() {
    let mut idle = observed(&[("a", SessionState::Idle), ("b", SessionState::Working)]);
    idle.last_set_ms = Some(NOW - TIMER_MS * 3);
    assert_eq!(due(&idle, &idle), None);
    let mut prev = observed(&[("a", SessionState::Working), ("b", SessionState::Working)]);
    prev.last_set_ms = idle.last_set_ms;
    assert_eq!(due(&prev, &idle), Some(Trigger::TurnEnd("a".to_string())));
    let both_working = observed(&[("a", SessionState::Working)]);
    assert_eq!(due(&both_working, &both_working), None);
}

#[test]
fn recent_write_waits_ten_seconds() {
    let (prev, mut now) = turn_ended();
    now.last_write_ms
        .insert("a".to_string(), NOW - QUIET_MS + 1);
    assert_eq!(due(&prev, &now), None);
    now.last_write_ms.insert("a".to_string(), NOW - QUIET_MS);
    assert_eq!(due(&prev, &now), Some(Trigger::TurnEnd("a".to_string())));
}

#[test]
fn a_blocked_turn_end_is_remembered_until_quiet() {
    let (prev, mut blocked) = turn_ended();
    blocked.last_write_ms.insert("a".to_string(), NOW - 1_000);
    assert_eq!(due(&prev, &blocked), None);
    let kept = carry(&prev, &blocked);
    assert_eq!(kept.session_state["a"], SessionState::Working);
    assert_eq!(kept.last_write_ms, blocked.last_write_ms);
    let mut later = blocked.clone();
    later.now_ms += QUIET_MS;
    assert_eq!(due(&kept, &later), Some(Trigger::TurnEnd("a".to_string())));
    let again = carry(&kept, &later);
    assert_eq!(again, later);
    let (prev, quiet_now) = turn_ended();
    assert_eq!(carry(&prev, &quiet_now), quiet_now);
}

fn main_session(id: &str, config: &Path, transcript: &Path) -> MainSession {
    MainSession {
        id: id.to_string(),
        tool: Tool::Claude,
        cwd: "/work/repo".to_string(),
        config_dir: Some(config.to_string_lossy().into_owned()),
        transcript: Some(transcript.to_string_lossy().into_owned()),
        respawn: Vec::new(),
        since_ms: 1,
    }
}

fn facts(root: &Path) -> RepoFacts {
    RepoFacts {
        root: root.to_path_buf(),
        origin: Some("git@example.com:o/r.git".to_string()),
        branch: Some("main".to_string()),
        head: "abc123".to_string(),
        upstream: Some("origin/main".to_string()),
        unpushed: 0,
        dirty: false,
        untracked: false,
        submodules: false,
        main_repo: None,
    }
}

struct Rig {
    home: TempDir,
}

impl Rig {
    fn new(tag: &str) -> Rig {
        Rig {
            home: TempDir::new(tag),
        }
    }

    fn data(&self) -> PathBuf {
        self.home.path("support/Notchling")
    }

    fn session(&self, id: &str, text: &str) -> MainSession {
        let transcript = self
            .home
            .write(&format!(".claude/projects/-p/{id}.jsonl"), text);
        main_session(id, &self.home.path(".claude"), &transcript)
    }

    fn plan(
        &self,
        main: &[MainSession],
        repos: &BTreeMap<String, Option<RepoFacts>>,
        acked: &BTreeMap<PathBuf, u64>,
    ) -> SetPlan {
        plan_set(
            main,
            repos,
            self.home.root(),
            &self.data(),
            7,
            Trigger::Timer,
            acked,
        )
    }
}

#[test]
fn turn_end_set_carries_only_the_new_transcript_tail() {
    let rig = Rig::new("plan-tail");
    let m = rig.session("abc", &"x".repeat(1000));
    let path = PathBuf::from(m.transcript.clone().expect("transcript"));
    let acked = BTreeMap::from([(path.clone(), 600u64)]);
    let repos = BTreeMap::from([("abc".to_string(), Some(facts(Path::new("/work/repo"))))]);
    let plan = rig.plan(&[m], &repos, &acked);
    assert_eq!(plan.tails, vec![(path.clone(), 600)]);
    assert!(!plan.files.contains(&path));
    assert!(plan
        .files
        .contains(&rig.home.path(".claude/projects/-p/abc")));
    assert!(plan
        .files
        .contains(&rig.home.path(".claude/file-history/abc")));
    let first = rig.plan(
        &[rig.session("abc", &"x".repeat(1000))],
        &repos,
        &BTreeMap::new(),
    );
    assert_eq!(first.tails, vec![(path, 0)]);
}

#[test]
fn transcript_goes_from_smallest_acked_length() {
    let a = PathBuf::from("/h/a.jsonl");
    let b = PathBuf::from("/h/b.jsonl");
    let one = BTreeMap::from([(a.clone(), 800u64), (b.clone(), 50)]);
    let two = BTreeMap::from([(a.clone(), 300u64)]);
    let both = smallest_acked(&[one.clone(), two]);
    assert_eq!(both, BTreeMap::from([(a.clone(), 300), (b, 0)]));
    assert_eq!(smallest_acked(std::slice::from_ref(&one)), one);
    assert!(smallest_acked(&[]).is_empty());

    let rig = Rig::new("plan-smallest");
    let m = rig.session("abc", &"x".repeat(1000));
    let path = PathBuf::from(m.transcript.clone().expect("transcript"));
    let acked = smallest_acked(&[
        BTreeMap::from([(path.clone(), 900u64)]),
        BTreeMap::from([(path.clone(), 250u64)]),
    ]);
    let plan = rig.plan(&[m], &BTreeMap::new(), &acked);
    assert_eq!(plan.tails, vec![(path, 250)]);
}

#[test]
fn devices_json_is_in_every_set() {
    let rig = Rig::new("plan-devices");
    let devices = rig.home.path("support/AgentConsole/bridge/devices.json");
    let triggers = [
        Trigger::TurnEnd("a".to_string()),
        Trigger::Timer,
        Trigger::Ready(id("b")),
        Trigger::Final,
    ];
    for trigger in triggers {
        let plan = plan_set(
            &[],
            &BTreeMap::new(),
            rig.home.root(),
            &rig.data(),
            1,
            trigger,
            &BTreeMap::new(),
        );
        assert!(plan.files.contains(&devices), "{plan:?}");
        assert!(plan.files.contains(&rig.data().join("pages")));
        assert_eq!(plan.dbs, vec![rig.data().join("cluster/cluster.db")]);
    }
}

#[test]
fn no_code_is_ever_planned() {
    let rig = Rig::new("plan-nocode");
    let one = rig.session("one", "x");
    let two = rig.session("two", "y");
    let root = rig.home.path("work/repo");
    let repos = BTreeMap::from([
        ("one".to_string(), Some(facts(&root))),
        ("two".to_string(), Some(facts(&rig.home.path("work/other")))),
    ]);
    let plan = rig.plan(&[one, two], &repos, &BTreeMap::new());
    let all: Vec<&PathBuf> = plan
        .files
        .iter()
        .chain(plan.dbs.iter())
        .chain(plan.tails.iter().map(|(p, _)| p))
        .collect();
    for entry in &plan.sessions {
        let root = &entry.repo.as_ref().expect("facts kept").root;
        assert!(
            all.iter()
                .all(|p| !p.starts_with(root) && !root.starts_with(p)),
            "{root:?} in {all:?}"
        );
    }
    assert_eq!(plan.sessions.len(), 2);
    assert!(plan.sessions.iter().all(|s| s.movable == Movable::Yes));
}

#[test]
fn a_repository_holding_planned_paths_is_left_out_and_cannot_move() {
    let rig = Rig::new("plan-dotfiles");
    let dotfiles = rig.session("dot", "x");
    let elsewhere_config = rig.home.path("other/.claude");
    let elsewhere_transcript = rig.home.write("other/.claude/projects/-q/two.jsonl", "y");
    let two = main_session("two", &elsewhere_config, &elsewhere_transcript);
    let repos = BTreeMap::from([
        ("dot".to_string(), Some(facts(rig.home.root()))),
        ("two".to_string(), Some(facts(Path::new("/work/two")))),
    ]);
    let plan = rig.plan(&[dotfiles, two], &repos, &BTreeMap::new());
    let dot = plan.sessions.iter().find(|s| s.id == "dot").expect("dot");
    assert_eq!(dot.repo, None);
    match &dot.movable {
        Movable::No(why) => assert!(why.contains("holds files a set carries"), "{why}"),
        Movable::Yes => panic!("a repository holding planned paths must not move"),
    }
    let own = rig.home.path(".claude");
    assert!(plan.tails.iter().all(|(p, _)| !p.starts_with(&own)));
    assert!(plan.files.iter().all(|p| !p.starts_with(&own)));
    assert!(plan.tails.iter().any(|(p, _)| *p == elsewhere_transcript));
    let other = plan.sessions.iter().find(|s| s.id == "two").expect("two");
    assert_eq!(other.movable, Movable::Yes);
    assert!(other.repo.is_some());
}

#[test]
fn an_unread_or_missing_repository_cannot_move() {
    let rig = Rig::new("plan-unread");
    let a = rig.session("a", "x");
    let b = rig.session("b", "y");
    let repos = BTreeMap::from([("b".to_string(), None)]);
    let plan = rig.plan(&[a, b], &repos, &BTreeMap::new());
    let a = plan.sessions.iter().find(|s| s.id == "a").expect("a");
    let b = plan.sessions.iter().find(|s| s.id == "b").expect("b");
    assert!(
        matches!(&a.movable, Movable::No(why) if why.contains("couldn't be read")),
        "{:?}",
        a.movable
    );
    assert!(
        matches!(&b.movable, Movable::No(why) if why.contains("isn't a git repository")),
        "{:?}",
        b.movable
    );
    assert!(a.repo.is_none() && b.repo.is_none());
    assert_eq!(a.cwd, "/work/repo");
    assert_eq!(a.tool, Tool::Claude);
}

#[test]
fn a_tail_over_the_size_limit_is_skipped_but_a_big_file_with_a_small_tail_goes() {
    let rig = Rig::new("plan-bigtail");
    let m = rig.session("big", "");
    let path = PathBuf::from(m.transcript.clone().expect("transcript"));
    std::fs::File::create(&path)
        .and_then(|f| f.set_len(MAX_FILE + 10))
        .expect("big");
    let whole = rig.plan(std::slice::from_ref(&m), &BTreeMap::new(), &BTreeMap::new());
    assert!(whole.tails.is_empty());
    let acked = BTreeMap::from([(path.clone(), MAX_FILE + 5)]);
    let small = rig.plan(&[m], &BTreeMap::new(), &acked);
    assert_eq!(small.tails, vec![(path, MAX_FILE + 5)]);
}

#[test]
fn excluded_paths_are_never_planned() {
    let rig = Rig::new("plan-excluded");
    let transcript = rig
        .home
        .write("support/Notchling/cluster/logins/x/t.jsonl", "x");
    let m = main_session("abc", &rig.home.path(".claude"), &transcript);
    let plan = rig.plan(&[m], &BTreeMap::new(), &BTreeMap::new());
    assert!(plan.tails.is_empty());
    assert!(plan
        .files
        .iter()
        .all(|p| !p.starts_with(rig.data().join("cluster/logins"))));
}

fn planned_inside(plan: &SetPlan, roots: &[&Path]) -> Vec<PathBuf> {
    plan.files
        .iter()
        .chain(plan.dbs.iter())
        .chain(plan.tails.iter().map(|(p, _)| p))
        .filter(|p| roots.iter().any(|r| p.starts_with(r)))
        .cloned()
        .collect()
}

#[cfg(unix)]
#[test]
fn a_symlinked_ancestor_of_a_path_not_yet_on_disk_still_counts_as_inside() {
    let rig = Rig::new("plan-symlink");
    let real = rig.home.path("real");
    std::fs::create_dir_all(real.join("repo")).expect("repo");
    let link = rig.home.path("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let root = std::fs::canonicalize(real.join("repo")).expect("canonical");
    let config = link.join("repo/.claude");
    let transcript = config.join("projects/-p/abc.jsonl");
    let m = main_session("abc", &config, &transcript);
    let repos = BTreeMap::from([("abc".to_string(), Some(facts(&root)))]);
    let plan = rig.plan(&[m], &repos, &BTreeMap::new());
    let entry = &plan.sessions[0];
    assert_eq!(entry.repo, None);
    assert!(matches!(&entry.movable, Movable::No(why) if why.contains("holds files")));
    assert_eq!(
        planned_inside(&plan, &[&root, &link.join("repo")]),
        Vec::<PathBuf>::new()
    );
}

#[cfg(unix)]
#[test]
fn a_path_that_is_a_symlink_out_of_the_repository_is_still_inside_by_name() {
    let rig = Rig::new("plan-symlink-out");
    let real = rig.home.path("real");
    std::fs::create_dir_all(real.join("repo/.claude/projects")).expect("repo");
    let link = rig.home.path("link");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let root = std::fs::canonicalize(real.join("repo")).expect("canonical");
    let outside = rig.home.write("outside/dir/abc.jsonl", "y");
    let target = outside.parent().expect("dir").to_path_buf();
    std::os::unix::fs::symlink(&target, root.join(".claude/projects/-p")).expect("inner");
    let config = link.join("repo/.claude");
    let transcript = config.join("projects/-p/abc.jsonl");
    assert!(super::plan::holds(&root, &transcript));
    let m = main_session("abc", &config, &transcript);
    let repos = BTreeMap::from([("abc".to_string(), Some(facts(&root)))]);
    let plan = rig.plan(&[m], &repos, &BTreeMap::new());
    assert_eq!(plan.sessions[0].repo, None);
    assert_eq!(
        planned_inside(&plan, &[&root, &link.join("repo"), &target]),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn a_planned_directory_holding_a_repository_is_left_out() {
    let rig = Rig::new("plan-pages-repo");
    let site = rig.data().join("pages/site");
    std::fs::create_dir_all(site.join(".git")).expect("repo");
    let root = std::fs::canonicalize(&site).expect("canonical");
    let first = rig.session("abc", "x");
    let second = rig.session("two", "y");
    let repos = BTreeMap::from([
        ("abc".to_string(), Some(facts(&root))),
        ("two".to_string(), Some(facts(Path::new("/work/two")))),
    ]);
    let plan = rig.plan(&[first, second], &repos, &BTreeMap::new());
    let entry = plan.sessions.iter().find(|s| s.id == "abc").expect("abc");
    assert_eq!(entry.repo, None);
    assert!(matches!(entry.movable, Movable::No(_)));
    assert!(!plan.files.contains(&rig.data().join("pages")));
    assert!(plan
        .files
        .contains(&rig.home.path("support/AgentConsole/bridge/devices.json")));
    assert!(plan.tails.iter().any(|(p, _)| p.ends_with("two.jsonl")));
    let alone = rig.plan(
        &[rig.session("two", "y")],
        &BTreeMap::new(),
        &BTreeMap::new(),
    );
    assert!(alone.files.contains(&rig.data().join("pages")));
}

#[test]
fn a_linked_worktrees_main_repository_inside_a_planned_directory_is_left_out() {
    let rig = Rig::new("plan-main-repo");
    let main_repo = rig.data().join("pages/site");
    std::fs::create_dir_all(&main_repo).expect("main");
    let mut linked = facts(Path::new("/work/linked"));
    linked.main_repo = Some(std::fs::canonicalize(&main_repo).expect("canonical"));
    let m = rig.session("abc", "x");
    let repos = BTreeMap::from([("abc".to_string(), Some(linked))]);
    let plan = rig.plan(&[m], &repos, &BTreeMap::new());
    assert!(!plan.files.contains(&rig.data().join("pages")));
    assert_eq!(plan.sessions[0].repo, None);
}

#[test]
fn a_session_entry_names_the_files_its_restore_needs() {
    let rig = Rig::new("plan-entry-paths");
    let m = rig.session("abc", "x");
    let path = PathBuf::from(m.transcript.clone().expect("transcript"));
    let repos = BTreeMap::from([("abc".to_string(), Some(facts(Path::new("/work/repo"))))]);
    let plan = rig.plan(&[m], &repos, &BTreeMap::new());
    let entry = &plan.sessions[0];
    assert_eq!(entry.transcripts, vec![path]);
    assert!(entry
        .files
        .contains(&rig.home.path(".claude/projects/-p/abc")));
    assert!(entry
        .files
        .contains(&rig.home.path(".claude/file-history/abc")));
}
