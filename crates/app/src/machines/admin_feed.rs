use std::path::PathBuf;
use std::time::Duration;

use notchling_engine::accounts::Secret;
use notchling_engine::machines::admin_secret::AdminSecrets;
use notchling_local_api::{ops, Client, ClientError, ClientKind, ConnectOptions};
use serde_json::{json, Value};

use super::cluster_feed::NOT_RUNNING;

pub const APPROVAL_WAIT: Duration = Duration::from_secs(330);

pub struct DaemonAdminSecrets {
    socket: PathBuf,
}

impl DaemonAdminSecrets {
    pub fn new(socket: PathBuf) -> DaemonAdminSecrets {
        DaemonAdminSecrets { socket }
    }
}

pub fn secret_in(reply: &Value) -> Option<Secret> {
    reply
        .get("secret")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(Secret::new)
}

pub fn failure_text(error: &ClientError) -> String {
    match error {
        ClientError::Api(api) => api.message.clone(),
        ClientError::Timeout => "the Vault didn't answer in time".to_owned(),
        ClientError::Protocol(_) => "the daemon sent something unexpected".to_owned(),
        _ => NOT_RUNNING.to_owned(),
    }
}

impl AdminSecrets for DaemonAdminSecrets {
    fn read(&self, entry: &str, machine: &str) -> Result<Secret, String> {
        let options = ConnectOptions {
            timeout: APPROVAL_WAIT,
            ..ConnectOptions::default()
        };
        let mut client = Client::connect_with(
            &self.socket,
            ClientKind::App,
            env!("CARGO_PKG_VERSION"),
            &options,
        )
        .map_err(|e| failure_text(&e))?;
        let reply = client
            .call(ops::VAULT_ADMIN, json!({"name": entry, "machine": machine}))
            .map_err(|e| failure_text(&e))?;
        secret_in(&reply).ok_or_else(|| format!("{entry} has no password saved"))
    }
}

#[cfg(test)]
#[path = "admin_feed_tests.rs"]
mod tests;
