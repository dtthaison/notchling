use super::blob::sha256_of;
use super::id::Tool;
use super::members::Term;
use super::restore::{
    blank_filters, drivers_probe, git_calls, origin_key, plan_restore, PathState, RestoreCtx,
    RestoreStep,
};
use super::snapshot::{
    dir_digest, Manifest, Movable, Part, PartKind, RepoFacts, SessionEntry, Shape,
};
use super::standby_store::StandbyStore;
use super::testkit::id;
use crate::sessions::testutil::TempDir;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

const HEAD: &str = "1111111111111111111111111111111111111111";
const OLDER: &str = "2222222222222222222222222222222222222222";
const S1: &str = "aaaaaaaa-1111-4111-8111-aaaaaaaaaaaa";
const S2: &str = "aaaaaaaa-2222-4222-8222-aaaaaaaaaaaa";
const CODEX: &str = "bbbbbbbb-1111-4111-8111-bbbbbbbbbbbb";
const CODEX_OTHER: &str = "bbbbbbbb-2222-4222-8222-bbbbbbbbbbbb";

struct Rig {
    tmp: TempDir,
    home: PathBuf,
    cluster: PathBuf,
    parts: Vec<(Part, Vec<u8>)>,
    sessions: Vec<SessionEntry>,
    path_dirs: Vec<PathBuf>,
}

fn node() -> super::NodeId {
    id("primary")
}

fn facts(root: &Path) -> RepoFacts {
    RepoFacts {
        root: root.to_path_buf(),
        origin: Some("git@example.invalid:o/r.git".to_owned()),
        branch: Some("main".to_owned()),
        head: HEAD.to_owned(),
        upstream: Some("origin/main".to_owned()),
        unpushed: 0,
        dirty: false,
        untracked: false,
        submodules: false,
        main_repo: None,
    }
}

impl Rig {
    fn new(tag: &str) -> Rig {
        let tmp = TempDir::new(tag);
        let home = tmp.path("home");
        fs::create_dir_all(&home).unwrap();
        let cluster = home.join("data/cluster");
        Rig {
            tmp,
            home,
            cluster,
            parts: Vec::new(),
            sessions: Vec::new(),
            path_dirs: Vec::new(),
        }
    }

    fn at(&self, rel: &str) -> PathBuf {
        self.home.join(rel)
    }

    fn session(&mut self, id: &str, repo: &str, movable: Movable) -> &mut SessionEntry {
        let root = self.at(repo);
        self.sessions.push(SessionEntry {
            id: id.to_owned(),
            tool: Tool::Claude,
            cwd: root.to_string_lossy().into_owned(),
            repo: Some(facts(&root)),
            movable,
            transcripts: Vec::new(),
            files: Vec::new(),
        });
        self.sessions.last_mut().unwrap()
    }

    fn tail(&mut self, path: &Path, from: u64, bytes: &[u8]) {
        let body = bytes[from as usize..].to_vec();
        self.parts.push((
            Part {
                kind: PartKind::Tail {
                    from,
                    rewritten: false,
                },
                path: path.to_path_buf(),
                sha256: sha256_of(&body),
                len: body.len() as u64,
            },
            body,
        ));
    }

    fn whole(&mut self, kind: PartKind, path: &Path, bytes: &[u8]) {
        self.parts.push((
            Part {
                kind,
                path: path.to_path_buf(),
                sha256: sha256_of(bytes),
                len: bytes.len() as u64,
            },
            bytes.to_vec(),
        ));
    }

    fn manifest(&self) -> Manifest {
        let store = StandbyStore::new(&self.cluster, &node());
        let m = Manifest {
            set: "s-1".to_owned(),
            seq: 1,
            term: Term { n: 1, node: node() },
            home: node(),
            created_ms: 1,
            sessions: self.sessions.clone(),
            parts: self.parts.iter().map(|(p, _)| p.clone()).collect(),
        };
        if store.current().is_some() {
            return m;
        }
        store.begin(&m).unwrap();
        for (n, (part, body)) in self.parts.iter().enumerate() {
            let from = self.tmp.path(&format!("src/{n}"));
            fs::create_dir_all(from.parent().unwrap()).unwrap();
            if part.kind == PartKind::Dir {
                fs::create_dir_all(&from).unwrap();
            } else {
                fs::write(&from, body).unwrap();
            }
            store.part("s-1", part, &from).unwrap();
        }
        store.commit("s-1").unwrap();
        m
    }

