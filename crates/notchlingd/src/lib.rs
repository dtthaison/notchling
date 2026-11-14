mod accounts_late;
pub mod accounts_ops;
mod accounts_signin;
pub mod acting;
mod acting_events;
pub mod acting_ops;
pub mod activity_ops;
pub mod agy_elsewhere;
pub mod agy_ops;
pub mod agy_sink;
pub mod attach;
pub mod bridge_backend;
pub mod bridge_ops;
pub mod catalog_ops;
pub mod cli;
pub mod clients;
pub mod codex_ops;
pub mod control_ops;
pub mod cost_ops;
pub mod daemon;
pub mod daemon_wiring;
pub mod delegate_cli;
pub mod git_ops;
pub mod held_ops;
pub mod hook;
pub mod hook_ops;
pub mod hook_output;
pub mod hook_pages;
pub mod host_id;
mod inbox_events;
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
pub mod log_bus;
pub mod log_checks;
pub mod log_fold;
pub mod log_ops;
pub mod log_queue;
pub mod log_retention;
pub mod log_rotate;
pub mod log_start;
pub mod op_group;
pub mod op_kinds;
pub mod pages_cli;
pub mod pages_ops;
pub mod prompts;
pub mod providers_daemon;
mod providers_gate;
pub mod providers_ops;
pub mod quiet_ops;
pub mod service;
pub mod settings_asks;
pub mod settings_cli;
pub mod settings_ops;
pub mod signals;
pub mod switches;
pub mod vault_args;
pub mod vault_caller;
pub mod vault_cli;
pub mod vault_ops;
pub mod vault_wait;

#[cfg(test)]
mod accounts_late_tests;
#[cfg(test)]
mod accounts_ops_log_tests;
#[cfg(test)]
mod accounts_ops_tests;
#[cfg(test)]
mod acting_events_tests;
#[cfg(test)]
mod acting_ops_tests;
#[cfg(test)]
mod acting_permission_tests;
#[cfg(test)]
mod activity_ops_tests;
#[cfg(test)]
mod bridge_models_tests;
#[cfg(test)]
mod catalog_ops_tests;
#[cfg(test)]
mod control_ops_tests;
#[cfg(test)]
mod cost_ops_tests;
#[cfg(test)]
mod daemon_tests;
#[cfg(test)]
mod event_assert;
#[cfg(test)]
mod git_ops_branch_tests;
#[cfg(test)]
mod git_ops_tests;
#[cfg(test)]
mod held_ops_tests;
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
mod install_ops_tests;
#[cfg(test)]
mod limits_inbox_tests;
#[cfg(test)]
mod limits_ops_tests;
#[cfg(test)]
mod log_activity_tests;
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
mod pages_ops_tests;
#[cfg(test)]
mod providers_add_tests;
#[cfg(test)]
mod providers_ops_tests;
#[cfg(test)]
mod providers_sync_tests;
#[cfg(test)]
mod session_of_tests;
#[cfg(test)]
mod vault_ops_lock_tests;

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
        Command::Help => {
            println!("{}", cli::usage());
            0
        }
        Command::Daemon { socket } => run_daemon(socket.as_deref()),
        Command::Status { json, socket } => match layout_for(socket.as_deref()) {
            Some(l) => clients::status(&l.socket, json),
            None => 1,
        },
        Command::Sessions { json, socket } => match layout_for(socket.as_deref()) {
            Some(l) => clients::sessions(&l.socket, json),
            None => 1,
        },
        Command::Pages {
            query,
            json,
            socket,
        } => match layout_for(socket.as_deref()) {
            Some(l) => pages_cli::run(&l, &query, true, json),
            None => 1,
        },
        Command::Page {
            query,
            json,
            socket,
        } => match layout_for(socket.as_deref()) {
            Some(l) => pages_cli::run(&l, &query, false, json),
            None => 1,
        },
        Command::Delegate { args } => delegate_cli::run(&args),
        Command::DelegateModels { args } => delegate_cli::models(&args),
        Command::Secrets { args } => vault_cli::secrets(&args),
        Command::Secret { args } => vault_cli::secret(&args),
        Command::SecretRequest { args } => vault_cli::secret_request(&args),
        Command::Settings { args } => settings_cli::run(&args),
        Command::Service { action, dir } => run_service(action, dir.as_deref()),
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
