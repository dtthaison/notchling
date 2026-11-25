use std::collections::BTreeSet;

use super::baseline::{entries, FOUNDING, GAPS, UNWIRED};
use super::{Capability, Kind, Registry, Tool};

fn listed(text: &str) -> BTreeSet<String> {
    entries(text).unwrap().into_iter().collect()
}

#[test]
fn every_gap_is_listed_and_every_listed_cell_is_a_gap() {
    let gaps: BTreeSet<String> = Registry::standard()
        .cells()
        .iter()
        .filter(|c| c.support.kind() == Kind::Gap)
        .map(|c| c.entry())
        .collect();
    assert_eq!(gaps, listed(GAPS));
}

#[test]
fn unwired_lists_exactly_the_native_and_fallback_cells_not_wired() {
    let cells = Registry::standard().cells();
    let unwired: BTreeSet<String> = cells
        .iter()
        .filter(|c| matches!(c.support.kind(), Kind::Native | Kind::Fallback) && !c.wired)
        .map(|c| c.entry())
        .collect();
    let file = listed(UNWIRED);
    assert_eq!(unwired, file);
    for c in &cells {
        if c.wired || matches!(c.support.kind(), Kind::Gap | Kind::NotApplicable) {
            assert!(!file.contains(&c.entry()), "{}", c.entry());
        }
    }
}

#[test]
fn the_files_are_sorted_unique_and_well_formed() {
    for text in [GAPS, UNWIRED] {
        let list = entries(text).unwrap();
        let mut sorted = list.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(list, sorted);
        for e in &list {
            let (tool, id) = e.split_once(' ').unwrap();
            assert!(Tool::parse(tool).is_some() && tool != "agy", "{e}");
            assert!(Capability::parse(id).is_some(), "{e}");
        }
        assert!(text.ends_with('\n'));
        assert_eq!(super::baseline::render(&list), text);
    }
}

#[test]
fn a_new_tool_starts_with_no_gaps() {
    let registry = Registry::standard();
    let named: Vec<String> = entries(GAPS)
        .unwrap()
        .into_iter()
        .chain(entries(UNWIRED).unwrap())
        .collect();
    for tool in Tool::ALL.into_iter().filter(|t| !FOUNDING.contains(t)) {
        for c in Capability::ALL {
            assert_ne!(registry.support(tool, c).kind(), Kind::Gap, "{}", c.id());
        }
        assert!(named.iter().all(|e| !e.starts_with(tool.name())));
    }
}

#[test]
fn wired_cells_are_native_or_fallback() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        for c in registry.of(tool).wired() {
            assert!(
                matches!(
                    registry.support(tool, *c).kind(),
                    Kind::Native | Kind::Fallback
                ),
                "{} {}",
                tool.name(),
                c.id()
            );
        }
    }
}

#[test]
fn the_registry_answers_for_each_tool_in_order() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        assert_eq!(registry.of(tool).tool(), tool);
    }
    let cells = registry.cells();
    assert_eq!(cells.len(), Tool::ALL.len() * Capability::ALL.len());
    assert_eq!(cells[0].entry(), "claude session.list");
    assert_eq!(cells[39].entry(), "codex session.list");
}
