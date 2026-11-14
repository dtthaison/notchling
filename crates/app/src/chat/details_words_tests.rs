use notchling_locale as t;
use serde_json::json;

use super::branch::{branch_name, BranchModel};
use super::git::{parse_status, tracking, GitFile, GitModel, SHOWN_FILES};
use super::inspector::{counted_title, fact_icon, fact_text, token_line, TaskSummary};
use super::markers;
use notchling_kit::Icon;

fn git_model(files: usize, truncated: bool) -> GitModel {
    GitModel {
        branch: "main".into(),
        tracking: None,
        files: (0..files)
            .map(|n| GitFile {
                path: format!("f{n}"),
                status: "M".into(),
                added: None,
                removed: None,
            })
            .collect(),
        truncated,
    }
}

fn branch_model(file_count: u64, uncommitted: u64) -> BranchModel {
    BranchModel {
        branch: "feature".into(),
        base: "main".into(),
        merge_base: String::new(),
        files: Vec::new(),
        added: 0,
        removed: 0,
        file_count,
        uncommitted_files: uncommitted,
        truncated: false,
    }
}

#[test]
fn section_titles_are_messages_with_counts() {
    assert_eq!(counted_title("Shells", 0, 0), "Shells");
    assert_eq!(counted_title("Shells", 0, 3), "Shells · 3");
    assert_eq!(counted_title("Shells", 2, 3), "Shells · 2 running");
    assert_eq!(
        counted_title(t::chat::details_subagents(), 1, 4),
        t::chat::details_running("Subagents", "1")
    );
    assert_eq!(
        counted_title(t::chat::details_subagents(), 0, 1200),
        "Subagents · 1200"
    );
    assert_eq!(t::chat::details_tasks(), "Tasks");
    assert_eq!(t::chat::details_session(), "Session");
    assert_eq!(t::chat::details_git(), "Git");
    assert_eq!(t::chat::details_tasks_progress(1, 3), "Tasks 1/3");
    assert_eq!(t::chat::details_tasks_counted(1, 3), "Tasks · 1/3");
    let summary = TaskSummary {
        done: 2,
        total: 5,
        current: None,
    };
    assert_eq!(summary.title(), "Tasks 2/5");
}

#[test]
fn the_empty_rows_and_load_more_read_as_they_did() {
    assert_eq!(t::chat::details_no_tasks(), "No tasks");
    assert_eq!(t::chat::details_nothing_read(), "Nothing read yet");
    assert_eq!(t::chat::details_none_started(), "None started");
    assert_eq!(t::chat::details_none_running(), "None running");
    assert_eq!(t::chat::details_load_more("10"), "Load 10 More");
}

#[test]
fn the_token_line_and_facts_read_as_they_did() {
    assert_eq!(token_line(15_400, 900, 0), "15.4k in · 900 out");
    assert_eq!(
        token_line(10_000, 900, 3_000),
        "10k in · 900 out\n3k cached (30%)"
    );
    assert_eq!(fact_text(markers::FACT_TURNS, "1"), "1 turn");
    assert_eq!(fact_text(markers::FACT_TURNS, "2"), "2 turns");
    assert_eq!(fact_text(markers::FACT_TURNS, "1200"), "1200 turns");
    assert_eq!(fact_text(markers::FACT_COMMANDS, "1"), "1 command");
    assert_eq!(fact_text(markers::FACT_COMMANDS, "3"), "3 commands");
    assert_eq!(fact_text(markers::FACT_BRANCH, "main"), "main");
    assert_eq!(fact_icon(markers::FACT_FOLDER), Icon::Folder);
    assert_eq!(fact_icon(markers::FACT_TOKENS), Icon::Tokens);
    assert_eq!(fact_icon("Files"), Icon::File);
}

#[test]
fn a_status_word_is_a_message() {
    assert_eq!(t::chat::status_running(), "Running");
    assert_eq!(t::chat::done(), "Done");
    assert_eq!(t::chat::status_failed(), "Failed");
    assert_eq!(t::chat::status_stopped(), "Stopped");
    assert_eq!(t::chat::status_not_running(), "Not running");
}

