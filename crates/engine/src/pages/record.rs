use super::content::{
    artifact_url_in, looks_failed, make_folder, page_title, project_name, published_version,
    wrapped,
};
use super::library::{safe_relative, PageLibrary, PagesError};
use crate::search::PageEntry;
use serde_json::{Map, Value};
use std::io::Read;
use std::path::{Component, Path, PathBuf};

pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

#[derive(Clone, Copy, Debug)]
pub struct RecordContext<'a> {
    pub account: &'a str,
    pub now: f64,
    pub home: &'a Path,
}

pub fn flatten(value: Option<&Value>) -> String {
    match value {
        None | Some(Value::Null) => String::new(),
        Some(Value::String(s)) => s.clone(),
        Some(v) => v.to_string(),
    }
}

pub fn join_normalized(base: &Path, rel: &str) -> PathBuf {
    let joined = if Path::new(rel).is_absolute() {
        PathBuf::from(rel)
    } else {
        base.join(rel)
    };
    let mut out = PathBuf::new();
    for part in joined.components() {
        match part {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other.as_os_str()),
        }
    }
    out
}

pub fn read_bounded(path: &Path) -> Option<Vec<u8>> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE_BYTES {
        return None;
    }
    let mut data = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut data)
        .ok()?;
    (data.len() as u64 <= MAX_FILE_BYTES).then_some(data)
}

fn text<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str)
}

impl PageLibrary {
    pub fn record(
        &self,
        event: &Value,
        ctx: &RecordContext<'_>,
    ) -> Result<Option<PageEntry>, PagesError> {
        if !self.is_writable() {
            return Err(PagesError::ReadOnly);
        }
        let empty = Map::new();
        let input = event
            .get("tool_input")
            .and_then(Value::as_object)
            .unwrap_or(&empty);
        let action = text(input, "action");
        if !(action.is_none() || action == Some("publish"))
            || input.get("asset").and_then(Value::as_bool) == Some(true)
        {
            return Ok(None);
        }
        let Some(path) = text(input, "file_path") else {
            return Ok(None);
        };
        let ext = Path::new(path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if ext != "html" && ext != "md" {
            return Ok(None);
        }

        let response = flatten(event.get("tool_response"));
        let url = artifact_url_in(&response)
            .or_else(|| text(input, "url").and_then(artifact_url_in))
            .unwrap_or_default();
        if url.is_empty() && looks_failed(&response) {
            return Ok(None);
        }

        let cwd = event.get("cwd").and_then(Value::as_str).unwrap_or("");
        let backfill = event.get("notchling_backfill").and_then(Value::as_object);
        let base = if cwd.is_empty() {
            ctx.home.to_path_buf()
        } else {
            PathBuf::from(cwd)
        };
        let source = join_normalized(&base, path);
        let Some(data) = read_bounded(&source) else {
            return Ok(None);
        };
        let source_text = source.to_string_lossy().into_owned();
        let fallback_title = source
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = page_title(&data, &ext, &fallback_title);

        self.with_lock(|| {
            let mut list = self.read_list();
            let existing = list.iter().position(|e| {
                if url.is_empty() {
                    e.source == source_text
                } else {
                    e.url == url
                }
            });
            let key = if url.is_empty() { &source_text } else { &url };
            let folder = match existing {
                Some(i) => list[i].folder.clone(),
                None => make_folder(&title, key),
            };
            if !super::library::safe_component(&folder) {
                return Ok(None);
            }
            let file = format!("index.{ext}");
            let folder_dir = self.dir().join(&folder);
            std::fs::create_dir_all(&folder_dir)?;

            let body = if ext == "html" {
                wrapped(&data)
            } else {
                data.clone()
            };
            notchling_platform::files::write_private_no_follow(&folder_dir.join(&file), &body)?;
            let other = if ext == "html" {
                "index.md"
            } else {
                "index.html"
            };
            let _ = std::fs::remove_file(folder_dir.join(other));
            copy_supporting(input, &base, &folder_dir, &file);

            let account = backfill
                .and_then(|b| text(b, "account"))
                .map(str::to_string)
                .or_else(|| {
                    existing
                        .map(|i| list[i].account.clone())
                        .filter(|a| !a.is_empty())
                })
                .unwrap_or_else(|| ctx.account.to_string());
            let entry = PageEntry {
                folder,
                file,
                title,
                url: url.clone(),
                source: source_text.clone(),
                project: project_name(cwd),
                account,
                updated: backfill
                    .and_then(|b| b.get("updated"))
                    .and_then(Value::as_f64)
                    .unwrap_or(ctx.now),
                version: published_version(&response),
            };
            match existing {
                Some(i) => list[i] = entry.clone(),
                None => list.push(entry.clone()),
            }
            self.save_list(&list)?;
            Ok(Some(entry))
        })
    }
}

fn copy_supporting(input: &Map<String, Value>, cwd: &Path, folder: &Path, main: &str) {
    let mut root = cwd.to_path_buf();
    if let Some(r) = text(input, "root").filter(|r| !r.is_empty()) {
        root = join_normalized(cwd, r);
    }
    let mut pairs: Vec<(String, String)> = Vec::new();
    match input.get("files") {
        Some(Value::Object(map)) => {
            for (published, value) in map {
                if let Some(s) = value.as_str() {
                    pairs.push((published.clone(), s.to_string()));
                } else if let Some(o) = value.as_object() {
                    if o.get("artifact").is_none() {
                        if let Some(from) = text(o, "from") {
                            pairs.push((published.clone(), from.to_string()));
                        }
                    }
                }
            }
        }
        Some(Value::Array(items)) => {
            for item in items.iter().filter_map(Value::as_object) {
                if item.get("artifact").is_none() {
                    if let Some(p) = text(item, "path") {
                        pairs.push((p.to_string(), p.to_string()));
                    }
                }
            }
        }
        _ => {}
    }
    pairs.sort();
    let mut total: u64 = 0;
    for (published, source) in pairs {
        let Some(parts) = safe_relative(&published) else {
            continue;
        };
        if published == main {
            continue;
        }
        let target = parts
            .iter()
            .fold(folder.to_path_buf(), |p, part| p.join(part));
        let Some(data) = read_bounded(&join_normalized(&root, &source)) else {
            continue;
        };
        if total + data.len() as u64 > MAX_TOTAL_BYTES {
            continue;
        }
        total += data.len() as u64;
        if let Some(parent) = target.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = notchling_platform::files::write_private_no_follow(&target, &data);
    }
}
