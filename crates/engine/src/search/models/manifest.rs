use serde::Deserialize;
use std::sync::OnceLock;

#[derive(Debug, Deserialize)]
pub struct Manifest {
    pub hosts: Vec<String>,
    pub models: Vec<ModelEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ModelEntry {
    pub id: String,
    pub folder: String,
    pub tier: String,
    pub listed: bool,
    pub recommended: bool,
    pub message: String,
    pub download_bytes: u64,
    pub kept_bytes: u64,
    pub files: Vec<ModelFile>,
}

#[derive(Debug, Deserialize)]
pub struct ModelFile {
    pub repo: String,
    pub revision: String,
    pub path: String,
    pub save_as: String,
    pub size: u64,
    pub sha256: String,
    pub license: String,
}

impl ModelFile {
    pub fn url(&self) -> String {
        format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            self.repo, self.revision, self.path
        )
    }
}

impl Manifest {
    pub fn allows(&self, host: &str) -> bool {
        let host = host.to_ascii_lowercase();
        self.hosts.iter().any(|h| host_allowed(h, &host))
    }

    pub fn model(&self, id: &str) -> Option<&ModelEntry> {
        self.models.iter().find(|m| m.id == id && m.listed)
    }
}

pub fn manifest() -> &'static Manifest {
    static PARSED: OnceLock<Manifest> = OnceLock::new();
    PARSED.get_or_init(|| {
        serde_json::from_str(include_str!("../../../data/models.json")).unwrap_or(Manifest {
            hosts: Vec::new(),
            models: Vec::new(),
        })
    })
}

pub fn host_allowed(pattern: &str, host: &str) -> bool {
    match pattern.strip_prefix("*.") {
        Some(rest) => host.len() > rest.len() + 1 && host.ends_with(&format!(".{rest}")),
        None => pattern == host,
    }
}
