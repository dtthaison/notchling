use super::dist_manifest::read;
use super::fakes_b::TempDir;

const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn put(dir: &TempDir, text: &str) {
    std::fs::write(dir.path().join("manifest.json"), text).unwrap();
}

fn manifest(build: &str, name: &str, hash: &str) -> String {
    format!(r#"{{"build":"{build}","files":{{"{name}":{{"sha256":"{hash}","size":12}}}}}}"#)
}

#[test]
fn reads_build_and_hashes() {
    let dir = TempDir::new("dist-ok");
    put(
        &dir,
        &manifest("abc1234def56", "notchling-linux-x86_64", HASH),
    );
    let found = read(dir.path()).unwrap();
    assert_eq!(found.build, "abc1234def56");
    let file = &found.files["notchling-linux-x86_64"];
    assert_eq!(file.sha256, HASH);
    assert_eq!(file.size, 12);
}

#[test]
fn reads_a_dirty_build_and_every_known_name() {
    let dir = TempDir::new("dist-names");
    let files = [
        "notchling-linux-x86_64",
        "notchling-linux-aarch64",
        "notchling-macos-x86_64",
    ]
    .map(|n| format!(r#""{n}":{{"sha256":"{HASH}","size":1}}"#))
    .join(",");
    put(
        &dir,
        &format!(r#"{{"build":"abc1234def56-dirty","files":{{{files}}}}}"#),
    );
    let found = read(dir.path()).unwrap();
    assert_eq!(found.build, "abc1234def56-dirty");
    assert_eq!(found.files.len(), 3);
}

#[test]
fn rejects_bad_hex() {
    for bad in [
        &HASH[..63],
        &format!("{HASH}0"),
        &HASH.to_uppercase(),
        &format!("{}g", &HASH[..63]),
        "",
    ] {
        let dir = TempDir::new("dist-hex");
        put(
            &dir,
            &manifest("abc1234def56", "notchling-linux-x86_64", bad),
        );
        assert!(read(dir.path()).is_none(), "{bad:?}");
    }
}

#[test]
fn rejects_a_bad_build_id() {
    for bad in ["", "main", "ABC1234DEF56", "abc12", "abc1234;rm"] {
        let dir = TempDir::new("dist-build");
        put(&dir, &manifest(bad, "notchling-linux-x86_64", HASH));
        assert!(read(dir.path()).is_none(), "{bad:?}");
    }
}

#[test]
fn rejects_a_file_name_outside_the_known_set() {
    for bad in [
        "notchling",
        "../notchling-linux-x86_64",
        "notchling-linux-x86_64.new",
        "notchling-windows-x86_64",
        "notchling-linux-riscv",
        "/etc/passwd",
    ] {
        let dir = TempDir::new("dist-name");
        put(&dir, &manifest("abc1234def56", bad, HASH));
        assert!(read(dir.path()).is_none(), "{bad:?}");
    }
}

#[test]
fn missing_manifest_is_none() {
    let dir = TempDir::new("dist-none");
    assert!(read(dir.path()).is_none());
    assert!(read(&dir.path().join("absent")).is_none());
}

#[test]
fn unreadable_json_is_none() {
    let dir = TempDir::new("dist-junk");
    put(&dir, "not json");
    assert!(read(dir.path()).is_none());
    put(&dir, r#"{"build":"abc1234def56"}"#);
    assert!(read(dir.path()).is_none());
}
