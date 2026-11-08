//! Layer 2: colors by meaning (`KitInk`), resolved for light or dark.

use crate::appearance::Appearance;
use crate::color::Color;
use crate::palette as p;

/// A session's status, as the kit colors it (`AgentGroup` in the Swift app).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Status {
    /// Waiting on the user: a question, a permission.
    Needs,
    Working,
    Done,
}

impl Status {
    pub const ALL: [Status; 3] = [Status::Needs, Status::Working, Status::Done];

    /// The word shown beside the icon; status is never color alone.
    pub fn word(self) -> &'static str {
        match self {
            Status::Needs => "Needs input",
            Status::Working => "Working",
            Status::Done => "Done",
        }
    }
}

/// What a color is for. Views name the meaning; `color` picks the shade for the appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Ink {
    /// Conversation background.
    Window,
    /// Sidebar background.
    Sidebar,
    /// Claude's messages, cards, the composer.
    Card,
    /// Hover, chips, code.
    Raised,
    /// The selected session.
    Selection,
    /// Borders and dividers.
    Hairline,
    /// Body text.
    Text,
    /// Secondary lines.
    TextMuted,
    /// Hints and ages.
    TextFaint,
    /// Send, primary buttons, focus.
    Accent,
    /// Orange words.
    AccentText,
    /// Text on an accent fill: dark, because white fails on orange.
    OnAccent,
    /// Your messages.
    AccentWash,
    Needs,
    Working,
    Done,
    Shadow,
    /// A `!` shell command: composer border and header, the run button.
    Shell,
    /// The composer in shell mode, sent shell commands.
    ShellWash,
    /// The icon on the run button.
    OnShell,
    /// Part labels (the ? button): blue, a guide over the UI, not part of it.
    Guide,
    /// Text on a part label.
    OnGuide,
    /// A secret past its last day.
    Expired,
    /// The terminal sheet is dark in both appearances: Claude Code picks colors for a dark terminal.
    Terminal,
    TerminalText,
}

impl Ink {
    pub const ALL: [Ink; 25] = [
        Ink::Window,
        Ink::Sidebar,
        Ink::Card,
        Ink::Raised,
        Ink::Selection,
        Ink::Hairline,
        Ink::Text,
        Ink::TextMuted,
        Ink::TextFaint,
        Ink::Accent,
        Ink::AccentText,
        Ink::OnAccent,
        Ink::AccentWash,
        Ink::Needs,
        Ink::Working,
        Ink::Done,
        Ink::Shadow,
        Ink::Shell,
        Ink::ShellWash,
        Ink::OnShell,
        Ink::Guide,
        Ink::OnGuide,
        Ink::Expired,
        Ink::Terminal,
        Ink::TerminalText,
    ];

