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

    #[test]
    fn a_change_is_read_back_from_the_store_not_taken_from_the_event() {
        let d = crate::testutil::Scratch::new("settings-changed");
        let mut store = Settings::open(&d.path().join("settings.json")).unwrap();
        store.set("chatHistory", json!("16mb")).unwrap();
        let keys = ["chatHistory", "showPet", "noSuchKey"].map(String::from);
        assert_eq!(
            current(d.path(), &keys),
            vec![
                ("chatHistory".to_owned(), json!("16mb")),
                ("showPet".to_owned(), json!(true)),
            ],
            "the stored value, a default for an unset key, nothing for an unknown one"
        );
        store.set("chatHistory", json!("2mb")).unwrap();
        assert_eq!(
            current(d.path(), &keys[..1]),
            vec![("chatHistory".to_owned(), json!("2mb"))],
            "a later write wins over what an older event said"
        );
    }
}
