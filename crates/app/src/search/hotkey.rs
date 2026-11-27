use super::shortcut::Shortcut;

pub trait HotKey {
    fn label(&self) -> String;

    fn register(&self, on_press: Box<dyn Fn()>) -> Result<Box<dyn Registration>, String>;
}

pub trait Registration {}

pub const NO_HOT_KEY: &str =
    "no global hot key on this system yet: only macOS registers one, through Carbon";

pub struct NoHotKey(pub Shortcut);

impl HotKey for NoHotKey {
    fn label(&self) -> String {
        self.0.label()
    }

    fn register(&self, _on_press: Box<dyn Fn()>) -> Result<Box<dyn Registration>, String> {
        Err(NO_HOT_KEY.to_owned())
    }
}

pub fn for_system(shortcut: Shortcut) -> Result<Box<dyn HotKey>, String> {
    #[cfg(target_os = "macos")]
    {
        crate::mac::hotkey::CarbonHotKey::new(shortcut).map(|k| Box::new(k) as Box<dyn HotKey>)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Ok(Box::new(NoHotKey(shortcut)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::search::shortcut::DEFAULT;

    #[test]
    fn without_an_implementation_registering_says_why() {
        let key = NoHotKey(DEFAULT);
        assert_eq!(key.label(), "⌃⌥P");
        let err = key.register(Box::new(|| {})).err();
        assert_eq!(err.as_deref(), Some(NO_HOT_KEY));
    }

    #[test]
    fn every_system_names_the_key_it_was_given() {
        let other = Shortcut::parse("alt+space").expect("a shortcut");
        for (shortcut, label) in [(DEFAULT, "⌃⌥P"), (other, "⌥Space")] {
            let key = for_system(shortcut).expect("a key for this system");
            assert_eq!(key.label(), label);
        }
    }
}
