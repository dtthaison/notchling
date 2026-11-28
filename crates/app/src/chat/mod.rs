pub mod activity;
pub mod ask_drafts;
pub mod ask_preview;
pub mod asks;
pub mod bench_fixture;
pub mod blocks;
#[cfg(test)]
mod blocks_tests;
pub mod branch;
pub mod branch_list;
pub mod branch_rows;
pub mod branches_preview;
pub mod branches_state;
pub mod browser_rows;
pub mod busy;
pub mod changes_target;
#[cfg(test)]
mod children_tests;
pub mod chrome;
#[cfg(test)]
mod chrome_tests;
pub mod claude_picker;
pub mod cleared;
pub mod closed;
pub mod code_tokens;
#[cfg(test)]
mod code_tokens_tests;
pub mod codex_children;
pub mod codex_list;
pub mod composer;
#[cfg(test)]
mod conversation_words_tests;
pub mod data;
pub mod delegated;
#[cfg(test)]
mod delegated_cap_tests;
#[cfg(test)]
mod details_words_tests;
pub mod dialog_card;
pub mod drafts;
pub mod drops;
pub mod escape;
pub mod file_panel;
#[cfg(test)]
mod file_panel_view_tests;
pub mod follow;
pub mod format;
#[cfg(test)]
mod format_words_tests;
pub mod git;
pub mod git_gates;
pub mod header;
pub mod history;
pub mod html_preview;
#[cfg(test)]
mod html_preview_tests;
pub mod image_size;
pub mod inspector;
pub mod job_fan;
pub mod json_view;
#[cfg(test)]
mod json_view_tests;
pub mod jsonl_view;
#[cfg(test)]
mod jsonl_view_tests;
pub mod keep_finished;
pub mod lane_limits;
pub mod limit_ring;
pub mod link_target;
pub mod loader;
#[cfg(test)]
mod loader_app_tests;
#[cfg(test)]
mod loader_change_tests;
pub mod log;
#[cfg(test)]
mod log_tests;
pub mod markdown;
pub mod markdown_cache;
pub mod markers;
pub mod message_meta;
pub mod outbox;
#[cfg(test)]
mod outbox_asked_tests;
#[cfg(test)]
mod outbox_box_same_tests;
#[cfg(test)]
mod outbox_box_wrap_tests;
#[cfg(test)]
mod outbox_confirmed_tests;
#[cfg(test)]
mod outbox_hearts_tests;
#[cfg(test)]
mod outbox_invisible_tests;
#[cfg(test)]
mod outbox_picture_tests;
#[cfg(test)]
mod outbox_unsure_tests;
pub mod outgoing;
pub mod page_panel;
pub mod paging;
pub mod panels;
#[cfg(test)]
mod parse_cost_tests;
pub mod paths;
pub mod pending_ask;
pub mod pending_child;
pub mod permission_text;
pub mod pickers;
#[cfg(test)]
mod prompt_busy_tests;
pub mod prompt_flow;
pub mod prompts;
#[cfg(test)]
mod prompts_permission_tests;
#[cfg(test)]
mod question_parity_tests;
pub mod reload;
pub mod reload_feed;
pub mod remote;
pub mod remote_control;
#[cfg(test)]
mod remote_control_tests;
pub mod remote_follow;
#[cfg(test)]
mod remote_follow_tests;
#[cfg(test)]
mod remote_prompt_tests;
#[cfg(test)]
mod remote_tests;
pub mod renders;
#[cfg(test)]
mod reply_words_tests;
pub mod rows;
pub mod run_command;
#[cfg(test)]
mod run_command_tests;
pub mod scroll_trace;
pub mod selection;
pub mod session_reload;
#[cfg(test)]
mod session_reload_tests;
pub mod shell_list;
pub mod shell_live;
#[cfg(test)]
mod shell_live_tests;
pub mod shell_rows;
pub mod sidebar;
pub mod sidebar_ask;
pub mod sidebar_completed;
pub mod sidebar_node;
#[cfg(test)]
mod sidebar_node_tests;
#[cfg(test)]
mod sidebar_words_tests;
pub mod simulator;
pub mod slash_menu;
pub mod steps;
pub mod subagent_rows;
pub mod subagent_view;
pub mod tasks_bar;
pub mod terminal;
pub mod terminal_paint;
pub mod terminal_sample;
#[cfg(test)]
mod tree_tests;
pub mod used;
pub mod vault_ask;
#[cfg(target_os = "macos")]
pub mod view;
#[cfg(test)]
mod window_words_tests;
pub mod worktrees;
