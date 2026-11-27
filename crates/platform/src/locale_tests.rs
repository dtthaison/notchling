use super::{from_env, posix_tag, region_of};
use std::collections::HashMap;

fn env(pairs: &[(&str, &str)]) -> Vec<String> {
    let map: HashMap<String, String> = pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    from_env(&|key| map.get(key).cloned())
}

#[test]
fn posix_tags() {
    assert_eq!(posix_tag("de_DE.UTF-8").as_deref(), Some("de-DE"));
    assert_eq!(posix_tag("pt_BR@euro").as_deref(), Some("pt-BR"));
    assert_eq!(posix_tag("en").as_deref(), Some("en"));
    assert_eq!(posix_tag("zh_Hans_CN").as_deref(), Some("zh-Hans-CN"));
    assert_eq!(posix_tag("DE").as_deref(), Some("de"));
    assert_eq!(posix_tag("C"), None);
    assert_eq!(posix_tag("POSIX"), None);
    assert_eq!(posix_tag(""), None);
    assert_eq!(posix_tag("C.UTF-8"), None);
    assert_eq!(posix_tag("not a tag"), None);
    assert_eq!(posix_tag("de__"), None);
}

#[test]
fn language_list_order_and_duplicates() {
    let got = env(&[("LANGUAGE", "de:fr"), ("LANG", "de_DE.UTF-8")]);
    assert_eq!(got, vec!["de", "fr", "de-DE"]);
    let got = env(&[("LANG", "en_US.UTF-8"), ("LC_ALL", "fr_FR.UTF-8")]);
    assert_eq!(got, vec!["fr-FR", "en-US"]);
    assert!(env(&[("LANG", "C")]).is_empty());
    assert!(env(&[]).is_empty());
    assert_eq!(env(&[("LANGUAGE", "de:de:C")]), vec!["de"]);
}

#[test]
fn region_of_tags() {
    assert_eq!(region_of("de-AT").as_deref(), Some("AT"));
    assert_eq!(region_of("zh-Hans-CN").as_deref(), Some("CN"));
    assert_eq!(region_of("es-419").as_deref(), Some("419"));
    assert_eq!(region_of("de"), None);
    assert_eq!(region_of("zh-Hans"), None);
    assert_eq!(region_of("pt_br").as_deref(), Some("BR"));
}

#[cfg(target_os = "macos")]
#[test]
fn preferred_is_not_empty_on_a_mac() {
    let list = super::preferred().expect("a list");
    assert!(!list.is_empty());
    assert!(list.iter().all(|tag| !tag.is_empty()));
}

#[cfg(target_os = "macos")]
#[test]
fn region_is_two_letters_or_absent() {
    if let Some(region) = super::region() {
        assert!(region.len() == 2 || region.len() == 3);
    }
}
