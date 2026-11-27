use notchling_engine::contract::{Capability, Kind, Registry, Tool};

use super::capabilities::*;

#[test]
fn native_cells_are_left_out() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        let expected = Capability::ALL
            .into_iter()
            .filter(|c| registry.support(tool, *c).kind() != Kind::Native)
            .count();
        assert_eq!(rows(tool).len(), expected, "{}", tool.name());
    }
    assert_eq!(parts().len(), Tool::ALL.len());
}

#[test]
fn every_row_says_why() {
    for (tool, list) in parts() {
        for row in list {
            assert!(!row.line.is_empty(), "{} {}", tool.name(), row.capability);
            assert!(!row.capability.is_empty());
            assert!(!row.state.word().is_empty());
        }
    }
}

#[test]
fn the_order_is_the_capability_order() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        let expected: Vec<&str> = Capability::ALL
            .into_iter()
            .filter(|c| registry.support(tool, *c).kind() != Kind::Native)
            .map(|c| c.title())
            .collect();
        let shown: Vec<&str> = rows(tool).iter().map(|r| r.capability).collect();
        assert_eq!(shown, expected, "{}", tool.name());
    }
}

#[test]
fn each_state_has_its_own_word() {
    let words = [
        RowState::Fallback.word(),
        RowState::NotApplicable.word(),
        RowState::Gap.word(),
    ];
    assert_eq!(
        words,
        ["Notchling's own", "Not applicable", "Not built yet"]
    );
}
