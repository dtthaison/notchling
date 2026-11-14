use std::path::PathBuf;

use notchling_engine::contract::matrix::markdown;
use notchling_engine::contract::{Capability, Registry};

fn golden() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/provider-matrix.md")
}

#[test]
fn the_matrix_matches_the_golden() {
    let text = markdown(&Registry::standard());
    if std::env::var("NOTCHLING_UPDATE_GOLDEN").is_ok_and(|v| v == "1") {
        std::fs::write(golden(), &text).unwrap();
        return;
    }
    let on_disk = std::fs::read_to_string(golden()).unwrap_or_default();
    assert_eq!(
        on_disk, text,
        "docs/provider-matrix.md is out of date: NOTCHLING_UPDATE_GOLDEN=1 cargo test -p notchling-engine --test provider_matrix"
    );
}

#[test]
fn every_cell_appears_once() {
    let text = markdown(&Registry::standard());
    for c in Capability::ALL {
        let row = format!("| `{}` |", c.id());
        let rows = text.lines().filter(|l| l.starts_with(&row)).count();
        assert_eq!(rows, 1, "{}", c.id());
    }
}
