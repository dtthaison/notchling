mod fluent_ref;

use std::path::{Path, PathBuf};

use notchling_locale::{shipped, testing};

fn locales() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../locales")
}

#[test]
fn every_message_matches_fluent() {
    for tag in shipped() {
        fluent_ref::compare(&locales(), tag, testing::messages(tag), &|key, args| {
            testing::format_plain_numbers(tag, key, args)
        });
    }
}

#[test]
fn every_ftl_file_parses_cleanly() {
    let mut paths = Vec::new();
    fluent_ref::files(&locales(), &mut paths);
    assert!(!paths.is_empty());
    for path in paths {
        fluent_ref::parse(&path);
    }
}
