pub mod error;
pub mod host;
pub mod state;
pub mod system;
pub mod wire;

#[cfg(unix)]
pub mod client;
#[cfg(unix)]
mod ops;
#[cfg(unix)]
pub mod pty_handle;
#[cfg(unix)]
pub mod server;

pub use error::HostError;

pub fn version_line() -> String {
    format!(
        "{} {}",
        env!("CARGO_PKG_NAME").trim_start_matches("notchling-"),
        env!("CARGO_PKG_VERSION")
    )
}

pub fn support_dir() -> Option<std::path::PathBuf> {
    let home = system::home()?;
    Some(if cfg!(target_os = "macos") {
        home.join("Library/Application Support/AgentConsole")
    } else {
        home.join(".local/share/AgentConsole")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_line_is_name_then_version() {
        assert_eq!(version_line(), "ptyhost 0.1.0");
    }
}
