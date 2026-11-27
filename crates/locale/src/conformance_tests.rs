#![allow(dead_code)]

use std::path::{Path, PathBuf};

use crate::format::{format_with, Body, Table};
use crate::testing::{numeric_of, PlainNumbers};
use crate::{numbers::Icu, Arg};

mod fixture {
    include!("../../../tools/locale/tests/golden/generated.rs");
}

#[path = "../tests/fluent_ref/mod.rs"]
mod fluent_ref;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tools/locale/tests/fixtures/tree")
}

fn bodies(tag: &str) -> Vec<Option<Body>> {
    match fixture::TABLES.iter().find(|(t, _)| *t == tag) {
        Some((_, Table::Full(all))) => all.iter().copied().map(Some).collect(),
        Some((_, Table::Part(all))) => all.to_vec(),
        None => Vec::new(),
    }
}

fn run(tag: &str) -> usize {
    let all = bodies(tag);
    let messages: Vec<(&str, Vec<bool>)> = all
        .iter()
        .enumerate()
        .filter_map(|(i, body)| body.as_ref().map(|b| (fixture::KEYS[i].0, numeric_of(b))))
        .collect();
    let numbers = PlainNumbers(Icu::new(tag, None));
    let ours = |key: &str, args: &[Arg<'_>]| -> Option<String> {
        let at = fixture::KEYS
            .binary_search_by(|(k, _)| (*k).cmp(key))
            .ok()?;
        let body = all.get(at).copied().flatten()?;
        Some(format_with(&body, args, &numbers, false))
    };
    fluent_ref::compare(&root(), tag, messages, &ours)
}

#[test]
fn the_fixture_tables_match_fluent_in_english() {
    assert!(run("en") > 20);
}

#[test]
fn the_fixture_tables_match_fluent_in_partial_languages() {
    assert!(run("pt") >= 2);
    assert!(run("pt-BR") >= 1);
}

#[test]
fn the_fixture_has_plurals_numbers_and_terms() {
    let all = bodies("en");
    assert!(all.iter().flatten().any(|b| matches!(b, Body::Plural(..))));
    assert!(all.iter().flatten().any(|b| matches!(b, Body::Parts(_))));
}
