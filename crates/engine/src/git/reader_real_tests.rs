use super::read::Failure;
use super::reader::Reader;
use super::refs::{parse_left_right, parse_shortstat, Counts, Lines};
use crate::control::launcher::{FakeLauncher, LaunchOutput};
use crate::control::{Launcher, RealLauncher};
use crate::sessions::testutil::TempDir;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime};

fn run_git(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@example.com"])
        .args(["-c", "commit.gpgsign=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let out = run_git(dir, args);
    assert!(out.status.success(), "git {args:?} failed");
}

fn git_out(dir: &Path, args: &[&str]) -> String {
    let out = run_git(dir, args);
    assert!(out.status.success(), "git {args:?} failed");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn launcher() -> Box<dyn Launcher> {
    let real = RealLauncher::with_claude(PathBuf::from("/nonexistent/claude"));
    #[cfg(target_os = "macos")]
    return Box::new(crate::git::SandboxedLauncher::waiting(
        std::sync::Arc::new(real),
        Duration::from_secs(60),
    ));
    #[cfg(not(target_os = "macos"))]
    Box::new(real)
}

fn lines(prefix: &str, n: usize) -> String {
    (0..n).map(|i| format!("{prefix}{i}\n")).collect()
}

struct Rig {
    t: TempDir,
    repo: PathBuf,
    lane_a: PathBuf,
    lane_b: PathBuf,
}

impl Rig {
    fn cwd(&self) -> String {
        self.repo.to_string_lossy().into_owned()
    }

    fn rev(&self, name: &str) -> String {
        git_out(&self.repo, &["rev-parse", name])
    }
}

fn rig(tag: &str) -> Rig {
    let t = TempDir::new(tag);
    let repo = t.path("repo");
    let lane_a = t.path("lane-a");
    let lane_b = t.path("lane-b");
    std::fs::create_dir_all(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    t.write("repo/a.txt", &lines("a", 3));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "first"]);
    git(&repo, &["branch", "merged"]);
    git(&repo, &["checkout", "-q", "-b", "int"]);
    t.write("repo/i1.txt", &lines("p", 2));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "int one"]);
    t.write("repo/i2.txt", &lines("q", 3));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "int two"]);
    t.write("repo/a.txt", &format!("{}more\n", lines("a", 3)));
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "int three"]);
    git(&repo, &["checkout", "-q", "main"]);
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "lane-a",
            lane_a.to_str().unwrap(),
            "int",
        ],
    );
    t.write("lane-a/l1.txt", "x\n");
    git(&lane_a, &["add", "."]);
    git(&lane_a, &["commit", "-q", "-m", "lane one"]);
    t.write("lane-a/l2.txt", "y\ny\n");
    git(&lane_a, &["add", "."]);
    git(&lane_a, &["commit", "-q", "-m", "lane two"]);
    t.write("lane-a/untracked.txt", "u\n");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "--detach",
            lane_b.to_str().unwrap(),
            "int",
        ],
    );
    git(&repo, &["switch", "-q", "--orphan", "orphan"]);
    git(&repo, &["commit", "-q", "--allow-empty", "-m", "alone"]);
    git(&repo, &["checkout", "-q", "main"]);
    git(
        &repo,
        &[
            "config",
            "remote.origin.url",
            "https://me:secret@github.com/o/r.git",
        ],
    );
    git(&repo, &["update-ref", "refs/remotes/origin/far", "int"]);
    std::fs::write(repo.join(".git/FETCH_HEAD"), "").unwrap();
    Rig {
        t,
        repo,
        lane_a,
        lane_b,
    }
}

fn open<'a>(cwd: &str, launcher: &'a dyn Launcher) -> Reader<'a> {
    Reader::open(cwd, launcher).unwrap()
}

fn real(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap()
}

#[test]
fn common_dir_is_the_main_git_dir_from_a_lane() {
    let rig = rig("reader-common");
    let l = launcher();
    let from_lane = open(&rig.lane_a.to_string_lossy(), &*l)
        .common_dir()
        .unwrap();
    let from_main = open(&rig.cwd(), &*l).common_dir().unwrap();
    assert_eq!(from_lane, real(&rig.repo.join(".git")));
    assert_eq!(from_main, from_lane);
}

#[test]
fn common_dir_outside_a_repository_is_not_a_repo() {
    let outside = TempDir::new("reader-norepo");
    let l = launcher();
    let reader = Reader::open(&outside.root().to_string_lossy(), &*l);
    let got = reader.and_then(|reader| reader.common_dir().map(|_| ()));
    assert_eq!(got, Err(Failure::NotRepo));
}

