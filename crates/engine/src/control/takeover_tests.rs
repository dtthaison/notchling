//! Ports of `TakeoverTests` (eligibility, spawn, pid to session) and the takeover cases of
//! `HostReplyTests`.

use super::*;
use crate::sessions::agent::sample;
use crate::sessions::testutil::TempDir;

const ID: &str = "7f1c2d3e-4a5b-4c6d-8e9f-0a1b2c3d4e5f";

struct Fixture {
    tmp: TempDir,
    roots: Roots,
    cwd: String,
}

fn fixture(transcript: bool, make_cwd: bool) -> Fixture {
    let tmp = TempDir::new("takeover");
    std::fs::create_dir_all(tmp.path(".claude/sessions")).expect("mkdir");
    if make_cwd {
        std::fs::create_dir_all(tmp.path("work")).expect("mkdir");
    }
    if transcript {
        tmp.write(&format!(".claude/projects/-tmp-work/{ID}.jsonl"), "{}\n");
    }
    let roots = Roots::new(tmp.root(), &tmp.path("data"));
    let cwd = tmp.path("work").to_string_lossy().into_owned();
    Fixture { tmp, roots, cwd }
}

fn session(f: &Fixture) -> TakeoverSession {
    TakeoverSession {
        id: ID.into(),
        pid: Some(4242),
        cwd: f.cwd.clone(),
        sdk: false,
        control: Control::ViewOnly,
        running: false,
    }
}

fn dead(_: u32) -> bool {
    false
}

fn probes(is_alive: &dyn Fn(u32) -> bool) -> TakeoverProbes<'_> {
    TakeoverProbes {
        is_alive,
        dir_exists: &real_dir_exists,
    }
}

fn error(result: Result<TakeoverPlan, TakeoverError>) -> Option<TakeoverError> {
    result.err()
}

fn eval(f: &Fixture, s: &TakeoverSession) -> Result<TakeoverPlan, TakeoverError> {
    evaluate(s, &f.roots, false, &probes(&dead))
}

// Port of TakeoverTests.testAnExitedSessionWithATranscriptAndFolderCanBeTakenOver.
#[test]
fn an_exited_session_with_a_transcript_and_folder_can_be_taken_over() {
    let f = fixture(true, true);
    let plan = eval(&f, &session(&f)).expect("a plan");
    assert_eq!(plan.arguments, ["--resume", ID]);
    assert_eq!(plan.cwd, f.cwd);
    assert!(plan
        .transcript
        .to_string_lossy()
        .ends_with(&format!("{ID}.jsonl")));
    let state = state(&session(&f), &f.roots, &probes(&dead), &|_| false);
    assert_eq!(state, Some(TakeoverState::Available));
}

// Port of TakeoverTests.testARunningOriginalIsBusyNoMatterWhat.
#[test]
fn a_running_original_is_busy_no_matter_what() {
    let f = fixture(true, true);
    let alive = |pid: u32| pid == 4242;
    assert_eq!(
        error(evaluate(&session(&f), &f.roots, false, &probes(&alive))),
        Some(TakeoverError::Alive)
    );
    let mut running = session(&f);
    running.pid = None;
    running.running = true;
    assert_eq!(error(eval(&f, &running)), Some(TakeoverError::Alive));
    let everything_alive = |_: u32| true;
    assert_eq!(
        state(&session(&f), &f.roots, &probes(&everything_alive), &|_| {
            false
        }),
        Some(TakeoverState::Unavailable(ALIVE_REASON.to_string()))
    );
}

#[test]
fn a_verified_original_is_stoppable_and_busy_says_so() {
    let f = fixture(true, true);
    let everything_alive = |_: u32| true;
    assert_eq!(
        state(&session(&f), &f.roots, &probes(&everything_alive), &|_| {
            true
        }),
        Some(TakeoverState::Stoppable { busy: false })
    );
    let mut running = session(&f);
    running.running = true;
    assert_eq!(
        state(&running, &f.roots, &probes(&everything_alive), &|_| true),
        Some(TakeoverState::Stoppable { busy: true })
    );
    // Stoppable only when everything else needed to resume is there.
    let missing = fixture(false, true);
    assert_eq!(
        state(
            &session(&missing),
            &missing.roots,
            &probes(&everything_alive),
            &|_| true
        ),
        Some(TakeoverState::Unavailable(ALIVE_REASON.to_string()))
    );
}

