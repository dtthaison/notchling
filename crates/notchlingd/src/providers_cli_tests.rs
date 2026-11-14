use notchling_engine::contract::{matrix, Registry};
use serde_json::Value;

use crate::providers_cli::run;

#[test]
fn the_text_is_the_matrix() {
    let mut out = Vec::new();
    assert_eq!(run(false, &mut out), 0);
    assert_eq!(
        String::from_utf8(out).unwrap(),
        matrix::markdown(&Registry::standard())
    );
}

#[test]
fn the_json_has_117_cells_and_says_which_are_wired() {
    let mut out = Vec::new();
    assert_eq!(run(true, &mut out), 0);
    let value: Value = serde_json::from_slice(&out).unwrap();
    let cells = value["cells"].as_array().unwrap();
    assert_eq!(cells.len(), 117);
    for cell in cells {
        for key in ["tool", "capability", "support", "line", "flow", "wired"] {
            assert!(cell.get(key).is_some(), "{key} in {cell}");
        }
        assert!(cell["wired"].is_boolean());
    }
    assert_eq!(value["tools"].as_array().unwrap().len(), 3);
}
