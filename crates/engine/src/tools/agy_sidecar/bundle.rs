use std::io::Read;
use std::path::Path;

pub const BUNDLE_ID: &str = "com.google.antigravity";
const MAX_PLIST: u64 = 256 * 1024;

fn plist_string(text: &str, key: &str) -> Option<String> {
    let marker = format!("<key>{key}</key>");
    let after = &text[text.find(&marker)? + marker.len()..];
    let after = after.trim_start().strip_prefix("<string>")?;
    Some(after[..after.find("</string>")?].trim().to_string())
}

fn bundle_id(app: &Path) -> Option<String> {
    let mut text = String::new();
    std::fs::File::open(app.join("Contents/Info.plist"))
        .ok()?
        .take(MAX_PLIST)
        .read_to_string(&mut text)
        .ok()?;
    plist_string(&text, "CFBundleIdentifier")
}

pub fn is_antigravity_app_executable(exe: &Path) -> bool {
    exe.ancestors().skip(1).any(|dir| {
        dir.extension().is_some_and(|e| e == "app")
            && exe.starts_with(dir.join("Contents"))
            && bundle_id(dir).is_some_and(|id| id == BUNDLE_ID)
    })
}