#[test]
fn an_open_reader_can_move_to_another_worktree() {
    let rig = rig("reader-at");
    let l = launcher();
    let main = open(&rig.cwd(), &*l);
    let lane = main.at(&rig.lane_a.to_string_lossy());
    assert_eq!(main.cwd(), rig.cwd());
    assert_eq!(lane.cwd(), rig.lane_a.to_string_lossy());
    assert_eq!(lane.common_dir().unwrap(), main.common_dir().unwrap());
}

#[test]
fn worktrees_list_main_linked_detached() {
    let rig = rig("reader-worktrees");
    let l = launcher();
    let (list, cut) = open(&rig.cwd(), &*l).worktrees().unwrap();
    assert!(!cut);
    assert_eq!(list.len(), 3);
    let by = |path: &Path| {
        list.iter()
            .find(|w| real(Path::new(&w.path)) == real(path))
            .unwrap()
    };
    let main = by(&rig.repo);
    assert_eq!(main.branch.as_deref(), Some("main"));
    assert_eq!(main.head.as_deref(), Some(rig.rev("main").as_str()));
    assert!(!main.detached);
    let a = by(&rig.lane_a);
    assert_eq!(a.branch.as_deref(), Some("lane-a"));
    assert_eq!(a.head.as_deref(), Some(rig.rev("lane-a").as_str()));
    let b = by(&rig.lane_b);
    assert!(b.detached && b.branch.is_none());
    assert_eq!(b.head.as_deref(), Some(rig.rev("int").as_str()));
}

