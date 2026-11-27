#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SecretFormat {
    Raw,
    KeyValue(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SecretUsage {
    pub name: String,
    pub machine: String,
    pub remote_path: String,
    pub format: SecretFormat,
}
