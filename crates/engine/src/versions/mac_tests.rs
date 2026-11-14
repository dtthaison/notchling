use super::mac::{detect, watch_paths, Problem, Roots};
use super::{IdentityCache, Method, Tool};
use crate::sessions::testutil::TempDir;
use std::path::Path;

fn roots(dir: &TempDir) -> Roots {
    Roots::inside(dir.root())
}

fn run(dir: &TempDir, tool: Tool) -> Option<super::mac::Installed> {
    detect(&roots(dir), tool, &mut IdentityCache::new())
}

#[cfg(unix)]
fn native_claude(dir: &TempDir, version: &str) {
    let target = dir.write(
        &format!(".local/share/claude/versions/{version}"),
        &format!("claude {version}"),
    );
    let link = dir.path(".local/bin/claude");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    let _ = std::fs::remove_file(&link);
    std::os::unix::fs::symlink(target, link).unwrap();
}

fn npm_package(dir: &TempDir, prefix: &str, scope: &str, name: &str, json: &str, bin: &str) {
    dir.write(
        &format!("{prefix}/lib/node_modules/{scope}/{name}/package.json"),
        json,
    );
    dir.write(
        &format!("{prefix}/lib/node_modules/{scope}/{name}/{bin}"),
        "#!/usr/bin/env node\n",
    );
}

#[cfg(unix)]
#[test]
fn native_layout_reads_the_link() {
    let dir = TempDir::new("mac-native");
    native_claude(&dir, "2.1.324");
    let found = run(&dir, Tool::Claude).unwrap();
    assert_eq!(found.version.unwrap().as_str(), "2.1.324");
    assert_eq!(found.method, Method::Native);
    assert!(found.path.ends_with(".local/bin/claude"));
    assert_eq!(found.identity.unwrap().sha256.len(), 64);
    assert!(found.problems.is_empty());
}

#[test]
fn npm_layout_reads_package_json() {
    let dir = TempDir::new("mac-npm");
    npm_package(
        &dir,
        "opt/homebrew",
        "@anthropic-ai",
        "claude-code",
        r#"{"name":"@anthropic-ai/claude-code","version":"2.1.320","bin":{"claude":"cli.js"}}"#,
        "cli.js",
    );
    let found = run(&dir, Tool::Claude).unwrap();
    assert_eq!(found.version.unwrap().as_str(), "2.1.320");
    assert_eq!(found.method, Method::Npm);
    assert!(found.path.ends_with("claude-code/cli.js"));
    assert!(found.identity.is_some());
}

#[test]
fn brew_layout_reads_the_cask_folder() {
    let dir = TempDir::new("mac-brew");
    dir.write("opt/homebrew/Caskroom/claude-code/2.1.322/claude", "old");
    dir.write("opt/homebrew/Caskroom/claude-code/2.1.324/claude", "new");
    let found = run(&dir, Tool::Claude).unwrap();
    assert_eq!(found.version.unwrap().as_str(), "2.1.324");
    assert_eq!(found.method, Method::Brew);
    assert!(found.identity.is_some());
    dir.write("opt/homebrew/Caskroom/codex/0.166.0/codex", "x");
    let codex = run(&dir, Tool::Codex).unwrap();
    assert_eq!(codex.version.unwrap().as_str(), "0.166.0");
    assert_eq!(codex.method, Method::Brew);
}

#[test]
fn codex_standalone_reads_its_package_file() {
    let dir = TempDir::new("mac-codex");
    dir.write(
        ".codex/packages/standalone/releases/0.165.0/codex-package.json",
        r#"{"version":"0.165.0"}"#,
    );
    dir.write(
        ".codex/packages/standalone/releases/0.166.0/codex-package.json",
        r#"{"version":"0.166.0"}"#,
    );
    dir.write(".codex/packages/standalone/releases/0.166.0/codex", "bin");
    let found = run(&dir, Tool::Codex).unwrap();
    assert_eq!(found.version.unwrap().as_str(), "0.166.0");
    assert_eq!(found.method, Method::Standalone);
    assert!(found.identity.is_some());
}

#[test]
fn codex_npm_reads_package_json() {
    let dir = TempDir::new("mac-codex-npm");
    npm_package(
        &dir,
        ".npm-global",
        "@openai",
        "codex",
        r#"{"version":"0.166.0","bin":"bin/codex.js"}"#,
        "bin/codex.js",
    );
    let found = run(&dir, Tool::Codex).unwrap();
    assert_eq!(found.version.unwrap().as_str(), "0.166.0");
    assert_eq!(found.method, Method::Npm);
}

