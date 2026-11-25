use crate::appearance::Appearance;
use crate::color::Color;
use crate::palette as p;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrgColor {
    pub name: &'static str,
    pub light: Color,
    pub dark: Color,
}

pub const ORG_COLORS: [OrgColor; 9] = [
    OrgColor {
        name: "grey",
        light: p::STONE,
        dark: p::FOG,
    },
    OrgColor {
        name: "orange",
        light: p::ORANGE500,
        dark: p::ORANGE500,
    },
    OrgColor {
        name: "yellow",
        light: p::AVATARS[4].0,
        dark: p::AVATARS[4].0,
    },
    OrgColor {
        name: "lime",
        light: p::LIME500,
        dark: p::LIME500,
    },
    OrgColor {
        name: "teal",
        light: p::AVATARS[5].1,
        dark: p::AVATARS[5].0,
    },
    OrgColor {
        name: "blue",
        light: p::BLUE600,
        dark: p::BLUE400,
    },
    OrgColor {
        name: "purple",
        light: p::AVATARS[3].1,
        dark: p::AVATARS[3].0,
    },
    OrgColor {
        name: "pink",
        light: p::PINK600,
        dark: p::PINK400,
    },
    OrgColor {
        name: "red",
        light: p::RED_LIGHT,
        dark: p::RED_DARK,
    },
];

pub fn org_color(name: &str) -> OrgColor {
    ORG_COLORS
        .into_iter()
        .find(|c| c.name == name)
        .unwrap_or(ORG_COLORS[0])
}

impl OrgColor {
    pub fn color(self, appearance: Appearance) -> Color {
        match appearance {
            Appearance::Light => self.light,
            Appearance::Dark => self.dark,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_grey_is_first() {
        let mut names: Vec<&str> = ORG_COLORS.iter().map(|c| c.name).collect();
        assert_eq!(names[0], "grey");
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), ORG_COLORS.len());
    }

    #[test]
    fn a_name_the_palette_lacks_reads_as_grey() {
        assert_eq!(org_color("mauve"), org_color("grey"));
        assert_eq!(
            org_color("blue").color(Appearance::Light).hex_string(),
            "#2C78D4"
        );
        assert_eq!(
            org_color("blue").color(Appearance::Dark).hex_string(),
            "#6EB0FF"
        );
    }

    #[test]
    fn every_colour_differs_from_the_window_in_both_looks() {
        for c in ORG_COLORS {
            assert_ne!(
                c.color(Appearance::Light),
                crate::Ink::Window.color(Appearance::Light),
                "{}",
                c.name
            );
            assert_ne!(
                c.color(Appearance::Dark),
                crate::Ink::Window.color(Appearance::Dark),
                "{}",
                c.name
            );
        }
    }
}
