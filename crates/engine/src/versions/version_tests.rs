use super::{Tool, Version};

#[test]
fn accepts_plain_and_prerelease() {
    assert_eq!(Version::parse("2.1.324").unwrap().as_str(), "2.1.324");
    assert_eq!(
        Version::parse("1.3.0-beta.2").unwrap().as_str(),
        "1.3.0-beta.2"
    );
    assert_eq!(Version::parse("2.1.324").unwrap().to_string(), "2.1.324");
}

#[test]
fn rejects_long_or_odd() {
    let long = format!("1.2.{}", "3".repeat(29));
    assert_eq!(long.len(), 33);
    for bad in [
        long.as_str(),
        "2.1",
        "v2.1.3",
        "2.1.3;rm",
        "2.1.3 ",
        " 2.1.3",
        "stable",
        "latest",
        "",
        "1.2.3-",
        "1.2.3-a-b",
        "1.2.3.4",
        "1..3",
        "2.1.3\n",
        "2.1.3\u{2028}",
    ] {
        assert!(Version::parse(bad).is_none(), "{bad:?}");
    }
    let edge = format!("1.2.{}", "3".repeat(28));
    assert_eq!(edge.len(), 32);
    assert!(Version::parse(&edge).is_some());
}

#[test]
fn orders_numerically() {
    let a = Version::parse("2.1.89").unwrap();
    let b = Version::parse("2.1.322").unwrap();
    assert!(a < b);
    assert!(Version::parse("2.10.0").unwrap() > Version::parse("2.9.9").unwrap());
    assert!(Version::parse("10.0.0").unwrap() > Version::parse("9.99.99").unwrap());
}

#[test]
fn prerelease_is_below_release() {
    let beta = Version::parse("1.3.0-beta.2").unwrap();
    let release = Version::parse("1.3.0").unwrap();
    assert!(beta < release);
    assert!(Version::parse("1.3.0-beta.2").unwrap() < Version::parse("1.3.0-beta.10").unwrap());
    assert!(Version::parse("1.3.0-alpha").unwrap() < Version::parse("1.3.0-beta").unwrap());
    assert!(Version::parse("1.2.9").unwrap() < beta);
}

#[test]
fn huge_numbers_compare_without_overflow() {
    let big = Version::parse("99999999999999999999999.0.0").unwrap();
    let small = Version::parse("1.0.0").unwrap();
    assert!(big > small);
}

#[test]
fn deserializing_checks_the_pattern() {
    let ok: Version = serde_json::from_str("\"2.1.3\"").unwrap();
    assert_eq!(ok.as_str(), "2.1.3");
    assert!(serde_json::from_str::<Version>("\"2.1.3;rm\"").is_err());
    assert_eq!(serde_json::to_string(&ok).unwrap(), "\"2.1.3\"");
}

#[test]
fn tool_names_are_stable_and_agy_is_read() {
    assert_eq!(
        serde_json::to_string(&Tool::Agy).unwrap(),
        "\"antigravity\""
    );
    assert_eq!(serde_json::to_string(&Tool::Claude).unwrap(), "\"claude\"");
    assert_eq!(serde_json::from_str::<Tool>("\"agy\"").unwrap(), Tool::Agy);
    assert_eq!(
        serde_json::from_str::<Tool>("\"antigravity\"").unwrap(),
        Tool::Agy
    );
    assert_eq!(Tool::from_name("agy"), Some(Tool::Agy));
    assert_eq!(Tool::from_name("antigravity"), Some(Tool::Agy));
    assert_eq!(Tool::from_name("gemini"), None);
    assert_eq!(Tool::Agy.command(), "agy");
    assert_eq!(Tool::Agy.name(), "antigravity");
    assert_eq!(Tool::Agy.title(), "Antigravity");
    assert_eq!(Tool::ALL.len(), 4);
}

#[test]
fn tools_are_map_keys() {
    let mut map = std::collections::BTreeMap::new();
    map.insert(Tool::Agy, 1);
    map.insert(Tool::Claude, 2);
    let text = serde_json::to_string(&map).unwrap();
    assert_eq!(text, "{\"claude\":2,\"antigravity\":1}");
    let back: std::collections::BTreeMap<Tool, i32> =
        serde_json::from_str("{\"agy\":1,\"claude\":2}").unwrap();
    assert_eq!(back, map);
}
