//! Icons (`KitIcon`) as data: the SF Symbol name the Swift app draws, the Lucide icon GPUI draws in
//! its place, and a text stand-in for an icon with no Lucide match. One weight (medium), one size
//! per place.
//!
//! SF Symbols may only ship in apps for Apple's systems, and this app builds for Windows and Linux
//! too, so the line icons are Lucide's (ISC; `assets/icons/LICENSE`), the closest match to each
//! symbol, vendored as only the SVGs used and embedded here.

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
    /// A turn's time, in the turn footer.
    Clock,
    /// A thinking row.
    Thinking,
    /// A file read.
    File,
    /// A web search or fetch.
    Web,
    /// A subagent.
    Agent,
    /// A task list.
    Task,
    /// Copy to the clipboard.
    Copy,
    /// A folded row's chevron.
    ChevronRight,
    /// An open row's chevron.
    ChevronDown,
    /// A file edit.
    Edit,
    /// A new file written.
    Write,
    /// A skill used.
    Skill,
    /// Any other tool (MCP, plugins).
    Plugin,
    /// A tool call that failed.
    Failed,
    /// The Latest button: jump to the newest message.
    ArrowDown,
    /// The Stop button.
    Stop,
    /// A session row's account on the main login (SF `person.crop.circle`).
    Account,
    /// A session row's account in a lane (SF `arrow.triangle.branch`).
    Lane,
    /// Codex's mark where the app's icon isn't to be had (SF `chevron.left.forwardslash.chevron.right`).
    Code,
    /// Antigravity's mark likewise (SF `paperplane`).
    Paperplane,
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
    pub const ALL: [Icon; 39] = [
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
        Icon::Clock,
        Icon::Thinking,
        Icon::File,
        Icon::Web,
        Icon::Agent,
        Icon::Task,
        Icon::Copy,
        Icon::ChevronRight,
        Icon::ChevronDown,
        Icon::Edit,
        Icon::Write,
        Icon::Skill,
        Icon::Plugin,
        Icon::Failed,
        Icon::ArrowDown,
        Icon::Stop,
        Icon::Account,
        Icon::Lane,
        Icon::Code,
        Icon::Paperplane,
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
            Icon::Clock => "clock",
            Icon::Thinking => "brain",
            Icon::File => "doc.text",
            Icon::Web => "globe",
            Icon::Agent => "person.2",
            Icon::Task => "checklist",
            Icon::Copy => "doc.on.doc",
            Icon::ChevronRight => "chevron.right",
            Icon::ChevronDown => "chevron.down",
            Icon::Edit => "pencil",
            Icon::Write => "doc.badge.plus",
            Icon::Skill => "sparkles",
            Icon::Plugin => "puzzlepiece.extension",
            // The Swift app's is filled; filled forms stay for status badges.
            Icon::Failed => "xmark.octagon",
            Icon::ArrowDown => "arrow.down",
            Icon::Stop => "stop",
            Icon::Account => "person.crop.circle",
            Icon::Lane => "arrow.triangle.branch",
            Icon::Code => "chevron.left.forwardslash.chevron.right",
            Icon::Paperplane => "paperplane",
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
            Icon::Clock => "◷",
            Icon::Thinking => "∿",
            Icon::File => "≡",
            Icon::Web => "◎",
            Icon::Agent => "⚇",
            Icon::Task => "☑",
            Icon::Copy => "⧉",
            Icon::ChevronRight => "›",
            Icon::ChevronDown => "⌄",
            Icon::Edit => "✎",
            Icon::Write => "+",
            Icon::Skill => "✧",
            Icon::Plugin => "◇",
            Icon::Failed => "✕",
            Icon::ArrowDown => "↓",
            Icon::Stop => "■",
            Icon::Account => "◉",
            Icon::Lane => "⑂",
            Icon::Code => "</>",
            Icon::Paperplane => "➤",
        }
    }

    /// The Lucide icon drawn for the symbol, as an asset path ([`asset`]); `None` keeps the glyph
    /// (Claude's mark and the phone have drawn marks of their own).
    pub fn svg(self) -> Option<&'static str> {
        Some(match self {
            Icon::Needs => "icons/circle-alert.svg",
            Icon::Working => "icons/circle-dashed.svg",
            Icon::Done => "icons/circle-check.svg",
            Icon::Terminal => "icons/square-terminal.svg",
            Icon::Tools => "icons/wrench.svg",
            Icon::Question => "icons/message-circle-question.svg",
            Icon::Search => "icons/search.svg",
            Icon::Info => "icons/info.svg",
            Icon::Check => "icons/check.svg",
            Icon::Clock => "icons/clock.svg",
            Icon::Thinking => "icons/brain.svg",
            Icon::File => "icons/file-text.svg",
            Icon::Web => "icons/globe.svg",
            Icon::Agent => "icons/bot.svg",
            Icon::Task => "icons/list-checks.svg",
            Icon::Copy => "icons/copy.svg",
            Icon::ChevronRight => "icons/chevron-right.svg",
            Icon::ChevronDown => "icons/chevron-down.svg",
            Icon::Edit => "icons/pencil.svg",
            Icon::Write => "icons/file-plus.svg",
            Icon::Skill => "icons/sparkles.svg",
            Icon::Plugin => "icons/puzzle.svg",
            Icon::Failed => "icons/octagon-x.svg",
            Icon::ArrowDown => "icons/arrow-down.svg",
            // The Swift app's stop.fill is a solid square; Lucide's is its outline.
            Icon::Stop => "icons/square.svg",
            Icon::Account => "icons/circle-user.svg",
            Icon::Lane => "icons/git-branch.svg",
            Icon::Code => "icons/code-xml.svg",
            Icon::Paperplane => "icons/send.svg",
            Icon::Resume
            | Icon::Send
            | Icon::Permission
            | Icon::Claude
            | Icon::Back
            | Icon::Sessions
            | Icon::Phone
            | Icon::More
            | Icon::Page
            | Icon::Settings => return None,
        })
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