#[test]
fn refs_list_heads_and_remotes() {
    let rig = rig("reader-refs");
    git(
        &rig.repo,
        &["update-ref", "refs/remotes/origin/main", "main"],
    );
    git(
        &rig.repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let l = launcher();
    let (refs, cut) = open(&rig.cwd(), &*l).refs().unwrap();
    assert!(!cut);
    let names: Vec<&str> = refs.iter().map(|r| r.refname.as_str()).collect();
    for want in [
        "refs/heads/main",
        "refs/heads/int",
        "refs/heads/lane-a",
        "refs/heads/merged",
        "refs/heads/orphan",
        "refs/remotes/origin/far",
        "refs/remotes/origin/main",
    ] {
        assert!(names.contains(&want), "{want} missing from {names:?}");
    }
    assert!(!names.contains(&"refs/remotes/origin/HEAD"));
    let int = refs.iter().find(|r| r.refname == "refs/heads/int").unwrap();
    assert_eq!(int.commit, rig.rev("int"));
    assert_eq!(int.subject, "int three");
    assert!(int.committed_ms.is_some());
}

fn rev_list_counts(rig: &Rig, base: &str, head: &str) -> Counts {
    let range = format!("{base}...{head}");
    let text = git_out(&rig.repo, &["rev-list", "--left-right", "--count", &range]);
    parse_left_right(&text).unwrap()
}

#[test]
fn ahead_behind_matches_rev_list() {
    let rig = rig("reader-counts");
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    let all = reader
        .ahead_behind_all("int")
        .unwrap()
        .expect("git has the atom");
    let (refs, _) = reader.refs().unwrap();
    assert!(refs.len() >= 6);
    for row in &refs {
        let got = all.get(&row.refname).copied();
        assert_eq!(
            got,
            Some(rev_list_counts(&rig, "int", &row.refname)),
            "{}",
            row.refname
        );
        assert_eq!(
            reader.left_right("int", &row.commit).unwrap(),
            got,
            "{}",
            row.refname
        );
    }
    assert_eq!(
        all["refs/heads/main"],
        Counts {
            ahead: 0,
            behind: 3
        }
    );
    assert_eq!(
        all["refs/heads/lane-a"],
        Counts {
            ahead: 2,
            behind: 0
        }
    );
}

#[test]
fn left_right_for_an_unknown_rev_is_none() {
    let rig = rig("reader-left-right-unknown");
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    assert_eq!(reader.left_right("nope", &rig.rev("int")), Ok(None));
    assert_eq!(reader.left_right("int", &"1".repeat(40)), Ok(None));
}

#[test]
fn lines_match_git_shortstat() {
    let rig = rig("reader-lines");
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    let int = rig.rev("int");
    let got = reader.lines("main", &int).unwrap().unwrap();
    assert_eq!(
        got,
        Lines {
            added: 6,
            removed: 0,
            files: 3
        }
    );
    let range = format!("main...{int}");
    let text = git_out(&rig.repo, &["diff", "--shortstat", &range]);
    assert_eq!(got, parse_shortstat(&text));
    let none = reader.lines("int", &int).unwrap().unwrap();
    assert_eq!(none, Lines::default());
}

#[test]
fn unrelated_history_has_no_lines() {
    let rig = rig("reader-unrelated-lines");
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    assert_eq!(reader.lines("main", &rig.rev("orphan")), Ok(None));
}

#[test]
fn related_false_for_an_orphan_true_for_a_lane() {
    let rig = rig("reader-related");
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    let main = rig.rev("main");
    assert_eq!(reader.related(&main, &rig.rev("orphan")), Ok(false));
    assert_eq!(reader.related(&main, &rig.rev("lane-a")), Ok(true));
    assert_eq!(reader.related(&main, &main), Ok(true));
}

#[test]
fn related_with_an_unknown_commit_fails() {
    let rig = rig("reader-related-unknown");
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    let got = reader.related(&rig.rev("main"), &"1".repeat(40));
    assert!(matches!(got, Err(Failure::Failed(_))), "{got:?}");
}

#[test]
fn commit_facts_for_a_detached_head() {
    let rig = rig("reader-facts");
    let l = launcher();
    let reader = open(&rig.lane_b.to_string_lossy(), &*l);
    let head = git_out(&rig.lane_b, &["rev-parse", "HEAD"]);
    let (at, subject) = reader.commit_facts(&head).unwrap();
    let seconds: i64 = git_out(&rig.lane_b, &["log", "-1", "--format=%ct", &head])
        .parse()
        .unwrap();
    assert_eq!(at, Some(seconds * 1000));
    assert_eq!(subject, "int three");
}

#[test]
fn commit_facts_make_the_subject_one_plain_line() {
    let rig = rig("reader-facts-plain");
    let long = format!("{}\u{202e}{}", "x".repeat(200), "y".repeat(300));
    git(&rig.repo, &["commit", "-q", "--allow-empty", "-m", &long]);
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    let head = rig.rev("HEAD");
    let (_, subject) = reader.commit_facts(&head).unwrap();
    assert_eq!(subject.chars().count(), 120);
    assert!(!subject.contains('\u{202e}'));
}

#[test]
fn remotes_keep_only_the_host() {
    let rig = rig("reader-remotes");
    git(
        &rig.repo,
        &[
            "config",
            "remote.fork.url",
            "git@gitlab.example.com:me/r.git",
        ],
    );
    let l = launcher();
    let mut got = open(&rig.cwd(), &*l).remotes().unwrap();
    got.sort();
    assert_eq!(
        got,
        vec![("fork".to_owned(), false), ("origin".to_owned(), true)]
    );
    let shown = format!("{got:?}");
    assert!(!shown.contains("secret") && !shown.contains("me") && !shown.contains("ghp"));
}

#[test]
fn a_repository_without_remotes_has_none() {
    let rig = rig("reader-no-remotes");
    git(&rig.repo, &["config", "--remove-section", "remote.origin"]);
    let l = launcher();
    assert_eq!(open(&rig.cwd(), &*l).remotes(), Ok(Vec::new()));
}

#[test]
fn default_rev_is_origin_head_then_main_then_master() {
    let rig = rig("reader-default");
    let l = launcher();
    let reader = open(&rig.cwd(), &*l);
    let local = vec!["int".to_owned(), "main".to_owned()];
    assert_eq!(reader.default_rev(&local).as_deref(), Some("main"));
    assert_eq!(reader.default_rev(&["int".to_owned()]), None);
    assert_eq!(
        reader.default_rev(&["master".to_owned()]).as_deref(),
        Some("master")
    );
    git(&rig.repo, &["update-ref", "refs/remotes/origin/int", "int"]);
    git(
        &rig.repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/int",
        ],
    );
    assert_eq!(reader.default_rev(&local).as_deref(), Some("origin/int"));
}

#[test]
fn fetched_at_from_fetch_head() {
    let rig = rig("reader-fetched");
    let git_dir = rig.repo.join(".git");
    let at = SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    std::fs::OpenOptions::new()
        .write(true)
        .open(git_dir.join("FETCH_HEAD"))
        .unwrap()
        .set_modified(at)
        .unwrap();
    assert_eq!(Reader::fetched_at(&git_dir), Some(1_700_000_000_000));
    std::fs::remove_file(git_dir.join("FETCH_HEAD")).unwrap();
    assert_eq!(Reader::fetched_at(&git_dir), None);
    assert_eq!(Reader::fetched_at(&rig.t.path("missing")), None);
}

fn scripted(fake: &FakeLauncher) {
    fake.push_output(Ok(LaunchOutput::failed(1, "")));
}

