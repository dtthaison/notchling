use super::system_languages;

fn tags(list: &[&str]) -> Vec<String> {
    list.iter().map(|t| t.to_string()).collect()
}

#[test]
fn render_ignores_system_languages() {
    assert!(system_languages(true, Ok(tags(&["de", "fr"]))).is_empty());
}

#[test]
fn error_reads_as_none() {
    let err = notchling_platform::Error::Unsupported("reading the preferred languages");
    assert!(system_languages(false, Err(err)).is_empty());
}

#[test]
fn run_keeps_order() {
    assert_eq!(
        system_languages(false, Ok(tags(&["fr-FR", "de-AT", "en"]))),
        tags(&["fr-FR", "de-AT", "en"])
    );
}