    fn plan(
        &self,
        ids: &[&str],
        aside: bool,
        shared: bool,
        probe: &dyn Fn(&Path) -> PathState,
    ) -> Result<Vec<RestoreStep>, String> {
        let m = self.manifest();
        let ctx = RestoreCtx {
            cluster: &self.cluster,
            home: &self.home,
            node: "m1",
            aside,
            shared,
            path_dirs: &self.path_dirs,
        };
        let named: BTreeSet<String> = ids.iter().map(|s| (*s).to_owned()).collect();
        plan_restore(&m, &ctx, &named, probe)
    }
}

fn missing(_: &Path) -> PathState {
    PathState::Missing
}

fn clean(branch: &str, head: &str) -> impl Fn(&Path) -> PathState {
    let (branch, head) = (branch.to_owned(), head.to_owned());
    move |_| PathState::Clean {
        branch: branch.clone(),
        head: head.clone(),
    }
}

#[test]
fn a_missing_folder_is_cloned_at_its_commit() {
    let mut rig = Rig::new("restore-clone");
    rig.session(S1, "code/a", Movable::Yes);
    let steps = rig.plan(&[S1], false, false, &missing).unwrap();
    assert_eq!(
        steps,
        vec![RestoreStep::CloneRepo {
            origin: "git@example.invalid:o/r.git".to_owned(),
            path: rig.at("code/a"),
            branch: "main".to_owned(),
            head: HEAD.to_owned(),
        }]
    );
}

#[test]
fn a_clean_clone_behind_the_set_is_fast_forwarded_and_one_at_the_commit_needs_nothing() {
    let mut rig = Rig::new("restore-ff");
    rig.session(S1, "code/a", Movable::Yes);
    let steps = rig
        .plan(&[S1], false, false, &clean("main", OLDER))
        .unwrap();
    assert_eq!(
        steps,
        vec![RestoreStep::FastForward {
            path: rig.at("code/a"),
            branch: "main".to_owned(),
            head: HEAD.to_owned(),
        }]
    );
    assert!(rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap()
        .is_empty());
}

#[test]
fn restore_refuses_a_folder_with_its_own_changes() {
    let mut rig = Rig::new("restore-changed");
    rig.session(S1, "code/a", Movable::Yes);
    let why = rig
        .plan(&[S1], false, false, &|_| PathState::Changed)
        .unwrap_err();
    assert_eq!(
        why,
        format!("{} on m1 has its own changes", rig.at("code/a").display())
    );
    assert!(rig
        .plan(&[S1], true, false, &|_| PathState::Changed)
        .is_err());
}

#[test]
fn restore_refuses_a_clone_on_another_branch() {
    let mut rig = Rig::new("restore-branch");
    rig.session(S1, "code/a", Movable::Yes);
    let why = rig
        .plan(&[S1], false, false, &clean("feature", HEAD))
        .unwrap_err();
    assert_eq!(
        why,
        format!("{} on m1 is on feature", rig.at("code/a").display())
    );
}

#[test]
fn unmovable_session_is_not_restored() {
    let mut rig = Rig::new("restore-unmovable");
    rig.session(
        S1,
        "code/a",
        Movable::No("/x has uncommitted changes".to_owned()),
    );
    rig.session(S2, "code/b", Movable::Yes);
    let why = rig.plan(&[S1], false, false, &missing).unwrap_err();
    assert_eq!(why, format!("{S1} can't move: /x has uncommitted changes"));
    let steps = rig.plan(&[S2], false, false, &missing).unwrap();
    assert_eq!(steps.len(), 1);
    assert!(rig.plan(&["gone"], false, false, &missing).is_err());
    assert!(rig.plan(&[], false, false, &missing).unwrap().is_empty());
}

#[test]
fn a_linked_worktree_needs_its_main_repository_there() {
    let mut rig = Rig::new("restore-worktree");
    let main = rig.at("code/main");
    rig.session(S1, "code/wt", Movable::Yes);
    rig.sessions[0].repo.as_mut().unwrap().main_repo = Some(main.clone());
    let why = rig.plan(&[S1], false, false, &missing).unwrap_err();
    assert!(why.starts_with(&main.display().to_string()), "{why}");
    let wt = rig.at("code/wt");
    let steps = rig
        .plan(&[S1], false, false, &move |path| {
            if path == wt {
                PathState::Missing
            } else {
                PathState::Changed
            }
        })
        .unwrap();
    assert_eq!(
        steps,
        vec![RestoreStep::AddWorktree {
            main,
            origin: "git@example.invalid:o/r.git".to_owned(),
            path: rig.at("code/wt"),
            branch: "main".to_owned(),
            head: HEAD.to_owned(),
        }]
    );
}

