use super::manifest::manifest;

fn deny_allow() -> Vec<String> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../deny.toml");
    let text = std::fs::read_to_string(root).unwrap();
    let start = text.find("[licenses]").unwrap();
    let body = &text[start..];
    let from = body.find("allow = [").unwrap() + "allow = [".len();
    let to = from + body[from..].find(']').unwrap();
    body[from..to]
        .split(',')
        .map(|s| s.trim().trim_matches('"').to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

#[test]
fn licenses_are_within_deny_toml() {
    let allow = deny_allow();
    assert!(allow.iter().any(|l| l == "Apache-2.0"));
    for model in &manifest().models {
        for file in &model.files {
            assert!(allow.contains(&file.license), "{}", file.license);
        }
    }
}

#[test]
fn every_file_has_a_sha256_and_a_size() {
    assert!(!manifest().models.is_empty());
    for model in &manifest().models {
        assert!(!model.files.is_empty());
        let mut total = 0;
        for file in &model.files {
            assert_eq!(file.sha256.len(), 64);
            assert!(file
                .sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)));
            assert!(file.size > 0);
            assert_eq!(file.revision.len(), 40);
            assert!(!file.path.contains(".."));
            assert!(!file.save_as.contains('/') && !file.save_as.contains(".."));
            total += file.size;
        }
        assert_eq!(model.download_bytes, total, "{}", model.id);
    }
}

#[test]
fn ids_match_the_daemons_model_ids() {
    let ids: Vec<&str> = manifest().models.iter().map(|m| m.id.as_str()).collect();
    assert!(ids.contains(&"granite-97m-r2@835ad14/p1"));
    assert!(ids.contains(&"static-mrl@b68f412/512i8/p1"));
    let folders: Vec<&str> = manifest()
        .models
        .iter()
        .map(|m| m.folder.as_str())
        .collect();
    assert_eq!(folders, ["granite-97m-r2", "static-mrl"]);
}

#[test]
fn hosts_are_https_names_only() {
    assert!(!manifest().hosts.is_empty());
    for host in &manifest().hosts {
        let name = host.strip_prefix("*.").unwrap_or(host);
        assert!(name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-'));
        assert!(name.contains('.'));
    }
    assert!(manifest().hosts.iter().any(|h| h == "huggingface.co"));
}

#[test]
fn urls_are_pinned_resolve_urls() {
    let file = &manifest().models[0].files[0];
    assert!(file
        .url()
        .starts_with("https://huggingface.co/ibm-granite/"));
    assert!(file
        .url()
        .contains("/resolve/835ad14087e140460703cf0fae09f97d469d65c2/"));
}