#[cfg(unix)]
#[test]
fn two_installs_are_a_problem() {
    let dir = TempDir::new("mac-two");
    native_claude(&dir, "2.1.324");
    npm_package(
        &dir,
        "opt/homebrew",
        "@anthropic-ai",
        "claude-code",
        r#"{"version":"2.1.100","bin":{"claude":"cli.js"}}"#,
        "cli.js",
    );
    let found = run(&dir, Tool::Claude).unwrap();
    assert_eq!(found.method, Method::Native);
    assert_eq!(found.version.unwrap().as_str(), "2.1.324");
    assert_eq!(
        found.problems,
        vec![Problem::TwoInstalls { other: Method::Npm }]
    );
}

#[test]
fn agy_found_without_version() {
    let dir = TempDir::new("mac-agy");
    dir.write(".local/bin/agy", "binary");
    let found = run(&dir, Tool::Agy).unwrap();
    assert!(found.version.is_none());
    assert_eq!(found.method, Method::Standalone);
    assert!(found.problems.is_empty());
    assert!(found.identity.is_some());
}

#[test]
fn nothing_installed_is_none() {
    let dir = TempDir::new("mac-none");
    for tool in Tool::ALL {
        assert!(run(&dir, tool).is_none(), "{tool:?}");
    }
}

#[test]
fn bad_version_in_package_json_is_cant_tell() {
    let dir = TempDir::new("mac-bad");
    npm_package(
        &dir,
        "opt/homebrew",
        "@anthropic-ai",
        "claude-code",
        r#"{"version":"2.1.3; rm","bin":{"claude":"cli.js"}}"#,
        "cli.js",
    );
    let found = run(&dir, Tool::Claude).unwrap();
    assert!(found.version.is_none());
    assert_eq!(found.problems, vec![Problem::CantTell]);
}

#[test]
fn bin_entries_cannot_leave_the_package() {
    let dir = TempDir::new("mac-bin");
    npm_package(
        &dir,
        "opt/homebrew",
        "@openai",
        "codex",
        r#"{"version":"0.166.0","bin":"../../../../outside"}"#,
        "x",
    );
    let found = run(&dir, Tool::Codex).unwrap();
    assert!(found.identity.is_none());
    assert!(!found.path.contains(".."));
}

#[test]
fn notchling_is_not_read_here() {
    let dir = TempDir::new("mac-notchling");
    dir.write(".local/bin/notchling", "bin");
    assert!(run(&dir, Tool::Notchling).is_none());
}

#[test]
fn installed_serializes_in_the_wire_shape() {
    let dir = TempDir::new("mac-json");
    dir.write(".local/bin/agy", "binary");
    let found = run(&dir, Tool::Agy).unwrap();
    let json = serde_json::to_value(&found).unwrap();
    assert_eq!(json["tool"], "antigravity");
    assert_eq!(json["version"], serde_json::Value::Null);
    assert_eq!(json["method"], "standalone");
    assert!(json["identity"]["sha256"].is_string());
    assert_eq!(json["problems"], serde_json::json!([]));
}

#[test]
fn watch_paths_lists_only_folders_that_exist() {
    let dir = TempDir::new("mac-watch");
    assert!(watch_paths(&roots(&dir)).is_empty());
    dir.write(".local/bin/agy", "x");
    dir.write("opt/homebrew/Caskroom/codex/0.1.0/codex", "x");
    std::fs::create_dir_all(dir.path("opt/homebrew/lib/node_modules/@openai")).unwrap();
    let paths = watch_paths(&roots(&dir));
    assert!(paths.contains(&dir.path(".local/bin")));
    assert!(paths.contains(&dir.path("opt/homebrew/Caskroom")));
    assert!(paths.contains(&dir.path("opt/homebrew/lib/node_modules/@openai")));
    assert_eq!(paths.len(), 3);
    assert!(paths
        .iter()
        .all(|p: &std::path::PathBuf| Path::new(p).starts_with(dir.root())));
}

#[cfg(unix)]
#[test]
fn detect_starts_nothing() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new("mac-ran");
    let ran = dir.path("ran");
    let script = format!("#!/bin/sh\ntouch '{}'\n", ran.display());
    let make = |rel: &str| {
        let path = dir.write(rel, &script);
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    make(".local/share/claude/versions/2.1.324");
    let link = dir.path(".local/bin/claude");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(dir.path(".local/share/claude/versions/2.1.324"), &link).unwrap();
    make(".local/bin/agy");
    make(".local/bin/codex");
    make(".local/bin/notchling");
    make("opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js");
    dir.write(
        "opt/homebrew/lib/node_modules/@openai/codex/package.json",
        r#"{"version":"0.166.0","bin":"bin/codex.js"}"#,
    );
    make(".codex/packages/standalone/0.166.0/codex");
    dir.write(
        ".codex/packages/standalone/0.166.0/codex-package.json",
        r#"{"version":"0.166.0"}"#,
    );
    let roots = roots(&dir);
    let mut cache = IdentityCache::new();
    for tool in Tool::ALL {
        let _ = detect(&roots, tool, &mut cache);
    }
    let _ = watch_paths(&roots);
    assert!(!ran.exists());
}