#[test]
fn a_shared_repository_is_planned_once_and_at_one_commit() {
    let mut rig = Rig::new("restore-shared-repo");
    rig.session(S1, "code/a", Movable::Yes);
    rig.session(S2, "code/a", Movable::Yes);
    assert_eq!(
        rig.plan(&[S1, S2], false, false, &missing).unwrap().len(),
        1
    );
    rig.sessions[1].repo.as_mut().unwrap().head = OLDER.to_owned();
    let why = rig.plan(&[S1, S2], false, false, &missing).unwrap_err();
    assert!(why.contains("two commits"), "{why}");
}

fn with_transcript(rig: &mut Rig, local: Option<&[u8]>, stored: &[u8]) -> PathBuf {
    let path = rig.at(&format!(".claude/projects/p/{S1}.jsonl"));
    rig.session(S1, "code/a", Movable::Yes).transcripts = vec![path.clone()];
    rig.tail(&path, 0, stored);
    if let Some(bytes) = local {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
    }
    path
}

#[test]
fn a_transcript_is_put_appended_or_left_alone() {
    let mut rig = Rig::new("restore-transcript");
    let path = with_transcript(&mut rig, None, b"one\ntwo\nthree\n");
    let steps = rig.plan(&[S1], false, false, &clean("main", HEAD)).unwrap();
    assert!(matches!(&steps[..], [RestoreStep::PutFile { to, len: 14, .. }] if *to == path));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"one\ntwo\n").unwrap();
    let steps = rig.plan(&[S1], false, false, &clean("main", HEAD)).unwrap();
    assert!(matches!(
        &steps[..],
        [RestoreStep::AppendTail {
            offset: 8,
            end: 14,
            ..
        }]
    ));
    fs::write(&path, b"one\ntwo\nthree\n").unwrap();
    assert!(rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap()
        .is_empty());
}

#[test]
fn longer_local_transcript_refuses() {
    let mut rig = Rig::new("restore-longer");
    with_transcript(
        &mut rig,
        Some(b"one\ntwo\nthree\nfour\n"),
        b"one\ntwo\nthree\n",
    );
    let why = rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap_err();
    assert_eq!(why, format!("{S1} ran on m1 after this set"));
}

#[test]
fn a_different_local_transcript_refuses_whatever_its_length() {
    let mut rig = Rig::new("restore-different");
    with_transcript(&mut rig, Some(b"one\ntwx\nthree\n"), b"one\ntwo\nthree\n");
    let why = rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap_err();
    assert_eq!(why, format!("{S1} ran on m1 after this set"));
    let mut shorter = Rig::new("restore-different-short");
    with_transcript(&mut shorter, Some(b"one\ntwx\n"), b"one\ntwo\nthree\n");
    assert!(shorter
        .plan(&[S1], false, false, &clean("main", HEAD))
        .is_err());
}

#[test]
fn aside_moves_the_local_transcript_first() {
    let mut rig = Rig::new("restore-aside");
    let path = with_transcript(&mut rig, Some(b"longer than the set\n"), b"one\n");
    let steps = rig.plan(&[S1], true, false, &clean("main", HEAD)).unwrap();
    let copy = rig
        .cluster
        .join(format!("copies/{S1}/s-1"))
        .join(path.strip_prefix("/").unwrap());
    assert!(
        matches!(&steps[0], RestoreStep::MoveAside { path: p, to } if *p == path && *to == copy)
    );
    assert!(matches!(&steps[1], RestoreStep::PutFile { to, .. } if *to == path));
    assert_eq!(steps.len(), 2);
}

#[test]
fn a_transcript_missing_from_the_set_refuses() {
    let mut rig = Rig::new("restore-no-transcript");
    let path = rig.at(&format!(".claude/projects/p/{S1}.jsonl"));
    rig.session(S1, "code/a", Movable::Yes).transcripts = vec![path];
    let why = rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap_err();
    assert!(
        why.starts_with(&format!("{S1} can't move: its transcript")),
        "{why}"
    );
}

