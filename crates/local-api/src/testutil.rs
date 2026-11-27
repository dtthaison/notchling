use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct TempDir(PathBuf);

impl TempDir {
    pub fn new(tag: &str) -> TempDir {
        static N: AtomicU32 = AtomicU32::new(0);
        let base = if cfg!(unix) {
            PathBuf::from("/tmp")
        } else {
            std::env::temp_dir()
        };
        let n = N.fetch_add(1, Ordering::SeqCst);
        let path = base.join(format!("nl-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        notchling_platform::files::ensure_private_dir(&path).expect("make temp dir");
        TempDir(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn field_text(event: &notchling_events::Event, name: &str) -> Option<String> {
    use notchling_events::field::FieldValue;
    match event.fields.get(name)? {
        FieldValue::Name(text) => Some((*text).to_owned()),
        FieldValue::Token(text) => Some(text.to_string()),
        _ => None,
    }
}

pub fn summary(sink: &notchling_events::MemorySink) -> Vec<(String, String)> {
    sink.events()
        .iter()
        .map(|event| {
            let detail = ["op", "why", "code", "result"]
                .iter()
                .find_map(|name| field_text(event, name))
                .unwrap_or_default();
            (event.spec.name.to_owned(), detail)
        })
        .collect()
}
