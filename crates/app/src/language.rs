pub fn system_languages(
    render: bool,
    preferred: notchling_platform::Result<Vec<String>>,
) -> Vec<String> {
    if render {
        return Vec::new();
    }
    preferred.unwrap_or_default()
}

pub fn start_for_run() -> notchling_locale::Choice {
    let preferred = notchling_platform::locale::preferred();
    let list = system_languages(false, preferred);
    let region = notchling_platform::locale::region();
    notchling_locale::start(&list, region.as_deref(), None)
}

pub fn start_for_render() -> notchling_locale::Choice {
    notchling_locale::start(&[], None, None)
}

#[cfg(test)]
#[path = "language_tests.rs"]
mod language_tests;