    /// The token's name as the spec writes it.
    pub fn name(self) -> &'static str {
        match self {
            Ink::Window => "window",
            Ink::Sidebar => "sidebar",
            Ink::Card => "card",
            Ink::Raised => "raised",
            Ink::Selection => "selection",
            Ink::Hairline => "hairline",
            Ink::Text => "text",
            Ink::TextMuted => "textMuted",
            Ink::TextFaint => "textFaint",
            Ink::Accent => "accent",
            Ink::AccentText => "accentText",
            Ink::OnAccent => "onAccent",
            Ink::AccentWash => "accentWash",
            Ink::Needs => "needs",
            Ink::Working => "working",
            Ink::Done => "done",
            Ink::Shadow => "shadow",
            Ink::Shell => "shell",
            Ink::ShellWash => "shellWash",
            Ink::OnShell => "onShell",
            Ink::Guide => "guide",
            Ink::OnGuide => "onGuide",
            Ink::Expired => "expired",
            Ink::Terminal => "terminal",
            Ink::TerminalText => "terminalText",
        }
    }

    /// The color for the appearance (spec: Meaning table, light / dark).
    pub fn color(self, appearance: Appearance) -> Color {
        let light = appearance == Appearance::Light;
        let pick = |l: Color, d: Color| if light { l } else { d };
        match self {
            Ink::Window => pick(p::PAPER2, p::NIGHT2),
            Ink::Sidebar => pick(p::PAPER1, p::NIGHT1),
            Ink::Card => pick(p::PAPER0, p::NIGHT3),
            Ink::Raised => pick(p::PAPER4, p::NIGHT4),
            Ink::Selection => pick(p::ORANGE500.with_alpha(0.12), p::ORANGE500.with_alpha(0.16)),
            Ink::Hairline => pick(p::PAPER5, p::NIGHT5),
            Ink::Text => pick(p::SOOT, p::SNOW),
            Ink::TextMuted => pick(p::STONE, p::FOG),
            Ink::TextFaint => pick(p::PEBBLE, p::SMOKE),
            Ink::Accent => p::ORANGE500,
            Ink::AccentText => pick(p::ORANGE700, p::ORANGE300),
            Ink::OnAccent => pick(p::SOOT, p::NIGHT0),
            Ink::AccentWash => pick(p::ORANGE500.with_alpha(0.14), p::ORANGE500.with_alpha(0.18)),
            Ink::Needs => p::ORANGE500,
            Ink::Working => pick(p::BLUE600, p::BLUE400),
            Ink::Done => p::LIME500,
            Ink::Shadow => pick(Color::BLACK.with_alpha(0.08), Color::BLACK.with_alpha(0.45)),
            Ink::Shell => pick(p::PINK600, p::PINK400),
            Ink::ShellWash => pick(p::PINK600.with_alpha(0.10), p::PINK400.with_alpha(0.14)),
            Ink::OnShell => pick(p::PAPER0, p::NIGHT0),
            Ink::Guide => pick(p::BLUE600, p::BLUE400),
            Ink::OnGuide => pick(p::PAPER0, p::NIGHT0),
            Ink::Expired => pick(p::RED_LIGHT, p::RED_DARK),
            Ink::Terminal => p::NIGHT0,
            Ink::TerminalText => p::SNOW,
        }
    }

    /// The ink for a status.
    pub fn status(status: Status) -> Ink {
        match status {
            Status::Needs => Ink::Needs,
            Status::Working => Ink::Working,
            Status::Done => Ink::Done,
        }
    }
}

/// The top of Claude's mark: a gradient from orange300 down to the accent.
pub fn claude_mark_top() -> Color {
    p::ORANGE300
}

/// A session keeps its colors: a gradient pair picked from its name by FNV-1a, the same hash and the
/// same bucket the Swift app uses, so a session looks the same in both apps.
pub fn avatar(name: &str) -> (Color, Color) {
    let mut hash: u64 = 1_469_598_103_934_665_603;
    for byte in name.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(1_099_511_628_211);
    }
    // The low bits of FNV-1a spread poorly over a few buckets; use the high ones.
    let index = (hash >> 40) % p::AVATARS.len() as u64;
    p::AVATARS[index as usize]
}

#[cfg(test)]
mod tests {
    use super::*;
    use Appearance::{Dark, Light};

    fn hex(ink: Ink, a: Appearance) -> String {
        ink.color(a).hex_string()
    }

    #[test]
    fn surfaces_follow_the_meaning_table() {
        assert_eq!(hex(Ink::Window, Light), "#FAF8F5");
        assert_eq!(hex(Ink::Window, Dark), "#1B1917");
        assert_eq!(hex(Ink::Sidebar, Light), "#F4F0EA");
        assert_eq!(hex(Ink::Sidebar, Dark), "#151311");
        assert_eq!(hex(Ink::Card, Light), "#FFFFFF");
        assert_eq!(hex(Ink::Card, Dark), "#242120");
        assert_eq!(hex(Ink::Raised, Dark), "#2F2B28");
        assert_eq!(hex(Ink::Hairline, Light), "#DED5C9");
        assert_eq!(hex(Ink::Hairline, Dark), "#3B3632");
    }