#[test]
fn git_words_read_as_they_did() {
    assert_eq!(tracking("origin/main", 0, 0), "origin/main · up to date");
    assert_eq!(tracking("origin/main", 2, 0), "origin/main · 2 ahead");
    assert_eq!(tracking("origin/main", 0, 3), "origin/main · 3 behind");
    assert_eq!(
        tracking("origin/main", 2, 3),
        "origin/main · 2 ahead, 3 behind"
    );
    assert_eq!(git_model(3, false).hidden_text(), None);
    assert_eq!(
        git_model(0, true).hidden_text().as_deref(),
        Some("More files not shown")
    );
    assert_eq!(
        git_model(SHOWN_FILES + 1, false).hidden_text().as_deref(),
        Some("1 more files not shown")
    );
    assert_eq!(
        git_model(SHOWN_FILES + 5, true).hidden_text().as_deref(),
        Some("5+ more files not shown")
    );
    assert_eq!(git_model(2, false).title(), "Git · 2");
    let detached = json!({"repo": true, "detached": true, "head": "abc1234"});
    assert_eq!(branch_name(&detached), "Detached at abc1234");
    assert_eq!(
        branch_name(&json!({"repo": true, "detached": true})),
        "Detached HEAD"
    );
    assert_eq!(branch_name(&json!({"repo": true})), "No branch");
    assert_eq!(branch_name(&json!({"repo": true, "branch": "dev"})), "dev");
    let status = parse_status(&detached).map(|m| m.branch);
    assert_eq!(status.as_deref(), Some("Detached at abc1234"));
    assert_eq!(
        parse_status(&json!({"repo": true}))
            .map(|m| m.branch)
            .as_deref(),
        Some("No branch")
    );
    assert_eq!(t::chat::git_no_uncommitted(), "No uncommitted changes");
    assert_eq!(t::chat::git_no_changes(), "No changes");
    assert_eq!(t::chat::git_vs_base("origin/main"), "vs origin/main");
}

#[test]
fn the_changes_panels_words_read_as_they_did() {
    assert_eq!(branch_model(1, 0).counts_line(), "1 file");
    assert_eq!(branch_model(0, 0).counts_line(), "0 files");
    assert_eq!(branch_model(12, 0).counts_line(), "12 files");
    assert_eq!(
        branch_model(12, 3).counts_line(),
        "12 files · 3 uncommitted"
    );
    assert_eq!(branch_model(1, 1).counts_line(), "1 file · 1 uncommitted");
    assert_eq!(t::chat::changes_title(), "Changes");
    assert_eq!(t::chat::changes_untracked(), "Untracked");
    assert_eq!(
        t::chat::changes_subtitle("docs", "Changes"),
        "docs · Changes"
    );
    assert_eq!(t::chat::changes_large(), "Large diff");
    assert_eq!(t::chat::changes_close(), "Close");
    assert_eq!(t::chat::changes_open_in_panel(), "Open in File Panel");
    assert_eq!(t::chat::changes_filter_placeholder(), "Filter files");
    assert_eq!(t::chat::changes_compare_against(), "Compare Against");
    assert_eq!(t::chat::changes_reading_branches(), "Reading the branches…");
    assert_eq!(
        t::chat::changes_no_branches(),
        "There are no other branches."
    );
    assert_eq!(t::chat::changes_nothing(), "Nothing to compare.");
    assert_eq!(
        t::chat::changes_no_changes_base(),
        "No changes against the base."
    );
    assert_eq!(t::chat::changes_no_match(), "No files match.");
    assert_eq!(t::chat::changes_reading(), "Reading the changes…");
    assert_eq!(
        t::chat::changes_none_to_show(),
        "There are no changes to show."
    );
    assert_eq!(
        t::chat::changes_truncated(),
        "Only the first part of this diff is shown."
    );
    assert_eq!(
        t::chat::changes_unreadable(),
        "The diff came back unreadable."
    );
}
