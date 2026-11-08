//! E16: `notchling`, the engine service and its command line (decision 0004).
//!
//! - `notchling daemon` serves the local API ([`daemon`]): the session list and its changes.
//! - `notchling status`, `notchling sessions [--json]` are clients ([`clients`]).
//! - `notchling service install|uninstall|print [--dir]` writes the per-OS service file
//!   ([`service`]) and prints the command that loads it; it never runs that command.
//!
//! - `notchling hook [--socket <path>]` is Claude Code's hook ([`hook`]): it asks the daemon about a
//!   permission or question and prints the answer; anything else, or any trouble, passes.
//!
//! Milestone 3 adds the acting ops ([`acting_ops`], session control in [`control_ops`], the real
//! terminal and launcher in [`attach`]), switched off by [`acting`] until the maintainer hands
//! sessions over. Later milestones add `secret`, `delegate` and `page`.

pub mod acting;
pub mod acting_ops;
pub mod attach;
pub mod cli;
pub mod clients;
pub mod control_ops;
pub mod daemon;
pub mod hook;
pub mod hook_output;
pub mod layout;
pub mod prompts;
pub mod service;
pub mod signals;

#[cfg(test)]
mod acting_ops_tests;
#[cfg(test)]
mod control_ops_tests;
#[cfg(test)]
mod hook_tests;

use cli::{Command, ServiceAction};
use layout::Layout;
use notchling_platform::paths::{process_env, Os};
use std::path::Path;

/// The log file's name in the data folder, for the service.
pub const LOG_NAME: &str = "notchlingd.log";

/// Runs a parsed command; the exit code.
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
        Command::Status { socket } => match layout_for(socket.as_deref()) {
            Some(l) => clients::status(&l.socket),
            None => 1,
        },
        Command::Sessions { json, socket } => match layout_for(socket.as_deref()) {
            Some(l) => clients::sessions(&l.socket, json),
            None => 1,
        },
        Command::Service { action, dir } => run_service(action, dir.as_deref()),
        // Quiet and 0 whatever happens: a hook's exit 2 would block the tool in Claude Code.
        Command::Hook { socket } => match Layout::from_env(socket.as_deref()) {
            Some(l) => hook::run(&l.socket, &l.claude_dir),
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
            // Not a failure, so a service manager doesn't restart it in a loop.
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
