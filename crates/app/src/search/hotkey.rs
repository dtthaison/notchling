//! The global shortcut that opens the search panel, behind a trait. macOS registers ⌘⇧P through
//! Carbon's hot key API (`mac::hotkey`), which needs no Accessibility permission and sees no other
//! key press. Linux and Windows have no implementation yet: [`NoHotKey`] says so, and the panel
//! opens only from the app there.

/// A system-wide key.
pub trait HotKey {
    /// The key as the Settings sheet names it, such as "⌘⇧P".
    fn label(&self) -> &'static str;

    /// Starts calling `on_press` on each press, on the UI thread, until the registration drops.
    /// `Err` says why not: another app holds the key, or this system has no implementation.
    fn register(&self, on_press: Box<dyn Fn()>) -> Result<Box<dyn Registration>, String>;
}

/// A registered key; dropping it unregisters.
pub trait Registration {}

/// What [`NoHotKey::register`] says.
pub const NO_HOT_KEY: &str =
    "no global hot key on this system yet: only macOS registers one, through Carbon";

/// Linux and Windows: no shortcut yet.
pub struct NoHotKey;

impl HotKey for NoHotKey {
    fn label(&self) -> &'static str {
        "⌘⇧P"
    }

    fn register(&self, _on_press: Box<dyn Fn()>) -> Result<Box<dyn Registration>, String> {
        Err(NO_HOT_KEY.to_owned())
    }
}

/// The hot key this system has: Carbon's ⌘⇧P on macOS, none elsewhere.
pub fn for_system() -> Box<dyn HotKey> {
    #[cfg(target_os = "macos")]
    {
        Box::new(crate::mac::hotkey::CarbonHotKey::command_shift_p())
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(NoHotKey)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn without_an_implementation_registering_says_why() {
        let key = NoHotKey;
        assert_eq!(key.label(), "⌘⇧P");
        let err = key.register(Box::new(|| {})).err();
        assert_eq!(err.as_deref(), Some(NO_HOT_KEY));
    }

    #[test]
    fn every_system_names_the_same_key() {
        // Only the label is read: registering would need a window server and the real shortcut.
        assert_eq!(for_system().label(), "⌘⇧P");
    }
}