/// A Lucide icon fills its 24-unit box less fully than an SF Symbol fills its font size (2 units
/// spare on each side), so its box is this much larger than the symbol's point size to look the
/// same size beside it.
pub const SVG_SCALE: f32 = 1.25;

/// The box a Lucide icon is drawn in to match an SF Symbol of `points`.
pub fn svg_box(points: f32) -> f32 {
    points * SVG_SCALE
}

/// `(asset path, SVG)` for each vendored icon, by its Lucide name.
macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        [$((
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../assets/icons/", $name, ".svg")) as &[u8],
        )),*]
    };
}

/// The vendored SVGs (lucide-static 0.460.0), by asset path.
const SVGS: [(&str, &[u8]); 29] = icons![
    "arrow-down",
    "brain",
    "bot",
    "check",
    "chevron-down",
    "chevron-right",
    "circle-alert",
    "circle-check",
    "circle-dashed",
    "circle-user",
    "clock",
    "code-xml",
    "copy",
    "file-plus",
    "file-text",
    "git-branch",
    "globe",
    "info",
    "list-checks",
    "message-circle-question",
    "octagon-x",
    "pencil",
    "puzzle",
    "search",
    "send",
    "sparkles",
    "square",
    "square-terminal",
    "wrench",
];

/// An embedded icon's SVG by its asset path (the GPUI asset source serves these).
pub fn asset(path: &str) -> Option<&'static [u8]> {
    SVGS.iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

/// Every embedded asset path under `prefix`.
pub fn assets(prefix: &str) -> impl Iterator<Item = &'static str> + '_ {
    SVGS.iter()
        .map(|(name, _)| *name)
        .filter(move |name| name.starts_with(prefix))
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

    #[test]
    fn every_svg_is_embedded_and_used() {
        let used: Vec<&str> = Icon::ALL.iter().filter_map(|i| i.svg()).collect();
        for path in &used {
            let bytes = asset(path).unwrap_or_else(|| panic!("{path} is not embedded"));
            let text = std::str::from_utf8(bytes).expect("utf-8");
            assert!(
                text.contains("<svg") && text.contains("viewBox=\"0 0 24 24\""),
                "{path}"
            );
            assert!(
                text.contains("lucide-static") && text.contains("ISC"),
                "{path}"
            );
        }
        for path in assets("icons/") {
            assert!(
                used.contains(&path),
                "{path} is embedded but nothing draws it"
            );
        }
        assert_eq!(assets("icons/").count(), SVGS.len());
        assert_eq!(asset("icons/none.svg"), None);
        assert_eq!(Icon::Terminal.svg(), Some("icons/square-terminal.svg"));
        assert_eq!(Icon::Claude.svg(), None, "Claude's mark is drawn");
        assert_eq!(Icon::Stop.svg(), Some("icons/square.svg"));
        assert_eq!(Icon::ArrowDown.svg(), Some("icons/arrow-down.svg"));
        assert_eq!(svg_box(12.0), 15.0);
    }
}
