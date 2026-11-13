use gpui::App;
use notchling_kit::{set_preference, Preference};
use objc2::MainThreadMarker;
use objc2_app_kit::{NSAppearance, NSAppearanceNameAqua, NSAppearanceNameDarkAqua, NSApplication};
use serde_json::Value;

pub fn set_native(preference: Preference) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let name = match preference {
        Preference::System => None,
        Preference::Light => Some(unsafe { NSAppearanceNameAqua }),
        Preference::Dark => Some(unsafe { NSAppearanceNameDarkAqua }),
    };
    let appearance = name.and_then(NSAppearance::appearanceNamed);
    app.setAppearance(appearance.as_deref());
}

pub fn on_change(key: &str, value: &Value, cx: &App) {
    let Some(chosen) = crate::appearance::changed_to(key, value) else {
        return;
    };
    set_preference(chosen);
    cx.spawn(async move |_| set_native(chosen)).detach();
}