#[test]
fn restore_steps_never_touch_excluded_dirs() {
    let mut rig = Rig::new("restore-excluded");
    let grants = rig.at("data/cluster/grants/s1.jsonl");
    rig.session(S1, "code/a", Movable::Yes).transcripts = vec![grants.clone()];
    rig.tail(&grants, 0, b"x\n");
    let why = rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap_err();
    assert!(why.contains("never carries"), "{why}");
    let mut shared = Rig::new("restore-excluded-shared");
    let login = shared.at("data/cluster/logins/x.json");
    let incoming = shared.at("data/cluster/incoming/x.db");
    shared.whole(PartKind::File, &login, b"{}");
    shared.whole(PartKind::Db, &incoming, b"db");
    assert!(shared.plan(&[], false, true, &missing).is_err());
    assert!(shared.plan(&[], false, false, &missing).unwrap().is_empty());
}

#[test]
fn paths_outside_the_home_folder_or_inside_a_repository_refuse() {
    let mut rig = Rig::new("restore-outside");
    let outside = PathBuf::from("/etc/s1.jsonl");
    rig.session(S1, "code/a", Movable::Yes).transcripts = vec![outside.clone()];
    rig.tail(&outside, 0, b"x\n");
    assert!(rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap_err()
        .contains("isn't under"));
    let mut inside = Rig::new("restore-inside");
    let hook = inside.at("code/a/.git/hooks/post-checkout");
    inside.session(S1, "code/a", Movable::Yes).transcripts = vec![hook.clone()];
    inside.tail(&hook, 0, b"x\n");
    assert!(inside
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap_err()
        .contains("code never travels"));
}

#[test]
fn shared_parts_are_restored_with_the_database_last() {
    let mut rig = Rig::new("restore-shared");
    let db = rig.at("data/cluster/cluster.db");
    let devices = rig.at("AgentConsole/bridge/devices.json");
    let pages = rig.at("data/pages");
    rig.whole(PartKind::Db, &db, b"db");
    rig.whole(PartKind::File, &devices, b"[]");
    rig.parts.push((
        Part {
            kind: PartKind::Dir,
            path: pages.clone(),
            sha256: dir_digest(&[]),
            len: 0,
        },
        Vec::new(),
    ));
    let steps = rig.plan(&[], false, true, &missing).unwrap();
    assert!(matches!(&steps[0], RestoreStep::MergeDevices { to, .. } if *to == devices));
    assert!(matches!(&steps[1], RestoreStep::PutDir { to, .. } if *to == pages));
    assert!(matches!(&steps[2], RestoreStep::RestoreDb { to, .. } if *to == db));
    assert_eq!(steps.len(), 3);
}

#[test]
fn a_codex_index_is_merged_for_its_session_only() {
    let mut rig = Rig::new("restore-index");
    let index = rig.at(".codex/session_index.jsonl");
    let rollout = rig.at(&format!(
        ".codex/sessions/2026/11/15/rollout-x-{CODEX}.jsonl"
    ));
    let entry = rig.session(&format!("codex:{CODEX}"), "code/a", Movable::Yes);
    entry.tool = Tool::Codex;
    entry.transcripts = vec![rollout.clone()];
    entry.files = vec![index.clone()];
    rig.tail(&rollout, 0, b"{}\n");
    rig.whole(
        PartKind::File,
        &index,
        b"{\"id\":\"abc\"}\n{\"id\":\"zzz\"}\n",
    );
    let steps = rig
        .plan(
            &[&format!("codex:{CODEX}")],
            false,
            false,
            &clean("main", HEAD),
        )
        .unwrap();
    assert!(steps.iter().any(|s| matches!(s,
        RestoreStep::MergeIndex { to, session, .. } if *to == index && session == CODEX)));
    assert!(!steps
        .iter()
        .any(|s| matches!(s, RestoreStep::PutFile { to, .. } if *to == index)));
}

fn empty_dir(rig: &mut Rig, path: &Path) {
    rig.parts.push((
        Part {
            kind: PartKind::Dir,
            path: path.to_path_buf(),
            sha256: dir_digest(&[]),
            len: 0,
        },
        Vec::new(),
    ));
}