// Port of TakeoverTests.testAnotherLiveProcessWritingTheSameSessionIdIsBusy.
#[test]
fn another_live_process_writing_the_same_session_id_is_busy() {
    let f = fixture(true, true);
    f.tmp.write(
        ".claude/sessions/999.json",
        &format!(r#"{{"pid":999,"sessionId":"{ID}","cwd":"/x"}}"#),
    );
    let mut s = session(&f);
    s.pid = None;
    let alive = |pid: u32| pid == 999;
    assert_eq!(
        error(evaluate(&s, &f.roots, false, &probes(&alive))),
        Some(TakeoverError::Alive)
    );
    assert_eq!(
        error(eval(&f, &s)),
        None,
        "a stale file of a dead process does not block"
    );
}

// Port of TakeoverTests.testNoResumableTranscriptMeansNoTakeover.
#[test]
fn no_resumable_transcript_means_no_takeover() {
    let f = fixture(false, true);
    assert_eq!(
        error(eval(&f, &session(&f))),
        Some(TakeoverError::NoTranscript)
    );
    // Desktop sessions keep their transcript elsewhere; the CLI cannot resume those.
    f.tmp
        .write(&format!("local-agent-mode-sessions/x/{ID}.jsonl"), "{}");
    assert_eq!(
        error(eval(&f, &session(&f))),
        Some(TakeoverError::NoTranscript)
    );
}

// Port of TakeoverTests.testBadIdsNeverReachThePath.
#[test]
fn bad_ids_never_reach_the_path() {
    let f = fixture(true, true);
    for bad in ["../../etc/passwd", "short", "", &format!("{ID}/../x")] {
        let mut s = session(&f);
        s.id = bad.to_string();
        assert_eq!(
            error(eval(&f, &s)),
            Some(TakeoverError::NoTranscript),
            "{bad}"
        );
    }
}

// Port of TakeoverTests.testSdkMissingFolderAndAlreadyControllableAreRefused.
#[test]
fn sdk_missing_folder_and_already_controllable_are_refused() {
    let f = fixture(true, true);
    let mut sdk = session(&f);
    sdk.sdk = true;
    assert_eq!(error(eval(&f, &sdk)), Some(TakeoverError::Sdk));
    for cwd in ["/definitely/not/here", ""] {
        let mut s = session(&f);
        s.cwd = cwd.into();
        assert_eq!(
            error(eval(&f, &s)),
            Some(TakeoverError::NoWorkingFolder),
            "{cwd:?}"
        );
    }
    let mut mine = session(&f);
    mine.control = Control::ConsolePty;
    assert_eq!(
        error(eval(&f, &mine)),
        Some(TakeoverError::AlreadyControllable)
    );
    assert_eq!(
        state(&mine, &f.roots, &probes(&dead), &|_| false),
        None,
        "only view-only sessions carry a takeover state"
    );
}

// Port of TakeoverTests.testSpawnRequestMatchesANewSessionWithResumeAndAScrubbedEnvironment.
#[test]
fn spawn_request_has_resume_and_a_scrubbed_environment() {
    let f = fixture(true, true);
    let plan = eval(&f, &session(&f)).expect("a plan");
    let env: BTreeMap<String, String> = [
        ("PATH", "/bin"),
        ("CLAUDE_CODE_ENTRYPOINT", "cli"),
        ("CLAUDECODE", "1"),
        ("HOME", "/h"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let request = spawn(&plan, "/x/claude", &env);
    assert_eq!(request.executable, "/x/claude");
    assert_eq!(request.arguments, ["--resume", ID]);
    assert_eq!(request.cwd, f.cwd);
    assert!(!request.environment.contains_key("CLAUDE_CODE_ENTRYPOINT"));
    assert!(!request.environment.contains_key("CLAUDECODE"));
    assert_eq!(request.environment["PATH"], "/bin");
    assert_eq!(request.environment["TERM"], "xterm-256color");
    assert_eq!(request.environment, scrubbed(&env));
}

// Port of TakeoverTests.testTheResumedProcessMapsBackToItsSessionWhetherTheIdStaysOrChanges.
#[test]
fn the_resumed_process_maps_back_to_its_session() {
    let f = fixture(true, true);
    let dir = f.tmp.path(".claude/sessions");
    let parents = |pid: u32| match pid {
        501 => Some(500),
        500 => Some(300),
        _ => None,
    };
    f.tmp.write(
        ".claude/sessions/501.json",
        &format!(r#"{{"pid":501,"sessionId":"{ID}"}}"#),
    );
    let same = session_id_for_root_pid(300, &dir, &parents).expect("mapped");
    assert_eq!(same, ID);
    assert_eq!(resolution(ID, &same), Resolution::SameSession);

    let changed_id = "11111111-2222-4333-8444-555555555555";
    f.tmp.write(
        ".claude/sessions/501.json",
        &format!(r#"{{"pid":501,"sessionId":"{changed_id}"}}"#),
    );
    let changed = session_id_for_root_pid(300, &dir, &parents).expect("mapped");
    assert_eq!(
        resolution(ID, &changed),
        Resolution::Replaced {
            old: ID.into(),
            new: changed_id.into()
        }
    );
    assert_eq!(session_id_for_root_pid(777, &dir, &parents), None);
}

// Port of HostReplyTests.testTakeoverIsOfferedOnlyForTerminalsTheHostDoesNotOwnAndEndedSessions.
#[test]
fn takeover_is_offered_only_for_terminals_the_host_does_not_own_and_ended_sessions() {
    let terminal = |pid| sample(AgentTarget::Process(pid), None);
    assert!(offers_takeover(&terminal(43), false));
    assert!(!offers_takeover(&terminal(42), true));
    let mut headless = terminal(44);
    headless.host = "claude -p".into();
    assert!(!offers_takeover(&headless, false));
    assert!(!offers_takeover(
        &sample(AgentTarget::Job("abc".into()), None),
        false
    ));
    let ended = sample(
        AgentTarget::Resume {
            session_id: "e".into(),
            cwd: "/tmp".into(),
        },
        None,
    );
    assert!(offers_takeover(&ended, false));
}

// Port of HostReplyTests.testTakeoverPromptsFollowTheState.
#[test]
fn takeover_prompts_follow_the_state() {
    let available = prompt_for(Some(&TakeoverState::Available), "x").expect("prompt");
    assert_eq!(available.confirm, "Take Over");
    assert!(!available.destructive);
    let stoppable =
        prompt_for(Some(&TakeoverState::Stoppable { busy: false }), "x").expect("prompt");
    assert_eq!(stoppable.confirm, "Stop Original and Take Over");
    assert!(stoppable.destructive);
    let blocked =
        prompt_for(Some(&TakeoverState::Unavailable("lý do".into())), "x").expect("prompt");
    assert_eq!(blocked.message, "lý do");
    assert_eq!(blocked.confirm, "OK");
    assert_eq!(prompt_for(None, "x"), None);
    assert!(busy_prompt().destructive);
}
