//! The type scale (`KitType`): seven roles, one size and weight each.

/// A font weight, by the CSS number it maps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Weight {
    Regular,
    Medium,
    Semibold,
    Bold,
}

impl Weight {
    /// The CSS weight: 400, 500, 600, 700.
    pub fn css(self) -> u16 {
        match self {
            Weight::Regular => 400,
            Weight::Medium => 500,
            Weight::Semibold => 600,
            Weight::Bold => 700,
        }
    }
}

/// What a run of text is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TypeRole {
    /// Sidebar and header titles.
    Title,
    /// Session names, card titles.
    Heading,
    /// Messages.
    Body,
    /// Buttons, pills.
    Label,
    /// Status lines, hints.
    Meta,
    /// Sidebar groups: tracked, uppercase.
    Section,
    /// Code, tables, tool input.
    Mono,
}

/// How a role is set.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TypeSpec {
    /// Point size.
    pub size: f32,
    pub weight: Weight,
    /// The monospaced family instead of the system one.
    pub mono: bool,
    /// Drawn in capitals.
    pub uppercase: bool,
    /// Extra letter spacing in points (0 for most roles).
    pub tracking: f32,
}

impl TypeRole {
    pub const ALL: [TypeRole; 7] = [
        TypeRole::Title,
        TypeRole::Heading,
        TypeRole::Body,
        TypeRole::Label,
        TypeRole::Meta,
        TypeRole::Section,
        TypeRole::Mono,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TypeRole::Title => "title",
            TypeRole::Heading => "heading",
            TypeRole::Body => "body",
            TypeRole::Label => "label",
            TypeRole::Meta => "meta",
            TypeRole::Section => "section",
            TypeRole::Mono => "mono",
        }
    }

    /// Spec: the Type table.
    pub fn spec(self) -> TypeSpec {
        let plain = |size: f32, weight: Weight| TypeSpec {
            size,
            weight,
            mono: false,
            uppercase: false,
            tracking: 0.0,
        };
        match self {
            TypeRole::Title => plain(17.0, Weight::Semibold),
            TypeRole::Heading => plain(13.0, Weight::Semibold),
            TypeRole::Body => plain(13.0, Weight::Regular),
            TypeRole::Label => plain(12.0, Weight::Medium),
            TypeRole::Meta => plain(11.0, Weight::Regular),
            TypeRole::Section => TypeSpec {
                uppercase: true,
                tracking: 0.6,
                ..plain(10.5, Weight::Semibold)
            },
            TypeRole::Mono => TypeSpec {
                mono: true,
                ..plain(12.0, Weight::Regular)
            },
        }
    }

    /// The text as this role draws it (section headers are uppercase).
    pub fn display(self, text: &str) -> String {
        if self.spec().uppercase {
            text.to_uppercase()
        } else {
            text.to_owned()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_weights_follow_the_table() {
        let s = |r: TypeRole| (r.spec().size, r.spec().weight.css());
        assert_eq!(s(TypeRole::Title), (17.0, 600));
        assert_eq!(s(TypeRole::Heading), (13.0, 600));
        assert_eq!(s(TypeRole::Body), (13.0, 400));
        assert_eq!(s(TypeRole::Label), (12.0, 500));
        assert_eq!(s(TypeRole::Meta), (11.0, 400));
        assert_eq!(s(TypeRole::Section), (10.5, 600));
        assert_eq!(s(TypeRole::Mono), (12.0, 400));
    }

    #[test]
    fn only_mono_is_monospaced_and_only_section_is_tracked_uppercase() {
        for r in TypeRole::ALL {
            let spec = r.spec();
            assert_eq!(spec.mono, r == TypeRole::Mono, "{:?}", r);
            assert_eq!(spec.uppercase, r == TypeRole::Section, "{:?}", r);
            assert_eq!(spec.tracking > 0.0, r == TypeRole::Section, "{:?}", r);
        }
        assert_eq!(TypeRole::Section.display("Buttons"), "BUTTONS");
        assert_eq!(TypeRole::Body.display("Buttons"), "Buttons");
    }
}
