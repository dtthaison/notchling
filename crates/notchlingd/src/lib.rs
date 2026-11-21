#![allow(clippy::result_large_err)]

mod account_history;
mod accounts_late;
pub mod accounts_ops;
mod accounts_signin;
pub mod acting;
mod acting_events;
pub mod acting_ops;
pub mod activity_ops;
pub mod agy_app;
pub mod agy_elsewhere;
pub mod agy_hook_run;
pub mod agy_link;
pub mod agy_ops;
pub mod agy_phone;
pub mod agy_sidecar_ops;
pub mod agy_sidecar_run;
pub mod agy_sink;
mod ask_cli;
mod ask_cli_args;
mod ask_cli_run;
mod ask_events;
mod ask_find;
mod ask_inbox;
mod ask_main;
pub mod ask_ops;
mod ask_rows;
mod ask_tick;
mod ask_waiters;
pub mod ask_wiring;
pub mod attach;
pub mod awake_loop;
pub mod awake_ops;
pub mod bridge_backend;
pub mod bridge_cluster;
pub mod bridge_machines;
pub mod bridge_ops;
pub mod caller_session;
pub mod capabilities_ops;
pub mod catalog_ops;
pub mod chrome_ops;
pub mod cli;
pub mod clients;
pub mod cluster_blobs;
pub mod cluster_cli;
pub mod cluster_dispatch;
pub mod cluster_dispatch_delegate;
mod cluster_driver;
#[cfg(test)]
mod cluster_driver_order_tests;
#[cfg(test)]
mod cluster_driver_rebuild_tests;
pub mod cluster_failover_setting;
pub mod cluster_fence;
pub mod cluster_fence_feed;
pub mod cluster_fence_keeper;
pub mod cluster_forward;
pub mod cluster_host_facts;
pub mod cluster_inbox;
pub mod cluster_inbox_follow;
#[cfg(test)]
pub(crate) mod cluster_inbox_testkit;
pub mod cluster_invite;
pub mod cluster_join;
pub mod cluster_logins;
pub mod cluster_logins_jobs;
pub mod cluster_logins_notes;
pub mod cluster_logins_probe;
pub mod cluster_logins_run;
pub mod cluster_logins_send;
pub mod cluster_main;
pub mod cluster_mirror;
pub mod cluster_move;
pub mod cluster_move_calls;
pub mod cluster_move_promote;
pub mod cluster_move_run;
pub mod cluster_move_snaps;
pub mod cluster_node;
pub mod cluster_ops;
pub mod cluster_outbox;
pub mod cluster_promote;
mod cluster_promote_acts;
mod cluster_promote_facts;
pub mod cluster_promote_items;
pub mod cluster_promote_watch;
pub mod cluster_prompts;
mod cluster_prompts_book;
mod cluster_prompts_clean;
pub mod cluster_publisher;
pub mod cluster_reach;
pub mod cluster_redact;
pub mod cluster_relay;
pub mod cluster_relay_wiring;
pub mod cluster_returns;
mod cluster_returns_aside;
mod cluster_returns_claim;
mod cluster_returns_items;
mod cluster_returns_step;
mod cluster_returns_touch;
pub mod cluster_returns_waiting;
mod cluster_returns_wire;
pub mod cluster_sessions;
pub mod cluster_switches;
pub mod cluster_tail_mask;
pub mod cluster_versions;
pub mod cluster_wiring;
pub mod cluster_workspace;
pub mod cluster_workspace_send;
pub mod codex_children;
pub mod codex_fence;
pub mod codex_ops;
pub mod commands;
pub mod contract;
pub mod control_ops;
pub mod cost_ops;
pub mod cost_profile_json;
pub mod cost_profile_ops;
pub mod cost_report_cli;
pub mod cost_report_ops;
pub mod cost_report_text;
pub mod cost_setup_cli;
pub mod daemon;
pub mod daemon_wiring;
pub mod delegate_cli;
pub mod every_session;
pub mod git_args;
pub mod git_branch_list;
pub mod git_cache;
pub mod git_ops;
pub mod git_targets;
pub mod git_view;
pub mod git_work_reads;
pub mod git_worktrees;
pub mod grant_broker;
pub mod grant_calls;
pub mod grant_janitor;
pub mod grant_roots;
pub mod grant_use;
pub mod grant_wiring;
pub mod help_cli;
pub mod hook;
pub mod hook_fence;
pub mod hook_guard;
pub mod hook_ops;
pub mod hook_output;
pub mod hook_pages;
pub mod host_id;
mod inbox_events;
pub mod inbox_mirrors;
pub mod inbox_ops;
pub mod install_ops;
pub mod late_emit;
pub mod late_reply;
pub mod layout;
pub mod limits_ops;
pub mod limits_sources;
pub mod limits_usage;
pub mod log_activity;
pub mod log_audit;
mod log_audit_key;
pub mod log_bus;
pub mod log_checks;
pub mod log_fold;
pub mod log_keys;
pub mod log_ops;
pub mod log_queue;
pub mod log_retention;
pub mod log_rotate;
pub mod log_start;
pub mod models_cli;
pub mod models_ops;
pub mod op_group;
pub mod op_kinds;
pub mod organizations_ops;
pub mod outbox_anchor;
pub mod outbox_courier;
pub mod outbox_inbox;
pub mod outbox_ops;
pub mod outbox_postman;
mod outbox_rows;
pub mod outbox_runners;
mod outbox_settle;
pub mod outbox_verify;
pub mod pages_cli;
pub mod pages_ops;
pub mod postmen_wiring;
mod process_facts;
pub mod prompts;
pub mod providers_cli;
pub mod providers_daemon;
mod providers_gate;
pub mod providers_ops;
pub mod quiet_ops;
pub mod reload_cli;
pub mod reload_ops;
mod reply_attempts;
mod reply_outbox;
mod reply_screen;
mod screen_keep;
pub mod service;
pub mod settings_asks;
pub mod settings_cli;
pub mod settings_ops;
pub mod shell_jobs;
pub mod signals;
pub mod snap_copy;
pub mod snap_db;
pub mod snap_files;
pub mod snap_observe;
pub mod snap_ready;
pub mod snap_recv;
pub mod snap_repo;
pub mod snap_restore;
mod snap_restore_fs;
pub mod snap_run;
pub mod snap_send;
pub mod switches;
pub mod tailscale_ops;
mod turn_gate;
pub mod typing_locks;
pub mod vault_args;
pub mod vault_caller;
pub mod vault_cli;
pub mod vault_ops;
pub mod vault_skill_wiring;
pub mod vault_wait;
pub mod versions_check;
pub mod versions_drift;
pub mod versions_ops;
pub mod versions_update;
pub mod wake_loop;