#[test]
fn a_hostile_set_cannot_restore_a_shared_part_this_machine_doesnt_own() {
    let hostile: [(&str, PartKind); 5] = [
        (".ssh", PartKind::Dir),
        ("Library/LaunchAgents", PartKind::Dir),
        ("data/notchling.db", PartKind::Db),
        ("elsewhere/AgentConsole/bridge/devices.json", PartKind::File),
        ("data/cluster/other.db", PartKind::Db),
    ];
    for (n, (rel, kind)) in hostile.into_iter().enumerate() {
        let mut rig = Rig::new(&format!("restore-hostile-shared-{n}"));
        let path = rig.at(rel);
        if kind == PartKind::Dir {
            empty_dir(&mut rig, &path);
        } else {
            rig.whole(kind, &path, b"x");
        }
        let why = rig.plan(&[], false, true, &missing).unwrap_err();
        assert!(
            why.contains("shared part this machine doesn't restore"),
            "{rel}: {why}"
        );
        assert!(why.contains(rel), "{why}");
        assert!(rig.plan(&[], false, false, &missing).unwrap().is_empty());
    }
}

#[test]
fn a_session_can_only_restore_the_files_a_session_keeps() {
    let main = format!(".claude/projects/p/{S1}.jsonl");
    let cases: Vec<(String, Shape, bool)> = vec![
        (".zshrc".to_owned(), Shape::File, false),
        (".claude/settings.json".to_owned(), Shape::File, false),
        (main.clone(), Shape::Transcript, true),
        (
            format!(".claude/projects/p/q/{S1}.jsonl"),
            Shape::Transcript,
            false,
        ),
        (
            format!(".claude/other/{S1}.jsonl"),
            Shape::Transcript,
            false,
        ),
        (
            ".claude/projects/p/s1.jsonl".to_owned(),
            Shape::Transcript,
            false,
        ),
        (
            format!(".claude/projects/other/{S2}.jsonl"),
            Shape::Transcript,
            false,
        ),
        (".claude/todos/other-1.json".to_owned(), Shape::File, false),
        (format!(".claude/todos/{S1}-agent.json"), Shape::File, true),
        (
            format!(".claude-lanes/work/todos/{S1}-agent.json"),
            Shape::File,
            false,
        ),
        (".ssh/authorized_keys".to_owned(), Shape::Transcript, false),
        (".ssh".to_owned(), Shape::Dir, false),
        (".claude/projects/p/.git".to_owned(), Shape::Dir, false),
        (".claude/file-history/hooks".to_owned(), Shape::Dir, false),
        (format!(".claude/file-history/{S1}"), Shape::Dir, true),
        (format!(".claude/projects/p/{S1}"), Shape::Dir, true),
        (format!(".claude/todos/{S1}"), Shape::Dir, false),
        (format!(".claude/file-history/{S1}"), Shape::File, false),
    ];
    for (n, (rel, shape, ok)) in cases.into_iter().enumerate() {
        let mut rig = Rig::new(&format!("restore-shapes-{n}"));
        let path = rig.at(&rel);
        let main_path = rig.at(&main);
        let entry = rig.session(S1, "code/a", Movable::Yes);
        entry.transcripts = vec![main_path.clone()];
        match shape {
            Shape::Transcript => {
                if path != main_path {
                    entry.transcripts.insert(0, path.clone());
                }
            }
            _ => entry.files = vec![path.clone()],
        }
        rig.tail(&main_path, 0, b"x\n");
        match shape {
            Shape::Transcript if path != main_path => rig.tail(&path, 0, b"x\n"),
            Shape::Transcript => {}
            Shape::File => rig.whole(PartKind::File, &path, b"x"),
            Shape::Dir => empty_dir(&mut rig, &path),
        }
        let result = rig.plan(&[S1], false, false, &clean("main", HEAD));
        assert_eq!(result.is_ok(), ok, "{rel}: {result:?}");
        if let Err(why) = result {
            assert!(
                why.contains("isn't a file the session keeps"),
                "{rel}: {why}"
            );
        }
    }
}

#[test]
fn a_codex_session_keeps_only_its_own_rollout_and_the_index() {
    let mut rig = Rig::new("restore-codex-shapes");
    let own = rig.at(&format!(
        ".codex/sessions/2026/11/15/rollout-t-{CODEX}.jsonl"
    ));
    let other = rig.at(&format!(
        ".codex/sessions/2026/11/15/rollout-t-{CODEX_OTHER}.jsonl"
    ));
    let entry = rig.session(&format!("codex:{CODEX}"), "code/a", Movable::Yes);
    entry.tool = Tool::Codex;
    entry.transcripts = vec![own.clone(), other.clone()];
    rig.tail(&own, 0, b"{}\n");
    rig.tail(&other, 0, b"{}\n");
    let why = rig
        .plan(
            &[&format!("codex:{CODEX}")],
            false,
            false,
            &clean("main", HEAD),
        )
        .unwrap_err();
    assert!(why.contains(CODEX_OTHER), "{why}");
    assert!(!why.contains(&format!("rollout-t-{CODEX}.jsonl")), "{why}");
}

