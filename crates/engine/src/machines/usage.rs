//! Where a vault secret is delivered (Swift `SecretUsage`, `SecretFormat`): a file on one machine.

/// How a secret sits in its file on the machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecretFormat {
    /// The value alone, no newline (`github-token`).
    Raw,
    /// `KEY=<value>\n` with this key (`claude.env`).
    KeyValue(String),
}

/// One place a vault secret is delivered to: a file on one machine.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretUsage {
    pub name: String,
    pub machine: String,
    pub remote_path: String,
    pub format: SecretFormat,
}
