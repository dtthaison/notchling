use super::fakes::TestMachines;
use super::step::StepId;
use super::tests_versions_probe::engine_with;
use super::versions_facts::{prune, save, DOC};
use crate::store;
use crate::versions::{parse_probe, WorkerTools};
use serde_json::Value;
use std::path::PathBuf;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-vfacts-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn tools(text: &str) -> WorkerTools {
    parse_probe(text, 1)
}

fn saved(dir: &PathBuf) -> Value {
    serde_json::from_str(&store::doc(dir, DOC).unwrap().unwrap()).unwrap()
}

#[test]
fn save_keeps_other_machines() {
    let dir = temp("keeps");
    save(&dir, "m1", &tools("claude.path=/a/claude\n")).unwrap();
    save(&dir, "m2", &tools("codex.path=/a/codex\n")).unwrap();
    save(&dir, "m1", &tools("claude.path=\n")).unwrap();
    let doc = saved(&dir);
    assert_eq!(doc.as_object().unwrap().len(), 2);
    assert_eq!(doc["m1"]["tools"]["claude"]["state"], "notInstalled");
    assert_eq!(doc["m2"]["tools"]["codex"]["state"], "installed");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_damaged_doc_is_replaced_not_trusted() {
    let dir = temp("damaged");
    store::put_doc(&dir, DOC, "[1,2").unwrap();
    save(&dir, "m1", &tools("")).unwrap();
    assert_eq!(saved(&dir).as_object().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn prune_drops_removed_machines() {
    let dir = temp("prune");
    save(&dir, "m1", &tools("")).unwrap();
    save(&dir, "gone", &tools("")).unwrap();
    prune(&dir, &["m1".to_string()]).unwrap();
    let doc = saved(&dir);
    assert!(doc.get("m1").is_some());
    assert!(doc.get("gone").is_none());
    let empty = temp("prune-none");
    prune(&empty, &[]).unwrap();
    assert!(store::doc(&empty, DOC).unwrap().is_none());
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&empty);
}

#[test]
fn refresh_saves_facts_once_per_change() {
    let dir = temp("refresh");
    let m = TestMachines::mac("m1");
    let e = engine_with(&m, Some(dir.clone()));
    e.refresh(Some(&["m1".to_string()]), false);
    let first = store::doc(&dir, DOC).unwrap().unwrap();
    assert!(first.contains("\"m1\""));
    assert!(e.results("m1").contains_key(&StepId::TOOL_VERSIONS));
    store::put_doc(&dir, DOC, "{}").unwrap();
    e.refresh(Some(&["m1".to_string()]), false);
    e.refresh(Some(&["m1".to_string()]), false);
    assert_eq!(store::doc(&dir, DOC).unwrap().unwrap(), "{}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn no_data_folder_saves_nothing() {
    let m = TestMachines::mac("m1");
    let e = engine_with(&m, None);
    e.refresh(Some(&["m1".to_string()]), false);
    assert!(e.results("m1").contains_key(&StepId::TOOL_VERSIONS));
}