#[test]
fn an_id_that_is_not_a_uuid_keeps_nothing() {
    let mut rig = Rig::new("restore-short-id");
    let path = rig.at(".claude/projects/p/a.jsonl");
    let entry = rig.session("a", "code/a", Movable::Yes);
    entry.transcripts = vec![path.clone()];
    rig.tail(&path, 0, b"x\n");
    let why = rig
        .plan(&["a"], false, false, &clean("main", HEAD))
        .unwrap_err();
    assert!(why.contains("isn't a file the session keeps"), "{why}");
    let mut codex = Rig::new("restore-short-codex");
    let rollout = codex.at(".codex/sessions/2026/11/15/rollout-t-1.jsonl");
    let entry = codex.session("codex:1", "code/a", Movable::Yes);
    entry.tool = Tool::Codex;
    entry.transcripts = vec![rollout.clone()];
    codex.tail(&rollout, 0, b"{}\n");
    assert!(codex
        .plan(&["codex:1"], false, false, &clean("main", HEAD))
        .is_err());
}

fn hostile_root(rel: &str, tag: &str) -> Result<Vec<RestoreStep>, String> {
    let mut rig = Rig::new(tag);
    let root = rig.at(rel);
    rig.session(S1, "code/a", Movable::Yes);
    rig.sessions[0].cwd = root.to_string_lossy().into_owned();
    rig.sessions[0].repo.as_mut().unwrap().root = root;
    rig.plan(&[S1], false, false, &missing)
}

#[test]
fn a_repository_root_in_a_hidden_or_system_folder_is_refused() {
    let refused = [
        (".ssh", "hidden folder"),
        (".config/agents", "hidden folder"),
        (".claude/skills/x", "hidden folder"),
        ("Library/LaunchAgents", "system folder"),
        ("Library/Application Support/x", "system folder"),
        ("Applications/x", "system folder"),
        ("code/a/.git/modules/x", ".git folder"),
        ("library/LaunchAgents/x", "system folder"),
        ("LIBRARY/Application Support/x", "system folder"),
        ("APPLICATIONS/x", "system folder"),
        ("x/.GIT/hooks", ".git folder"),
        ("code/BIN/x", "bin folder"),
        ("code/sbin", "bin folder"),
    ];
    for (n, (rel, why)) in refused.into_iter().enumerate() {
        let err = hostile_root(rel, &format!("restore-root-{n}")).unwrap_err();
        assert!(err.contains(why), "{rel}: {err}");
    }
    assert!(hostile_root("code/a", "restore-root-ok").is_ok());
    assert!(hostile_root("Library/CloudStorage/Dropbox/a", "restore-root-cloud").is_ok());
    assert!(hostile_root("Library/CloudStorage", "restore-root-cloud-bare").is_err());
    assert!(hostile_root("LIBRARY/cloudstorage/Dropbox/a", "restore-root-cloud-case").is_ok());
}

#[test]
fn the_home_folder_itself_is_not_a_repository_root() {
    let mut rig = Rig::new("restore-home-root");
    let home = rig.home.clone();
    rig.session(S1, "code/a", Movable::Yes);
    rig.sessions[0].repo.as_mut().unwrap().root = home.clone();
    rig.sessions[0].cwd = home.to_string_lossy().into_owned();
    let why = rig.plan(&[S1], false, false, &missing).unwrap_err();
    assert!(why.contains("home folder itself"), "{why}");
}

#[test]
fn a_session_folder_outside_its_repository_is_refused() {
    let mut rig = Rig::new("restore-cwd");
    rig.session(S1, "code/a", Movable::Yes);
    rig.sessions[0].cwd = rig.at("code/b").to_string_lossy().into_owned();
    let why = rig.plan(&[S1], false, false, &missing).unwrap_err();
    assert!(why.contains("isn't inside its repository"), "{why}");
}

