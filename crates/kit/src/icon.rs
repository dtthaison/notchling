//! Icons (`KitIcon`) as data: the SF Symbol name, and a text stand-in for where symbols can't be
//! drawn (GPUI has no SF Symbols). One weight (medium), one size per place.

use crate::ink::Status;
use crate::typescale::Weight;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Icon {
    Needs,
    Working,
    Done,
    Terminal,
    Resume,
    Send,
    Tools,
    Question,
    Permission,
    /// Claude's mark: a sparkle on an orange disc.
    Claude,
    Search,
    Info,
    Back,
    Sessions,
    Phone,
    Check,
    More,
    /// A saved Artifact page.
    Page,
    /// The Settings sheet.
    Settings,
}

/// Where an icon sits decides its size.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconSize {
    /// Rows and pills: 12.
    Row,
    /// Buttons: 13.
    Button,
}

impl IconSize {
    pub fn points(self) -> f32 {
        match self {
            IconSize::Row => 12.0,
            IconSize::Button => 13.0,
        }
    }
}

/// Every icon is drawn at one weight.
pub const WEIGHT: Weight = Weight::Medium;

impl Icon {
    pub const ALL: [Icon; 19] = [
        Icon::Needs,
        Icon::Working,
        Icon::Done,
        Icon::Terminal,
        Icon::Resume,
        Icon::Send,
        Icon::Tools,
        Icon::Question,
        Icon::Permission,
        Icon::Claude,
        Icon::Search,
        Icon::Info,
        Icon::Back,
        Icon::Sessions,
        Icon::Phone,
        Icon::Check,
        Icon::More,
        Icon::Page,
        Icon::Settings,
    ];

    /// The SF Symbol name (AppKit, the phone's mapping).
    pub fn symbol(self) -> &'static str {
        match self {
            Icon::Needs => "exclamationmark.bubble.fill",
            Icon::Working => "circle.dotted",
            Icon::Done => "checkmark.circle.fill",
            Icon::Terminal => "terminal",
            Icon::Resume => "arrow.clockwise",
            Icon::Send => "arrow.up",
            Icon::Tools => "wrench.and.screwdriver",
            Icon::Question => "questionmark.bubble",
            Icon::Permission => "lock.shield",
            Icon::Claude => "sparkle",
            Icon::Search => "magnifyingglass",
            Icon::Info => "info.circle",
            Icon::Back => "arrow.uturn.backward",
            Icon::Sessions => "bubble.left.and.bubble.right",
            Icon::Phone => "iphone",
            Icon::Check => "checkmark",
            Icon::More => "ellipsis",
            Icon::Page => "doc.richtext",
            Icon::Settings => "gearshape",
        }
    }

    /// A text stand-in for renderers without SF Symbols. Each one was checked drawn by the system
    /// font at 12 to 17 pt: a glyph the font lacks falls back to a thin or tiny form, so the
    /// boxier Unicode shapes (▯, ◫, ⚒, ⎘) are avoided. The phone has a drawn mark too
    /// (`ui::phone_mark`), which views prefer.
    pub fn glyph(self) -> &'static str {
        match self {
            Icon::Needs => "!",
            Icon::Working => "◌",
            Icon::Done => "✓",
            Icon::Terminal => ">_",
            Icon::Resume => "↻",
            Icon::Send => "↑",
            Icon::Tools => "✎",
            Icon::Question => "?",
            Icon::Permission => "⛨",
            Icon::Claude => "✦",
            Icon::Search => "⌕",
            Icon::Info => "i",
            Icon::Back => "↩",
            Icon::Sessions => "❏",
            Icon::Phone => "⌷",
            Icon::Check => "✓",
            Icon::More => "…",
            Icon::Page => "▤",
            Icon::Settings => "⚙",
        }
    }

    /// Filled forms are only for status badges.
    pub fn filled(self) -> bool {
        self.symbol().ends_with(".fill")
    }

    pub fn status(status: Status) -> Icon {
        match status {
            Status::Needs => Icon::Needs,
            Status::Working => Icon::Working,
            Status::Done => Icon::Done,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filled_forms_only_for_status() {
        let status: Vec<Icon> = Status::ALL.iter().map(|s| Icon::status(*s)).collect();
        for icon in Icon::ALL {
            if icon.filled() {
                assert!(status.contains(&icon), "{:?} is filled", icon);
            }
        }
        assert!(Icon::Needs.filled() && Icon::Done.filled());
    }

    #[test]
    fn every_icon_has_a_symbol_and_a_glyph() {
        for icon in Icon::ALL {
            assert!(
                !icon.symbol().is_empty() && !icon.glyph().is_empty(),
                "{:?}",
                icon
            );
        }
        assert_eq!(Icon::Claude.symbol(), "sparkle");
        assert_eq!(IconSize::Row.points(), 12.0);
        assert_eq!(IconSize::Button.points(), 13.0);
        assert_eq!(WEIGHT, Weight::Medium);
    }
}
