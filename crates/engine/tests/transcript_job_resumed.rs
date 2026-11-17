use notchling_engine::transcript::locate::job_transcript;

const LINK: &str = "/h/.claude/projects/-Users-me-app/new-conv.jsonl";
const IN_WORKTREE: &str =
    "/h/.claude/projects/-Users-me-app--claude-worktrees-music/new-conv.jsonl";
const OLD_IN_WORKTREE: &str =
    "/h/.claude/projects/-Users-me-app--claude-worktrees-music/old-conv.jsonl";
const OLD_ELSEWHERE: &str =
    "/h/.claude/projects/-Users-me-app--claude-worktrees-sound/old-conv.jsonl";
const WORKTREE: &str = "/Users/me/app/.claude/worktrees/music";

#[test]
fn a_cleared_job_is_found_by_the_link_stem_in_its_worktree_folder() {
    let found = job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), |p| {
        (p == IN_WORKTREE).then_some(9.0)
    });
    assert_eq!(
        found.as_deref(),
        Some(IN_WORKTREE),
        "the job's state names the new conversation in the project folder of its cwd, but Claude Code wrote the file in the worktree's folder"
    );
}

#[test]
fn the_older_conversation_does_not_win_over_the_new_one() {
    let dates = |p: &str| match p {
        OLD_IN_WORKTREE => Some(5.0),
        IN_WORKTREE => Some(9.0),
        OLD_ELSEWHERE => Some(99.0),
        _ => None,
    };
    assert_eq!(
        job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), dates).as_deref(),
        Some(IN_WORKTREE)
    );
}

#[test]
fn the_recorded_path_stays_when_nothing_else_exists() {
    assert_eq!(
        job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), |_| None).as_deref(),
        Some(LINK)
    );
}

#[test]
fn a_job_with_no_session_id_still_looks_for_the_stem_in_its_worktree() {
    let found = job_transcript(Some(LINK), None, Some(WORKTREE), |p| {
        (p == IN_WORKTREE).then_some(1.0)
    });
    assert_eq!(found.as_deref(), Some(IN_WORKTREE));
}

#[test]
fn the_recorded_path_wins_when_it_exists_and_is_newest() {
    let dates = |p: &str| match p {
        LINK => Some(20.0),
        IN_WORKTREE => Some(10.0),
        _ => None,
    };
    assert_eq!(
        job_transcript(Some(LINK), Some("old-conv"), Some(WORKTREE), dates).as_deref(),
        Some(LINK)
    );
}
