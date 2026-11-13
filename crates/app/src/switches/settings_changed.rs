use std::path::Path;

use notchling_engine::settings::{spec, Settings};
use serde_json::Value;

pub fn current(data_dir: &Path, keys: &[String]) -> Vec<(String, Value)> {
    let Ok(store) = Settings::open(&data_dir.join("settings.json")) else {
        return Vec::new();
    };
    keys.iter()
        .filter(|key| spec(key).is_some())
        .filter_map(|key| Some((key.clone(), store.get(key)?)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Dir(std::path::PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn dir(tag: &str) -> Dir {
        let d =
            std::env::temp_dir().join(format!("nl-settings-changed-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    #[test]
    fn a_change_is_read_back_from_the_store_not_taken_from_the_event() {
        let d = dir("store");
        let mut store = Settings::open(&d.0.join("settings.json")).unwrap();
        store.set("chatHistory", json!("16mb")).unwrap();
        let keys = ["chatHistory", "showPet", "noSuchKey"].map(String::from);
        assert_eq!(
            current(&d.0, &keys),
            vec![
                ("chatHistory".to_owned(), json!("16mb")),
                ("showPet".to_owned(), json!(true)),
            ],
            "the stored value, a default for an unset key, nothing for an unknown one"
        );
        store.set("chatHistory", json!("2mb")).unwrap();
        assert_eq!(
            current(&d.0, &keys[..1]),
            vec![("chatHistory".to_owned(), json!("2mb"))],
            "a later write wins over what an older event said"
        );
    }
}
