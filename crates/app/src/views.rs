//! The views `--render` can draw, by name, with the window size each wants.

/// A renderable view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewSpec {
    pub name: &'static str,
    /// Window size in points; the capture is at the display's scale.
    pub size: (f32, f32),
    pub about: &'static str,
}

pub const ALL: [ViewSpec; 4] = [
    ViewSpec {
        name: "kit-gallery",
        size: (760.0, 900.0),
        about: "every kit part in every state, in the system's appearance",
    },
    ViewSpec {
        name: "kit-gallery-light",
        size: (760.0, 900.0),
        about: "the kit gallery, light",
    },
    ViewSpec {
        name: "kit-gallery-dark",
        size: (760.0, 900.0),
        about: "the kit gallery, dark",
    },
    ViewSpec {
        name: "shell",
        size: (1040.0, 720.0),
        about: "the main window as it opens (Chat arrives with U1)",
    },
];

pub const NAMES: [&str; 4] = [
    "kit-gallery",
    "kit-gallery-light",
    "kit-gallery-dark",
    "shell",
];

pub fn spec(name: &str) -> Option<&'static ViewSpec> {
    every().find(|v| v.name == name)
}

/// Every renderable view: these, then each part's own (the notch's).
pub fn every() -> impl Iterator<Item = &'static ViewSpec> {
    ALL.iter().chain(crate::notch::renders::VIEWS.iter())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_match_the_specs() {
        assert_eq!(ALL.map(|v| v.name), NAMES);
        for name in NAMES {
            let spec = spec(name).unwrap();
            assert!(spec.size.0 > 0.0 && spec.size.1 > 0.0);
            assert!(!spec.about.is_empty());
        }
        assert_eq!(spec("notch"), None);
    }
}
