use notchling_local_api::{Client, ClientError, ClientKind, ConnectOptions};
use serde_json::Value;
use std::path::Path;
use std::time::Duration;

pub trait Link {
    fn call(&mut self, op: &str, args: Value, within: Duration) -> Result<Value, LinkError>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LinkError {
    Gone,
    Refused(String),
}

pub struct DaemonLink {
    client: Client,
}

impl DaemonLink {
    pub fn connect(socket: &Path, within: Duration) -> Result<DaemonLink, LinkError> {
        let options = ConnectOptions {
            timeout: within,
            check_server: true,
        };
        Client::connect_with(
            socket,
            ClientKind::Hook,
            env!("CARGO_PKG_VERSION"),
            &options,
        )
        .map(|client| DaemonLink { client })
        .map_err(|_| LinkError::Gone)
    }
}

impl Link for DaemonLink {
    fn call(&mut self, op: &str, args: Value, within: Duration) -> Result<Value, LinkError> {
        self.client.set_timeout(within);
        self.client.call(op, args).map_err(|e| match e {
            ClientError::Api(api) => LinkError::Refused(api.message),
            _ => LinkError::Gone,
        })
    }
}