#[cfg(test)]
mod accounts_late_tests;
#[cfg(test)]
mod accounts_ops_log_tests;
#[cfg(test)]
mod accounts_ops_tests;
#[cfg(test)]
mod acting_dialog_tests;
#[cfg(test)]
mod acting_events_tests;
#[cfg(test)]
mod acting_login_tests;
#[cfg(test)]
mod acting_ops_tests;
#[cfg(test)]
mod acting_permission_tests;
#[cfg(test)]
mod acting_shell_output_tests;
#[cfg(test)]
mod activity_ops_tests;
#[cfg(test)]
mod agy_phone_tests;
#[cfg(test)]
mod ask_cluster_tests;
#[cfg(test)]
mod ask_find_tests;
#[cfg(test)]
mod ask_inbox_tests;
#[cfg(test)]
mod ask_load_tests;
#[cfg(test)]
mod ask_main_tests;
#[cfg(test)]
mod ask_marks_tests;
#[cfg(test)]
mod ask_testkit;
#[cfg(test)]
mod awake_follow_tests;
#[cfg(test)]
mod awake_loop_tests;
#[cfg(test)]
mod bridge_models_tests;
#[cfg(test)]
mod caller_session_tests;
#[cfg(test)]
mod caller_trusted_tests;
#[cfg(test)]
mod catalog_ops_tests;
#[cfg(test)]
mod catalog_roles_tests;
#[cfg(test)]
mod chrome_ops_tests;
#[cfg(test)]
mod cluster_fence_keeper_kit;
#[cfg(test)]
mod cluster_fence_keeper_stop_tests;
#[cfg(test)]
mod cluster_fence_keeper_tests;
#[cfg(test)]
mod cluster_fence_stale_tests;
#[cfg(test)]
mod cluster_fence_tests;
#[cfg(test)]
mod cluster_fence_turn_tests;
#[cfg(test)]
mod cluster_fence_write_tests;
#[cfg(test)]
mod cluster_move_fence_fail_tests;
#[cfg(test)]
mod cluster_move_fence_tests;
#[cfg(test)]
mod cluster_move_fix2_tests;
#[cfg(test)]
mod cluster_move_fix_tests;
#[cfg(test)]
mod cluster_move_tests;
#[cfg(test)]
mod cluster_node_tools_tests;
#[cfg(test)]
mod cluster_promote_codex_tests;
#[cfg(test)]
pub(crate) mod cluster_promote_kit;
#[cfg(test)]
mod cluster_promote_ops_tests;
#[cfg(test)]
mod cluster_promote_reach_tests;
#[cfg(test)]
mod cluster_promote_tests;
#[cfg(test)]
mod cluster_promote_watch_tests;
#[cfg(test)]
mod cluster_reach_tests;
#[cfg(test)]
mod cluster_redact_tests;
#[cfg(test)]
mod cluster_relay_testkit;
#[cfg(test)]
mod cluster_returns_claim_tests;
#[cfg(test)]
pub(crate) mod cluster_returns_kit;
#[cfg(test)]
mod cluster_returns_ops_tests;
#[cfg(test)]
mod cluster_returns_step_tests;
#[cfg(test)]
mod cluster_returns_tests;
#[cfg(test)]
mod cluster_returns_touch_tests;
#[cfg(test)]
mod cluster_returns_waiting_tests;
#[cfg(test)]
mod cluster_testkit;
#[cfg(test)]
mod cluster_workspace_testkit;
#[cfg(test)]
mod codex_children_tests;
#[cfg(test)]
mod control_ops_tests;
#[cfg(test)]
mod cost_guard_report_tests;
#[cfg(test)]
mod cost_ops_tests;
#[cfg(test)]
mod cost_profile_ops_tests;
#[cfg(test)]
mod cost_report_tests;
#[cfg(test)]
mod daemon_tests;
#[cfg(test)]
mod event_assert;
#[cfg(test)]
mod every_session_cap_tests;
#[cfg(test)]
mod every_session_tests;
#[cfg(all(test, unix))]
mod git_args_tests;
#[cfg(test)]
mod git_cache_tests;
#[cfg(all(test, unix))]
mod git_ops_allowlist_tests;
#[cfg(all(test, unix))]
mod git_ops_branch_list_tests;
#[cfg(test)]
mod git_ops_branch_tests;
#[cfg(test)]
mod git_ops_listed_tests;
#[cfg(all(test, unix))]
mod git_ops_targets_tests;
#[cfg(test)]
mod git_ops_tests;
#[cfg(all(test, unix))]
mod git_ops_worktrees_tests;
#[cfg(all(test, unix))]
mod git_testkit;
#[cfg(test)]
mod hook_fence_tests;
#[cfg(test)]
mod hook_guard_tests;
#[cfg(test)]
mod hook_ops_tests;
#[cfg(test)]
mod hook_pages_tests;
#[cfg(test)]
mod hook_tests;
#[cfg(test)]
mod inbox_ops_events_tests;
#[cfg(test)]
mod inbox_ops_more_tests;
#[cfg(test)]
mod inbox_ops_tests;
#[cfg(test)]
mod install_events_tests;
#[cfg(test)]
mod install_guard_tests;
#[cfg(test)]
mod install_ops_cluster_tests;
#[cfg(test)]
mod install_ops_fence_tests;
#[cfg(test)]
mod install_ops_tests;
#[cfg(test)]
mod limits_inbox_tests;
#[cfg(test)]
mod limits_ops_tests;
#[cfg(test)]
mod log_activity_tests;
#[cfg(test)]
mod log_audit_keychain_tests;
#[cfg(test)]
pub(crate) mod log_audit_tests;
#[cfg(test)]
mod log_bus_tests;
#[cfg(test)]
pub(crate) mod log_checks_tests;
#[cfg(test)]
mod log_ops_tests;
#[cfg(test)]
mod log_retention_tests;
#[cfg(test)]
mod models_ops_tests;
#[cfg(test)]
mod organizations_ops_tests;
#[cfg(test)]
mod outbox_testkit;
#[cfg(test)]
mod pages_ops_tests;
#[cfg(all(test, any(target_os = "macos", target_os = "linux")))]
mod process_facts_tests;
#[cfg(test)]
mod providers_add_tests;
#[cfg(test)]
mod providers_cli_tests;
#[cfg(test)]
mod providers_ops_tests;
#[cfg(test)]
mod providers_sync_tests;
#[cfg(test)]
mod reload_cli_tests;
#[cfg(test)]
mod reload_ops_tests;
#[cfg(test)]
mod reply_clear_tests;
#[cfg(test)]
mod reply_screen_tests;
#[cfg(test)]
mod reply_screens_kept_tests;
#[cfg(test)]
mod session_of_tests;
#[cfg(all(test, unix))]
mod snap_copy_tests;
#[cfg(all(test, unix))]
mod snap_files_safety_tests;
#[cfg(all(test, unix))]
mod snap_files_stage_tests;
#[cfg(all(test, unix))]
mod snap_files_tests;
#[cfg(all(test, unix))]
mod snap_repo_safety_tests;
#[cfg(all(test, unix))]
mod snap_repo_tests;
#[cfg(all(test, unix))]
mod snap_restore_files_tests;
#[cfg(all(test, unix))]
mod snap_restore_testkit;
#[cfg(all(test, unix))]
mod snap_restore_tests;
#[cfg(all(test, unix))]
mod snap_testkit;
#[cfg(test)]
mod vault_ops_lock_tests;
#[cfg(test)]
mod versions_check_tests;
#[cfg(test)]
mod versions_drift_tests;
#[cfg(test)]
mod versions_inbox_tests;
#[cfg(test)]
mod versions_ops_tests;
#[cfg(test)]
mod versions_soak_tests;
#[cfg(test)]
mod versions_update_tests;
#[cfg(test)]
mod wake_loop_tests;

