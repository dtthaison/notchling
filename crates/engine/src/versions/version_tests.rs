use super::Version;

fn v(s: &str) -> Version {
    Version::parse(s).expect("a version")
}

#[test]
fn accepts_plain_and_prerelease() {
    assert_eq!(v("2.1.324").as_str(), "2.1.324");
    assert_eq!(v("1.3.0-beta.2").to_string(), "1.3.0-beta.2");
}

#[test]
fn rejects_long_or_odd() {
    let long = format!("1.1.{}", "9".repeat(30));
    for bad in [
        long.as_str(),
        "2.1",
        "v2.1.3",
        "2.1.3;rm",
        "2.1.3 ",
        "stable",
        "latest",
        "2.1.3-",
        "2..3",
        "",
    ] {
        assert!(Version::parse(bad).is_none(), "{bad}");
    }
}

#[test]
fn orders_numerically() {
    assert!(v("2.1.89") < v("2.1.322"));
    assert!(v("2.10.0") > v("2.9.9"));
}

#[test]
fn prerelease_is_below_release() {
    assert!(v("1.3.0-beta.2") < v("1.3.0"));
    assert!(v("1.3.0-beta.1") < v("1.3.0-beta.2"));
}

#[test]
fn serde_refuses_a_bad_string() {
    assert!(serde_json::from_str::<Version>("\"2.1.3;rm\"").is_err());
    assert_eq!(
        serde_json::to_string(&v("2.1.3")).expect("json"),
        "\"2.1.3\""
    );
}
