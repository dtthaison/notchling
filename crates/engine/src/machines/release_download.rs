use super::dist_manifest::{self, DistFile, DistManifest};
use super::runner::{Call, Runner, NOT_FOUND};
use super::steps_node_binary::hash_file;
use crate::cluster::build_id;
use std::path::Path;
use std::sync::Arc;

pub const RELEASE_BASE: &str = "https://github.com/dtthaison/notchling/releases/download/";
pub const MAX_MANIFEST: u64 = 64 * 1024;
pub const MAX_BINARY: u64 = 256 * 1024 * 1024;
const FETCH_SECS: u64 = 630;
const CURL_SECS: &str = "600";

pub trait ReleaseDownloads: Send + Sync {
    fn fetch(&self, url: &str, dest: &Path, max_bytes: u64) -> Result<(), String>;
}

pub struct NoDownloads;

impl ReleaseDownloads for NoDownloads {
    fn fetch(&self, _url: &str, _dest: &Path, _max_bytes: u64) -> Result<(), String> {
        Err("downloads aren't available here".into())
    }
}

pub struct CurlDownloads {
    runner: Arc<dyn Runner>,
}

impl CurlDownloads {
    pub fn new(runner: Arc<dyn Runner>) -> CurlDownloads {
        CurlDownloads { runner }
    }

    pub fn argv(url: &str, dest: &Path, max_bytes: u64) -> Vec<String> {
        [
            "/usr/bin/curl",
            "-q",
            "--silent",
            "--show-error",
            "--fail",
            "--location",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--max-time",
            CURL_SECS,
            "--max-filesize",
            &max_bytes.to_string(),
            "-o",
            &dest.to_string_lossy(),
            url,
        ]
        .map(str::to_string)
        .to_vec()
    }
}

impl ReleaseDownloads for CurlDownloads {
    fn fetch(&self, url: &str, dest: &Path, max_bytes: u64) -> Result<(), String> {
        if !url.starts_with("https://") {
            return Err("the address isn't https".into());
        }
        let done = self
            .runner
            .run(&Call::secs(Self::argv(url, dest, max_bytes), FETCH_SECS));
        match done.code {
            0 => Ok(()),
            NOT_FOUND => Err("curl wasn't found".into()),
            code => {
                let said = done.err.lines().find(|l| !l.trim().is_empty());
                Err(said.map_or_else(|| format!("curl exit {code}"), |l| l.trim().to_string()))
            }
        }
    }
}

pub fn release_version<'a>(version: &'a str, build: Option<&str>) -> Option<&'a str> {
    build.filter(|b| !b.ends_with("-dirty") && build_id::accepted(b).is_some())?;
    let parts: Vec<&str> = version.split('.').collect();
    let plain = parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()));
    plain.then_some(version)
}

pub fn this_release() -> Option<&'static str> {
    release_version(env!("CARGO_PKG_VERSION"), build_id::build_id())
}

pub fn release_url(version: &str, file: &str) -> String {
    format!("{RELEASE_BASE}v{version}/{file}")
}

struct Temp<'a>(&'a Path);

impl Drop for Temp<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

fn fetch_small(
    downloads: &dyn ReleaseDownloads,
    url: &str,
    dist: &Path,
) -> Result<Vec<u8>, String> {
    let temp = dist.join(".manifest.json.fetch");
    let _guard = Temp(&temp);
    downloads.fetch(url, &temp, MAX_MANIFEST)?;
    let meta = std::fs::metadata(&temp).map_err(|e| e.to_string())?;
    if meta.len() > MAX_MANIFEST {
        return Err("the release manifest is too large".into());
    }
    std::fs::read(&temp).map_err(|e| e.to_string())
}

fn released_entry(
    downloads: &dyn ReleaseDownloads,
    version: &str,
    build: &str,
    file: &str,
    dist: &Path,
) -> Result<DistManifest, String> {
    let bytes = fetch_small(downloads, &release_url(version, "manifest.json"), dist)?;
    let manifest = dist_manifest::parse(&bytes).ok_or("the release manifest isn't readable")?;
    if manifest.build != build {
        return Err(format!(
            "the {version} release was built from another commit than this Mac"
        ));
    }
    if !manifest.files.contains_key(file) {
        return Err(format!("the {version} release has no {file}"));
    }
    Ok(manifest)
}

fn place(
    downloads: &dyn ReleaseDownloads,
    version: &str,
    file: &str,
    entry: &DistFile,
    dist: &Path,
) -> Result<(), String> {
    if entry.size > MAX_BINARY {
        return Err(format!("{file} is larger than the limit"));
    }
    let temp = dist.join(format!(".{file}.download"));
    let _guard = Temp(&temp);
    downloads.fetch(&release_url(version, file), &temp, MAX_BINARY)?;
    let size = std::fs::metadata(&temp).map_err(|e| e.to_string())?.len();
    if size != entry.size {
        return Err(format!("{file} has the wrong size"));
    }
    if hash_file(&temp).as_deref() != Some(entry.sha256.as_str()) {
        return Err(format!("{file} doesn't match its SHA-256"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&temp, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| e.to_string())?;
    }
    std::fs::rename(&temp, dist.join(file)).map_err(|e| e.to_string())
}

pub fn download_linux(
    downloads: &dyn ReleaseDownloads,
    dist: &Path,
    file: &str,
    version: &str,
    build: &str,
) -> Result<(), String> {
    std::fs::create_dir_all(dist).map_err(|e| e.to_string())?;
    let released = released_entry(downloads, version, build, file, dist)?;
    let entry = released.files[file].clone();
    place(downloads, version, file, &entry, dist)?;
    let mut merged = dist_manifest::read(dist)
        .filter(|own| own.build == build)
        .unwrap_or(released);
    merged.files.insert(file.to_string(), entry);
    dist_manifest::write(dist, &merged)
}
