use super::bundle::{is_antigravity_app_executable, BUNDLE_ID};
use std::path::{Path, PathBuf};

fn bundle(name: &str, folder: &str, id: &str, plist: Option<&str>) -> PathBuf {
    let root = std::env::temp_dir().join(format!("nl-bundle-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let app = root.join(folder);
    std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
    std::fs::create_dir_all(app.join("Contents/Resources/bin")).unwrap();
    let text = plist.map_or_else(
        || {
            format!(
                "<?xml version=\"1.0\"?>\n<plist version=\"1.0\"><dict>\n\t<key>CFBundleExecutable</key>\n\t<string>Antigravity</string>\n\t<key>CFBundleIdentifier</key>\n\t<string>{id}</string>\n</dict></plist>\n"
            )
        },
        str::to_string,
    );
    std::fs::write(app.join("Contents/Info.plist"), text).unwrap();
    app
}

fn main_exe(app: &Path) -> PathBuf {
    app.join("Contents/MacOS/Antigravity")
}

#[test]
fn the_programs_of_the_real_app_count_wherever_the_bundle_sits() {
    for folder in ["Antigravity.app", "Elsewhere/Renamed.app"] {
        let app = bundle("real", folder, BUNDLE_ID, None);
        assert!(is_antigravity_app_executable(&main_exe(&app)), "{folder}");
        assert!(is_antigravity_app_executable(
            &app.join("Contents/Resources/bin/language_server")
        ));
    }
}

#[test]
fn a_path_that_only_says_antigravity_is_not_enough() {
    let app = bundle(
        "ide",
        "Antigravity IDE.app",
        "com.google.antigravity-ide",
        None,
    );
    assert!(!is_antigravity_app_executable(&main_exe(&app)));
    let app = bundle("other", "Antigravity.app", "com.example.fake", None);
    assert!(!is_antigravity_app_executable(&main_exe(&app)));
    let plain = std::env::temp_dir().join("antigravity/bin/tool");
    assert!(!is_antigravity_app_executable(&plain));
}

#[test]
fn a_program_outside_the_bundle_contents_or_an_unreadable_plist_does_not_count() {
    let app = bundle("outside", "Antigravity.app", BUNDLE_ID, None);
    assert!(!is_antigravity_app_executable(
        &app.parent().unwrap().join("evil")
    ));
    assert!(!is_antigravity_app_executable(&app.join("evil")));
    let app = bundle(
        "binary",
        "Antigravity.app",
        BUNDLE_ID,
        Some("bplist00\u{1}\u{2}"),
    );
    assert!(!is_antigravity_app_executable(&main_exe(&app)));
    let app = bundle(
        "other-keys",
        "Antigravity.app",
        BUNDLE_ID,
        Some("<dict><key>Other</key><string>x</string><key>CFBundleIdentifier</key>\n<string>com.google.antigravity</string></dict>"),
    );
    assert!(is_antigravity_app_executable(&main_exe(&app)));
}
