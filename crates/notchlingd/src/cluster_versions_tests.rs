use super::*;
use notchling_engine::cluster::versions::{CLUSTER_SCHEMA, FEATURE_DISPATCH, NODE_PROTO};
use std::path::{Path, PathBuf};

struct Home(PathBuf);

impl Home {
    fn new(tag: &str) -> Home {
        let path = PathBuf::from(format!("/tmp/nl-cv-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        Home(path)
    }

    fn write(&self, rel: &str, body: &str) -> PathBuf {
        let path = self.0.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    }

    fn root(&self) -> &Path {
        &self.0
    }
}

impl Drop for Home {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
#[test]
fn probe_uses_detect_and_adds_node_fields() {
    use std::os::unix::fs::PermissionsExt;
    let home = Home::new("probe");
    let ran = home.root().join("ran");
    let script = format!("#!/bin/sh\ntouch '{}'\n", ran.display());
    let target = home.write(".local/share/claude/versions/2.1.324", &script);
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o755)).unwrap();
    let link = home.root().join(".local/bin/claude");
    std::fs::create_dir_all(link.parent().unwrap()).unwrap();
    std::os::unix::fs::symlink(&target, &link).unwrap();
    home.write(
        "opt/homebrew/lib/node_modules/@openai/codex/package.json",
        r#"{"version":"0.166.0","bin":"bin/codex.js"}"#,
    );
    home.write(
        "opt/homebrew/lib/node_modules/@openai/codex/bin/codex.js",
        &script,
    );
    let roots = Roots::inside(home.root());
    let mut cache = IdentityCache::new();
    let agy = Version::parse("1.2.3");
    let got = probe(&roots, &mut cache, agy.clone());
    let expect = |tool| {
        detect(&roots, tool, &mut IdentityCache::new())
            .and_then(|i| i.version)
            .unwrap()
    };
    assert_eq!(got.tools.get(&Tool::Claude), Some(&expect(Tool::Claude)));
    assert_eq!(got.tools.get(&Tool::Codex), Some(&expect(Tool::Codex)));
    assert_eq!(got.tools.get(&Tool::Agy), agy.as_ref());
    assert_eq!(got.notchling, env!("CARGO_PKG_VERSION"));
    assert_eq!(got.proto, NODE_PROTO);
    assert_eq!(got.schema, CLUSTER_SCHEMA);
    assert_eq!(got.os, std::env::consts::OS);
    assert_eq!(got.arch, std::env::consts::ARCH);
    assert!(got.features.iter().any(|f| f == FEATURE_DISPATCH));
    assert!(got.logins.is_empty());
    assert!(!ran.exists(), "the probe starts nothing");
}

#[test]
fn unknown_agy_version_stays_unknown() {
    let home = Home::new("agy");
    home.write(".local/bin/agy", "#!/bin/sh\n");
    let got = probe(&Roots::inside(home.root()), &mut IdentityCache::new(), None);
    assert!(!got.tools.contains_key(&Tool::Agy));
    assert!(!got.tools.contains_key(&Tool::Claude));
}

#[test]
fn with_tools_keeps_the_node_tools_only() {
    let v = |s: &str| Version::parse(s).unwrap();
    let tools = BTreeMap::from([
        (Tool::Claude, v("2.1.324")),
        (Tool::Agy, v("1.2.3")),
        (Tool::Notchling, v("0.9.0")),
    ]);
    let versions = with_tools(tools);
    assert_eq!(versions.tools.len(), 2);
    assert_eq!(versions.tools[&Tool::Claude], v("2.1.324"));
    assert_eq!(versions.tools[&Tool::Agy], v("1.2.3"));
    assert_eq!(versions.notchling, env!("CARGO_PKG_VERSION"));
}