    #[test]
    fn text_and_accent_follow_the_table() {
        assert_eq!(hex(Ink::Text, Light), "#1F1B17");
        assert_eq!(hex(Ink::Text, Dark), "#F3EEE8");
        assert_eq!(hex(Ink::TextMuted, Dark), "#A69D93");
        assert_eq!(hex(Ink::TextFaint, Light), "#9B9187");
        assert_eq!(hex(Ink::Accent, Light), hex(Ink::Accent, Dark));
        assert_eq!(hex(Ink::Accent, Dark), "#F7873B");
        assert_eq!(hex(Ink::AccentText, Light), "#B4540E");
        assert_eq!(hex(Ink::AccentText, Dark), "#FFB47E");
        // Dark text on the orange fill: white fails on orange.
        assert_eq!(hex(Ink::OnAccent, Light), "#1F1B17");
        assert_eq!(hex(Ink::OnAccent, Dark), "#0F0E0D");
    }

    #[test]
    fn washes_carry_the_spec_percentages() {
        assert_eq!(Ink::Selection.color(Light).a, 0.12);
        assert_eq!(Ink::Selection.color(Dark).a, 0.16);
        assert_eq!(Ink::AccentWash.color(Light).a, 0.14);
        assert_eq!(Ink::AccentWash.color(Dark).a, 0.18);
        assert_eq!(Ink::ShellWash.color(Light).a, 0.10);
        assert_eq!(Ink::ShellWash.color(Dark).a, 0.14);
        assert_eq!(Ink::Shadow.color(Light).a, 0.08);
        assert_eq!(Ink::Shadow.color(Dark).a, 0.45);
    }

    #[test]
    fn status_and_shell_colors() {
        assert_eq!(hex(Ink::Needs, Dark), "#F7873B");
        assert_eq!(hex(Ink::Working, Light), "#2C78D4");
        assert_eq!(hex(Ink::Working, Dark), "#6EB0FF");
        assert_eq!(hex(Ink::Done, Light), "#7DCB2C");
        assert_eq!(hex(Ink::Shell, Light), "#C2307A");
        assert_eq!(hex(Ink::Shell, Dark), "#FF7AB8");
        assert_eq!(hex(Ink::Guide, Dark), "#6EB0FF");
        assert_eq!(Ink::status(Status::Working), Ink::Working);
    }

    #[test]
    fn terminal_is_dark_in_both_appearances() {
        assert_eq!(hex(Ink::Terminal, Light), "#0F0E0D");
        assert_eq!(hex(Ink::Terminal, Dark), "#0F0E0D");
        assert_eq!(hex(Ink::TerminalText, Light), "#F3EEE8");
    }

    #[test]
    fn body_text_reads_on_its_surfaces() {
        for a in Appearance::ALL {
            for surface in [Ink::Window, Ink::Sidebar, Ink::Card, Ink::Raised] {
                let ratio = Ink::Text.color(a).contrast(surface.color(a));
                assert!(ratio >= 7.0, "{:?} text on {:?}: {ratio}", a, surface);
            }
            assert!(Ink::OnAccent.color(a).contrast(Ink::Accent.color(a)) >= 4.5);
        }
    }

    #[test]
    fn every_ink_has_a_unique_name() {
        let mut names: Vec<&str> = Ink::ALL.iter().map(|i| i.name()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), Ink::ALL.len());
    }

    #[test]
    fn avatar_buckets_match_the_swift_hash() {
        // Computed with the Swift app's FNV-1a and `hash >> 40 % 8`.
        assert_eq!(avatar("design system"), p::AVATARS[0]);
        assert_eq!(avatar("wifi"), p::AVATARS[4]);
        assert_eq!(avatar(""), p::AVATARS[7]);
        assert_eq!(avatar("design system"), avatar("design system"));
    }
}
