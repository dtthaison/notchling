use super::*;

#[test]
fn version_is_dotted_digits_when_present() {
    if let Some(version) = os_version() {
        assert!(dotted(&version), "{version}");
    }
}

#[test]
fn os_release_gives_its_version_id() {
    assert_eq!(
        parse_os_release("NAME=\"Ubuntu\"\nVERSION_ID=\"22.04\"\n"),
        Some("22.04".into())
    );
    assert_eq!(parse_os_release("VERSION_ID=12\n"), Some("12".into()));
    assert_eq!(parse_os_release("NAME=Arch\n"), None);
    assert_eq!(parse_os_release("VERSION_ID=\"rolling\"\n"), None);
}

#[test]
fn dotted_refuses_anything_else() {
    assert!(dotted("15.1"));
    assert!(!dotted(""));
    assert!(!dotted("15..1"));
    assert!(!dotted("15.1-beta"));
}