#[test]
fn odd_rev_or_commit_refused_before_running() {
    let fake = FakeLauncher::new();
    scripted(&fake);
    let reader = Reader::open("/w", &fake).unwrap();
    let before = fake.commands().len();
    let good = "0123456789abcdef0123456789abcdef01234567";
    let short = &good[..39];
    let upper = good.to_uppercase();
    let refused =
        |got: Result<(), Failure>| assert!(matches!(got, Err(Failure::Refused(_))), "{got:?}");
    for base in ["-c", "a..b", "", "--output=x", "a b", "x:y", "/abs"] {
        refused(reader.ahead_behind_all(base).map(|_| ()));
        refused(reader.left_right(base, good).map(|_| ()));
        refused(reader.lines(base, good).map(|_| ()));
    }
    for head in [
        "-c",
        "a..b",
        short,
        &upper,
        "main",
        "HEAD",
        &format!("{good}0"),
        "",
    ] {
        refused(reader.left_right("main", head).map(|_| ()));
        refused(reader.lines("main", head).map(|_| ()));
        refused(reader.related(good, head).map(|_| ()));
        refused(reader.related(head, good).map(|_| ()));
        refused(reader.commit_facts(head).map(|_| ()));
    }
    assert_eq!(fake.commands().len(), before, "a refused call ran git");
}

#[test]
fn every_command_is_sandboxed_read_only_and_never_fetches() {
    let fake = FakeLauncher::new();
    scripted(&fake);
    let reader = Reader::open("/w", &fake).unwrap();
    let good = "0123456789abcdef0123456789abcdef01234567";
    let _ = reader.common_dir();
    let _ = reader.worktrees();
    let _ = reader.refs();
    let _ = reader.ahead_behind_all("origin/main");
    let _ = reader.left_right("origin/main", good);
    let _ = reader.lines("origin/main", good);
    let _ = reader.related(good, good);
    let _ = reader.commit_facts(good);
    let _ = reader.remotes();
    let _ = reader.default_rev(&["main".to_owned()]);
    let calls: Vec<Vec<String>> = fake.commands().into_iter().map(|c| c.argv).collect();
    assert!(calls.len() >= 10);
    let verbs = [
        "config",
        "rev-parse",
        "worktree",
        "for-each-ref",
        "rev-list",
        "diff",
        "merge-base",
        "log",
        "symbolic-ref",
    ];
    for call in &calls {
        assert_eq!(call[0], "git");
        for must in [
            "--no-optional-locks",
            "core.fsmonitor=false",
            "core.hooksPath=/dev/null",
            "diff.external=",
        ] {
            assert!(call.iter().any(|a| a == must), "{must} missing in {call:?}");
        }
        let at = call.iter().position(|a| a == "-C").unwrap();
        assert_eq!(call[at + 1], "/w");
        let verb = &call[at + 2];
        assert!(verbs.contains(&verb.as_str()), "{verb}");
        if verb == "worktree" {
            assert_eq!(call[at + 3], "list");
        }
        for banned in [
            "fetch",
            "pull",
            "push",
            "remote",
            "ls-remote",
            "gc",
            "add",
            "prune",
        ] {
            assert!(!call[at + 2..].iter().any(|a| a == banned), "{call:?}");
        }
    }
    let diff = calls
        .iter()
        .find(|c| c.iter().any(|a| a == "diff") && c.iter().any(|a| a == "--shortstat"))
        .unwrap();
    for flag in [
        "--no-color",
        "--no-ext-diff",
        "--no-textconv",
        "--ignore-submodules=all",
        "-M",
    ] {
        assert!(diff.iter().any(|a| a == flag), "{flag}");
    }
    assert_eq!(diff.last().map(String::as_str), Some("--"));
    let range = format!("origin/main...{good}");
    let range_at = diff.iter().position(|a| *a == range).unwrap();
    assert_eq!(range_at, diff.len() - 2);
    let counts = calls
        .iter()
        .find(|c| c.iter().any(|a| a == "--left-right"))
        .unwrap();
    assert_eq!(counts.last().map(String::as_str), Some("--"));
}

#[test]
fn a_remote_url_never_reaches_an_error() {
    let fake = FakeLauncher::new();
    scripted(&fake);
    let reader = Reader::open("/w", &fake).unwrap();
    fake.push_output(Ok(LaunchOutput::failed(
        2,
        "error: bad https://me:secret@github.com/o/r.git",
    )));
    let got = reader.remotes();
    let shown = format!("{got:?}");
    assert!(got.is_err());
    assert!(
        !shown.contains("secret") && !shown.contains("github"),
        "{shown}"
    );
}