#[test]
fn nested_repositories_are_refused_except_a_worktree_in_its_main() {
    let mut rig = Rig::new("restore-nested");
    rig.session(S1, "code/a", Movable::Yes);
    rig.session(S2, "code/a/inner", Movable::Yes);
    let why = rig.plan(&[S1, S2], false, false, &missing).unwrap_err();
    assert!(why.contains("is inside"), "{why}");
    let mut linked = Rig::new("restore-nested-linked");
    let main = linked.at("code/a");
    linked.session(S1, "code/a", Movable::Yes);
    linked.session(S2, "code/a/.claude/worktrees/x", Movable::Yes);
    linked.sessions[1].repo.as_mut().unwrap().main_repo = Some(main);
    assert!(linked.plan(&[S1, S2], false, false, &missing).is_ok());
    assert!(hostile_root("go/src/x", "restore-root-go").is_ok());
    assert!(hostile_root("code/a/.git/hooks", "restore-root-hooks").is_err());
}

#[test]
fn a_clone_from_a_folder_on_the_primary_outside_home_is_refused() {
    let cases = [
        ("/etc/evil", false),
        ("file:///etc/evil", false),
        ("file:///var/../etc", false),
        ("git@example.invalid:o/r.git", true),
        ("https://example.invalid/o/r.git", true),
    ];
    for (n, (origin, ok)) in cases.into_iter().enumerate() {
        let mut rig = Rig::new(&format!("restore-origin-{n}"));
        rig.session(S1, "code/a", Movable::Yes);
        rig.sessions[0].repo.as_mut().unwrap().origin = Some(origin.to_owned());
        let result = rig.plan(&[S1], false, false, &missing);
        assert_eq!(result.is_ok(), ok, "{origin}: {result:?}");
    }
    let mut under = Rig::new("restore-origin-home");
    under.session(S1, "code/a", Movable::Yes);
    let local = under.at("mirrors/r.git");
    under.sessions[0].repo.as_mut().unwrap().origin = Some(local.to_string_lossy().into_owned());
    assert!(under.plan(&[S1], false, false, &missing).is_ok());
    under.sessions[0].repo.as_mut().unwrap().origin = Some(format!("file://{}", local.display()));
    assert!(under.plan(&[S1], false, false, &missing).is_ok());
}

#[test]
fn blanking_filters_names_every_driver_the_folder_defines() {
    let step = RestoreStep::FastForward {
        path: PathBuf::from("/h/code/a"),
        branch: "main".to_owned(),
        head: HEAD.to_owned(),
    };
    let calls = git_calls(&step);
    let probe = drivers_probe(Path::new("/h/code/a"));
    assert!(probe.allow_failure);
    assert!(probe.argv().join(" ").contains("--get-regexp ^filter"));
    let found = "filter.lfs.clean\ngit-lfs clean\0filter.x.smudge\nboom\0";
    let joined = blank_filters(&calls[3], found).unwrap().join(" ");
    for name in ["lfs", "x"] {
        for key in ["clean", "smudge", "process"] {
            assert!(
                joined.contains(&format!("filter.{name}.{key}= ")),
                "{joined}"
            );
        }
        assert!(
            joined.contains(&format!("filter.{name}.required=false")),
            "{joined}"
        );
    }
    assert!(joined.contains("core.hooksPath=/dev/null"), "{joined}");
    assert!(
        joined.contains("-C /h/code/a merge -q --ff-only"),
        "{joined}"
    );
    let plain = blank_filters(&calls[3], "").unwrap().join(" ");
    assert!(!plain.contains("filter."), "{plain}");
}

#[test]
fn a_main_repository_that_is_not_a_repository_is_refused() {
    let mut rig = Rig::new("restore-foreign-main");
    let main = rig.at("code/main");
    rig.session(S1, "code/wt", Movable::Yes);
    rig.sessions[0].repo.as_mut().unwrap().main_repo = Some(main.clone());
    let wt = rig.at("code/wt");
    let why = rig
        .plan(&[S1], false, false, &move |path| {
            if path == wt {
                PathState::Missing
            } else {
                PathState::Foreign
            }
        })
        .unwrap_err();
    assert!(why.contains("isn't a repository"), "{why}");
    assert!(why.starts_with(&main.display().to_string()), "{why}");
    let mut root = Rig::new("restore-foreign-root");
    root.session(S1, "code/a", Movable::Yes);
    let why = root
        .plan(&[S1], false, false, &|_| PathState::Foreign)
        .unwrap_err();
    assert!(why.contains("isn't a repository"), "{why}");
}

