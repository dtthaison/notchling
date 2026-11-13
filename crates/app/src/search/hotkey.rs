pub trait HotKey {
    fn label(&self) -> &'static str;

    fn register(&self, on_press: Box<dyn Fn()>) -> Result<Box<dyn Registration>, String>;
}

pub trait Registration {}

pub const NO_HOT_KEY: &str =
    "no global hot key on this system yet: only macOS registers one, through Carbon";

pub struct NoHotKey;

impl HotKey for NoHotKey {
    fn label(&self) -> &'static str {
        "⌘⇧P"
    }

    fn register(&self, _on_press: Box<dyn Fn()>) -> Result<Box<dyn Registration>, String> {
        Err(NO_HOT_KEY.to_owned())
    }
}

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
        assert_eq!(for_system().label(), "⌘⇧P");
    }
}
