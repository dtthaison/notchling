use crate::acting_ops::Room;
use crate::daemon_wiring_embed::{read, statics};
use crate::embed_ops::{EmbedOps, EmbedParts, PluggedIn};
use crate::embed_paths;
use crate::limits_ops_tests::TempDir;
use crate::pages_ops::SearchSettings;
use notchling_engine::search::embed::helper::ProcessSpawn;
use notchling_engine::search::embed::ModelId;
use notchling_engine::search::range::IndexRange;
use notchling_engine::settings::Settings;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::sync::Arc;

struct Quiet;

impl Room for Quiet {
    fn publish(&self, _topic: &str, _event: &str, _data: &Value) {}

    fn can_answer(&self) -> bool {
        true
    }

    fn can_follow(&self, _topic: &str) -> bool {
        true
    }
}

#[test]
fn read_maps_the_semantic_keys() {
    let dir = TempDir::new("wiring-embed-read");
    let file = dir.root().join("settings.json");
    let search = SearchSettings::default();
    let empty = read(&file, &search);
    assert!(!empty.on);
    assert!(!empty.paused);
    assert!(empty.models.is_empty());
    assert_eq!(empty.range, search.range);

    let mut store = Settings::open(&file).unwrap();
    store.set_unchecked("semanticSearch", json!(true)).unwrap();
    store.set_unchecked("semanticPaused", json!(true)).unwrap();
    store
        .set_unchecked("semanticModel", json!(["granite@1", "static@1"]))
        .unwrap();
    store
        .set_unchecked("searchIndexRange", json!("30d"))
        .unwrap();
    let search = SearchSettings::read(&file);
    let got = read(&file, &search);
    assert!(got.on);
    assert!(got.paused);
    assert_eq!(got.models, vec!["granite@1", "static@1"]);
    assert_eq!(got.range, IndexRange::Days30);
    assert_eq!(got.categories, search.on.to_vec());
}

#[test]
fn a_real_layout_with_no_model_spawns_no_helper() {
    let dir = TempDir::new("wiring-embed-nomodel");
    let data = dir.root().to_path_buf();
    let file = data.join("settings.json");
    let mut store = Settings::open(&file).unwrap();
    store.set_unchecked("semanticSearch", json!(true)).unwrap();
    assert!(!embed_paths::granite_present(&data));
    assert!(!embed_paths::static_present(&data));
    let missing = data.join("no-such-helper");
    let ops = EmbedOps::with_parts(EmbedParts {
        data_dir: data.clone(),
        room: Arc::new(Quiet),
        settings: Arc::new({
            let file = file.clone();
            move || read(&file, &SearchSettings::read(&file))
        }),
        corpora: Arc::new(|_| Vec::new()),
        spawn: Arc::new(ProcessSpawn {
            exe: missing.clone(),
            model_dir: embed_paths::granite_dir(&data),
        }),
        granite_id: ModelId(embed_paths::GRANITE_ID.to_string()),
        granite_present: false,
        statics: Arc::new(|| Ok(None)),
        fixture: false,
        power: Arc::new(PluggedIn),
        net: crate::embed_fetch::ModelNet::offline(),
        now: Arc::new(crate::pages_ops::system_now),
    });
    ops.start();
    let status = ops.status();
    ops.close();
    assert_eq!(status["state"], "no_model");
    assert!(status["error"].is_null());
    assert_eq!(status["stored"], 0);
    assert!(!missing.exists());
}

#[test]
fn statics_treats_bad_checksum_as_absent_and_reports_unreadable_table() {
    let dir = TempDir::new("wiring-embed-statics");
    let data = dir.root().to_path_buf();
    let folder = embed_paths::static_dir(&data);
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::write(folder.join("tokenizer.json"), b"{}").unwrap();
    std::fs::write(folder.join("table-512i8.bin"), b"short").unwrap();

    assert!(statics(data.clone()).unwrap().is_none());

    std::fs::write(folder.join("table-512i8.sha256"), "wrong").unwrap();
    assert!(statics(data.clone()).unwrap().is_none());

    let digest: String = Sha256::digest(b"short")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    std::fs::write(folder.join("table-512i8.sha256"), digest).unwrap();
    let err = statics(data).err().expect("expected open error");
    assert!(err.starts_with("The static model could not be opened ("));
}
