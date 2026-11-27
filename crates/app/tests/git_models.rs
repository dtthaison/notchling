use std::path::Path;

const MODELS: [&str; 4] = [
    "src/chat/worktrees.rs",
    "src/chat/branch_list.rs",
    "src/chat/changes_target.rs",
    "src/chat/git_gates.rs",
];

const FORBIDDEN: [&str; 7] = [
    "std::fs",
    "std::process",
    "Command",
    "File::",
    "read_dir",
    "metadata(",
    "actions",
];

#[test]
fn models_take_no_io() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    for file in MODELS {
        let source = std::fs::read_to_string(root.join(file)).expect("model source");
        let code = source
            .split("#[cfg(test)]")
            .next()
            .unwrap_or(source.as_str());
        for word in FORBIDDEN {
            assert!(!code.contains(word), "{file} names {word}");
        }
    }
}