use cli::{Command, ServiceAction};
use layout::Layout;
use notchling_platform::paths::{process_env, Os};
use std::path::Path;

pub const LOG_NAME: &str = "notchlingd.log";

pub fn run(command: Command) -> i32 {
    match command {
        Command::Version => {
            println!("{}", cli::version_line());
            0
        }
        Command::Help { json } => help_cli::help(json, &mut std::io::stdout()),
        Command::Capabilities { json, socket } => help_cli::capabilities(json, socket.as_deref()),
        Command::Daemon { socket } => run_daemon(socket.as_deref()),
        Command::Status { json, socket } => match client_layout(socket.as_deref(), json) {
            Ok(l) => clients::status(&l.socket, json),
            Err(code) => code,
        },
        Command::Sessions { json, socket } => match client_layout(socket.as_deref(), json) {
            Ok(l) => clients::sessions(&l.socket, json),
            Err(code) => code,
        },
        Command::Pages {
            query,
            json,
            socket,
        } => match client_layout(socket.as_deref(), json) {
            Ok(l) => pages_cli::run(&l, &query, true, json),
            Err(code) => code,
        },
        Command::Page {
            query,
            json,
            socket,
        } => match client_layout(socket.as_deref(), json) {
            Ok(l) => pages_cli::run(&l, &query, false, json),
            Err(code) => code,
        },
        Command::Providers { json } => providers_cli::run(json, &mut std::io::stdout()),
        Command::Delegate { args } => delegate_cli::run(&args),
        Command::DelegateModels { args } => delegate_cli::models(&args),
        Command::Models { args } => models_cli::run(&args),
        Command::Cost { args } => match args.first().map(String::as_str) {
            Some("report") => cost_report_cli::run(&args),
            _ => cost_setup_cli::run(&args),
        },
        Command::Secrets { args } => vault_cli::secrets(&args),
        Command::Secret { args } => vault_cli::secret(&args),
        Command::SecretRequest { args } => vault_cli::secret_request(&args),
        Command::Settings { args } => settings_cli::run(&args),
        Command::Cluster { args } => cluster_cli::run(&args),
        Command::Session { args } => match args.first().map(String::as_str) {
            Some("reload") => reload_cli::run(&args),
            _ => contract::Failure::usage(reload_cli::USAGE).report(contract::wants_json(&args)),
        },
        Command::Ask { args } => ask_cli::run_ask(&args, None),
        Command::Asks { args } => ask_cli::run_asks(&args, None),
        Command::Answer { args } => ask_cli::run_answer(&args, None),
        Command::Service { action, dir } => run_service(action, dir.as_deref()),
        Command::AgySidecar => match Layout::from_env(None) {
            Some(l) => agy_sidecar_run::run(&l),
            None => 0,
        },
        Command::AgyHook { event } => match Layout::from_env(None) {
            Some(l) => agy_hook_run::run(&l, &event),
            None => 0,
        },
        Command::HookFence => match Layout::from_env(None) {
            Some(l) => hook_fence::run(&l),
            None => 0,
        },
        Command::HookGuard => match Layout::from_env(None) {
            Some(l) => hook_guard::run(&l),
            None => 0,
        },
        Command::Hook { socket } => match Layout::from_env(socket.as_deref()) {
            Some(l) => hook::run(&l),
            None => 0,
        },
    }
}

