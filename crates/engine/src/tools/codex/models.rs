use serde_json::Value;
use std::io::Read;
use std::path::{Path, PathBuf};

const MAX_CACHE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Model {
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub efforts: Vec<String>,
    pub default_effort: Option<String>,
    pub context_window: Option<u64>,
}

pub const SNAPSHOT: &str = "codex-models.json";

pub fn source_file(codex_home: &Path, snapshot: Option<&Path>) -> PathBuf {
    let cache = codex_home.join("models_cache.json");
    let modified = |p: &Path| std::fs::metadata(p).and_then(|m| m.modified()).ok();
    match (snapshot, modified(&cache)) {
        (Some(s), Some(cached)) if modified(s).is_some_and(|m| m > cached) => s.to_path_buf(),
        (Some(s), None) if modified(s).is_some() => s.to_path_buf(),
        _ => cache,
    }
}

pub fn load_with(codex_home: &Path, snapshot: Option<&Path>) -> Vec<Model> {
    load_file(&source_file(codex_home, snapshot))
}

pub fn load(codex_home: &Path) -> Vec<Model> {
    load_file(&codex_home.join("models_cache.json"))
}

fn load_file(path: &Path) -> Vec<Model> {
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let mut data = Vec::new();
    if file.take(MAX_CACHE_BYTES).read_to_end(&mut data).is_err() {
        return Vec::new();
    }
    parse(&data)
}

pub fn parse(data: &[u8]) -> Vec<Model> {
    let Ok(root) = serde_json::from_slice::<Value>(data) else {
        return Vec::new();
    };
    let Some(models) = root.get("models").and_then(Value::as_array) else {
        return Vec::new();
    };
    let text = |m: &Value, key: &str| m.get(key).and_then(Value::as_str).map(String::from);
    let mut listed: Vec<&Value> = models
        .iter()
        .filter(|m| text(m, "visibility").as_deref().unwrap_or("list") == "list")
        .collect();
    listed.sort_by_key(|m| {
        m.get("priority")
            .and_then(Value::as_i64)
            .unwrap_or(i64::MAX)
    });
    listed
        .into_iter()
        .filter_map(|m| {
            let slug = text(m, "slug")?;
            let efforts = m
                .get("supported_reasoning_levels")
                .and_then(Value::as_array)
                .map(|levels| levels.iter().filter_map(|l| text(l, "effort")).collect())
                .unwrap_or_default();
            Some(Model {
                name: text(m, "display_name").unwrap_or_else(|| slug.clone()),
                summary: text(m, "description").unwrap_or_default(),
                efforts,
                default_effort: text(m, "default_reasoning_level"),
                context_window: m.get("context_window").and_then(Value::as_u64),
                slug,
            })
        })
        .collect()
}

pub fn supports(models: &[Model], model: &str, effort: &str) -> bool {
    match models.iter().find(|m| m.slug == model) {
        Some(m) => m.efforts.is_empty() || m.efforts.iter().any(|e| e == effort),
        None => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CACHE: &str = r#"{"models":[
      {"slug":"b","display_name":"B","priority":2,"visibility":"list","default_reasoning_level":"medium",
       "supported_reasoning_levels":[{"effort":"low"},{"effort":"medium"}]},
      {"slug":"hidden","display_name":"H","priority":0,"visibility":"hide"},
      {"slug":"a","display_name":"A","priority":1,"visibility":"list","description":"Fast"},
      {"display_name":"no slug","visibility":"list"}
    ]}"#;

    #[test]
    fn the_list_is_codexs_listed_models_in_its_order() {
        let models = parse(CACHE.as_bytes());
        let slugs: Vec<_> = models.iter().map(|m| m.slug.as_str()).collect();
        assert_eq!(slugs, ["a", "b"], "listed only, in Codex's order");
        assert_eq!(models[0].summary, "Fast");
        assert_eq!(models[1].efforts, ["low", "medium"]);
        assert_eq!(models[1].default_effort.as_deref(), Some("medium"));
        assert_eq!(models[0].name, "A");
    }

    #[test]
    fn the_context_window_is_read_when_the_cache_has_one() {
        let models = parse(
            br#"{"models":[{"slug":"a","context_window":272000},{"slug":"b","context_window":"big"},{"slug":"c"}]}"#,
        );
        let windows: Vec<_> = models.iter().map(|m| m.context_window).collect();
        assert_eq!(windows, [Some(272_000), None, None]);
    }

    #[test]
    fn a_model_without_a_name_is_named_by_its_slug() {
        let models = parse(br#"{"models":[{"slug":"x"}]}"#);
        assert_eq!(models[0].name, "x");
        assert!(models[0].efforts.is_empty());
        assert_eq!(models[0].default_effort, None);
    }

    #[test]
    fn nothing_readable_is_no_models() {
        for data in ["", "{}", "[]", "not json", r#"{"models":5}"#] {
            assert!(parse(data.as_bytes()).is_empty(), "{data:?}");
        }
        assert!(load(Path::new("/no/such/codex/home")).is_empty());
    }

    #[test]
    fn it_reads_the_cache_file_from_the_codex_home() {
        let dir = std::env::temp_dir().join(format!("nl-codex-models-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(dir.join("models_cache.json"), CACHE).expect("write");
        assert_eq!(load(&dir).len(), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_newer_of_codexs_cache_and_the_snapshot_is_the_list() {
        use std::time::{Duration, SystemTime};
        let dir = std::env::temp_dir().join(format!("nl-codex-snap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("dir");
        let cache = dir.join("models_cache.json");
        let snapshot = dir.join(SNAPSHOT);
        let set = |path: &Path, ago: u64| {
            let file = std::fs::OpenOptions::new()
                .write(true)
                .open(path)
                .expect("open");
            file.set_modified(SystemTime::now() - Duration::from_secs(ago))
                .expect("mtime");
        };
        assert_eq!(source_file(&dir, Some(&snapshot)), cache, "neither exists");
        std::fs::write(&cache, CACHE).expect("write");
        std::fs::write(
            &snapshot,
            r#"{"models":[{"slug":"new","visibility":"list"}]}"#,
        )
        .expect("write");
        set(&cache, 100);
        set(&snapshot, 10);
        assert_eq!(source_file(&dir, Some(&snapshot)), snapshot);
        let slugs: Vec<_> = load_with(&dir, Some(&snapshot))
            .into_iter()
            .map(|m| m.slug)
            .collect();
        assert_eq!(slugs, ["new"]);
        set(&snapshot, 200);
        assert_eq!(
            source_file(&dir, Some(&snapshot)),
            cache,
            "Codex's own is newer"
        );
        assert_eq!(load_with(&dir, None).len(), 2);
        std::fs::remove_file(&cache).expect("remove");
        assert_eq!(
            source_file(&dir, Some(&snapshot)),
            snapshot,
            "no cache of its own"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_effort_is_checked_against_the_model_when_the_cache_knows_it() {
        let models = parse(CACHE.as_bytes());
        assert!(supports(&models, "b", "low"));
        assert!(!supports(&models, "b", "xhigh"));
        assert!(supports(&models, "a", "xhigh"), "no list for a: any");
        assert!(supports(&models, "unknown", "xhigh"));
    }
}