#[test]
fn git_calls_are_hardened_and_never_reach_for_a_login() {
    let step = RestoreStep::CloneRepo {
        origin: "git@example.invalid:o/r.git".to_owned(),
        path: PathBuf::from("/h/code/a"),
        branch: "main".to_owned(),
        head: HEAD.to_owned(),
    };
    let calls = git_calls(&step);
    assert!(calls.len() >= 4);
    for call in &calls {
        let argv = call.argv().join(" ");
        assert!(argv.contains("core.hooksPath=/dev/null"), "{argv}");
        assert!(argv.contains("protocol.ext.allow=never"), "{argv}");
        assert!(argv.contains("core.fsmonitor=false"), "{argv}");
    }
    assert!(calls[0]
        .argv()
        .join(" ")
        .contains("clone --no-checkout --no-tags -- git@"));
    assert!(git_calls(&RestoreStep::MergeDevices {
        from: PathBuf::new(),
        to: PathBuf::new(),
        sha256: String::new()
    })
    .is_empty());
}

#[test]
fn a_root_that_is_or_holds_a_path_folder_under_home_is_refused() {
    let tools = |rig: &Rig| vec![rig.at("code/tools"), PathBuf::from("/usr/bin")];
    for (n, (rel, refused)) in [
        ("code/tools", true),
        ("code", true),
        ("CODE/Tools", true),
        ("code/tools/x", false),
        ("code/other", false),
        ("usr", false),
    ]
    .into_iter()
    .enumerate()
    {
        let mut rig = Rig::new(&format!("restore-path-{n}"));
        rig.path_dirs = tools(&rig);
        let root = rig.at(rel);
        rig.session(S1, "code/a", Movable::Yes);
        rig.sessions[0].cwd = root.to_string_lossy().into_owned();
        rig.sessions[0].repo.as_mut().unwrap().root = root;
        let result = rig.plan(&[S1], false, false, &missing);
        assert_eq!(result.is_err(), refused, "{rel}: {result:?}");
        if let Err(why) = result {
            assert!(why.contains("PATH"), "{rel}: {why}");
        }
    }
}

#[test]
fn nested_roots_are_found_whatever_their_case() {
    let mut rig = Rig::new("restore-nested-case");
    rig.session(S1, "code/a", Movable::Yes);
    rig.session(S2, "CODE/A/inner", Movable::Yes);
    let why = rig.plan(&[S1, S2], false, false, &missing).unwrap_err();
    assert!(why.contains("is inside"), "{why}");
    let mut linked = Rig::new("restore-nested-case-linked");
    let main = linked.at("code/a");
    linked.session(S1, "code/a", Movable::Yes);
    linked.session(S2, "code/a/wt", Movable::Yes);
    linked.sessions[1].repo.as_mut().unwrap().main_repo = Some(main);
    assert!(linked.plan(&[S1, S2], false, false, &missing).is_ok());
    linked.sessions[1].repo.as_mut().unwrap().main_repo = Some(linked.at("CODE/A"));
    assert!(linked.plan(&[S1, S2], false, false, &missing).is_err());
}

#[test]
fn session_files_inside_a_root_are_refused_whatever_their_case() {
    let mut rig = Rig::new("restore-inside-case");
    let hook = rig.at("CODE/A/.git/hooks/post-checkout");
    rig.session(S1, "code/a", Movable::Yes).transcripts = vec![hook.clone()];
    rig.tail(&hook, 0, b"x\n");
    assert!(rig
        .plan(&[S1], false, false, &clean("main", HEAD))
        .unwrap_err()
        .contains("code never travels"));
}

#[test]
fn origins_that_name_one_remote_share_a_key() {
    let same = [
        "git@github.com:me/r.git",
        "git@GitHub.com:me/r",
        "ssh://git@github.com/me/r.git",
        "ssh://git@github.com:22/me/r/",
        "https://github.com/me/r.git",
        "https://me@GITHUB.com:443/me/r/",
        "git://github.com/me/r",
    ];
    for url in same {
        assert_eq!(origin_key(url), "github.com/me/r", "{url}");
    }
    assert_ne!(origin_key("https://github.com/me/R"), "github.com/me/r");
    assert_ne!(
        origin_key("https://github.com/me/r"),
        origin_key("https://gitlab.com/me/r")
    );
    assert_ne!(
        origin_key("ssh://git@github.com:2222/me/r"),
        origin_key("ssh://git@github.com/me/r")
    );
    assert_eq!(origin_key("/srv/git/r.git/"), "/srv/git/r");
    assert_eq!(origin_key("file:///srv/git/r.git"), "/srv/git/r");
}