fn layout_for(socket: Option<&Path>) -> Option<Layout> {
    let layout = Layout::from_env(socket);
    if layout.is_none() {
        eprintln!("notchling: no home folder in the environment, so no data folder");
    }
    layout
}

fn client_layout(socket: Option<&Path>, json: bool) -> Result<Layout, i32> {
    Layout::from_env(socket).ok_or_else(|| contract::Failure::not_running().report(json))
}

fn run_daemon(socket: Option<&Path>) -> i32 {
    let Some(layout) = layout_for(socket) else {
        return 1;
    };
    signals::install();
    match daemon::run(
        layout,
        daemon::DaemonOptions::default(),
        &signals::stop_requested,
    ) {
        Ok(()) => 0,
        Err(e @ daemon::DaemonError::AlreadyRunning(_)) => {
            eprintln!("notchling: {e}");
            0
        }
        Err(e) => {
            eprintln!("notchling: {e}");
            1
        }
    }
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: getuid takes nothing and can't fail.
    unsafe { libc::getuid() }
}

#[cfg(not(unix))]
fn current_uid() -> u32 {
    0
}

fn run_service(action: ServiceAction, dir: Option<&Path>) -> i32 {
    let os = Os::current();
    let Some(layout) = layout_for(None) else {
        return 1;
    };
    let binary = match std::env::current_exe().and_then(std::fs::canonicalize) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("notchling: can't find this binary: {e}");
            return 1;
        }
    };
    let spec = service::ServiceSpec {
        binary,
        log: layout.data_dir.join(LOG_NAME),
    };
    if action == ServiceAction::Print {
        print!("{}", service::render(os, &spec));
        return 0;
    }
    let dir = match dir
        .map(Path::to_path_buf)
        .or_else(|| service::default_dir(os, &process_env))
    {
        Some(d) => d,
        None => {
            eprintln!("notchling: no folder for the service file; pass --dir");
            return 1;
        }
    };
    let uid = current_uid();
    match action {
        ServiceAction::Install => match service::install(os, &spec, &dir) {
            Ok(path) => {
                println!("Wrote {}", path.display());
                println!("Load it with: {}", service::load_command(os, &path, uid));
                0
            }
            Err(e) => {
                eprintln!("notchling: {e}");
                1
            }
        },
        ServiceAction::Uninstall => {
            let path = dir.join(service::file_name(os));
            println!(
                "Unload it first with: {}",
                service::unload_command(os, &path, uid)
            );
            match service::uninstall(os, &dir) {
                Ok(Some(path)) => {
                    println!("Removed {}", path.display());
                    0
                }
                Ok(None) => {
                    println!("No service file in {}", dir.display());
                    0
                }
                Err(e) => {
                    eprintln!("notchling: {e}");
                    1
                }
            }
        }
        ServiceAction::Print => 0,
    }
}
