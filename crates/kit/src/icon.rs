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
    Claude,
    Search,
    Info,
    Back,
    Sessions,
    Phone,
    Check,
    More,
    Page,
    Settings,
    Clock,
    Thinking,
    File,
    Web,
    Agent,
    Task,
    Copy,
    ChevronRight,
    ChevronDown,
    Edit,
    Write,
    Skill,
    Plugin,
    Failed,
    ArrowDown,
    Stop,
    Account,
    Lane,
    Code,
    Paperplane,
    Folder,
    Branch,
    Cpu,
    Turns,
    Tokens,
    Gauge,
    ChevronLeft,
    Close,
    Plus,
    Minus,
    Circle,
    Fail,
    Unsure,
    Key,
    Bell,
    Handover,
    Warning,
    PanelLeft,
    PanelRight,
    Spinner,
    Delivered,
    Return,
    Paperclip,
    SlashBox,
    Halfway,
    Expand,
    Shrink,
    ZoomIn,
    ZoomOut,
    ZoomFit,
    Home,
    Camera,
    Video,
    Simulator,
    Diff,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IconSize {
    Row,
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

pub const WEIGHT: Weight = Weight::Medium;

impl Icon {
    pub const ALL: [Icon; 74] = [
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
        Icon::Folder,
        Icon::Branch,
        Icon::Cpu,
        Icon::Turns,
        Icon::Tokens,
        Icon::Gauge,
        Icon::ChevronLeft,
        Icon::Close,
        Icon::Plus,
        Icon::Minus,
        Icon::Circle,
        Icon::Fail,
        Icon::Unsure,
        Icon::Key,
        Icon::Bell,
        Icon::Handover,
        Icon::Warning,
        Icon::PanelLeft,
        Icon::PanelRight,
        Icon::Spinner,
        Icon::Delivered,
        Icon::Return,
        Icon::Paperclip,
        Icon::SlashBox,
        Icon::Halfway,
        Icon::Expand,
        Icon::Shrink,
        Icon::ZoomIn,
        Icon::ZoomOut,
        Icon::ZoomFit,
        Icon::Home,
        Icon::Camera,
        Icon::Video,
        Icon::Simulator,
        Icon::Diff,
    ];

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
            Icon::Failed => "xmark.octagon",
            Icon::ArrowDown => "arrow.down",
            Icon::Stop => "stop",
            Icon::Account => "person.crop.circle",
            Icon::Lane => "arrow.triangle.branch",
            Icon::Code => "chevron.left.forwardslash.chevron.right",
            Icon::Paperplane => "paperplane",
            Icon::Folder => "folder",
            Icon::Branch => "arrow.triangle.branch",
            Icon::Cpu => "cpu",
            Icon::Turns => "arrow.left.arrow.right",
            Icon::Tokens => "text.word.spacing",
            Icon::Gauge => "gauge",
            Icon::ChevronLeft => "chevron.left",
            Icon::Close => "xmark",
            Icon::Plus => "plus",
            Icon::Minus => "minus",
            Icon::Circle => "circle",
            Icon::Fail => "xmark.circle",
            Icon::Unsure => "questionmark.circle",
            Icon::Key => "key",
            Icon::Bell => "bell",
            Icon::Handover => "arrow.left.arrow.right",
            Icon::Warning => "exclamationmark.triangle",
            Icon::PanelLeft => "sidebar.left",
            Icon::PanelRight => "sidebar.right",
            Icon::Spinner => "progress.indicator",
            Icon::Delivered => "checkmark.circle",
            Icon::Return => "return",
            Icon::Paperclip => "paperclip",
            Icon::SlashBox => "square.slash",
            Icon::Halfway => "circle.lefthalf.filled",
            Icon::Expand => "arrow.up.left.and.arrow.down.right",
            Icon::Shrink => "arrow.down.right.and.arrow.up.left",
            Icon::ZoomIn => "plus.magnifyingglass",
            Icon::ZoomOut => "minus.magnifyingglass",
            Icon::ZoomFit => "viewfinder",
            Icon::Home => "house",
            Icon::Camera => "camera",
            Icon::Video => "video",
            Icon::Simulator => "ipad.and.iphone",
            Icon::Diff => "plusminus",
        }
    }

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
            Icon::Folder => "▭",
            Icon::Branch => "⑂",
            Icon::Cpu => "▣",
            Icon::Turns => "⇄",
            Icon::Tokens => "≣",
            Icon::Gauge => "◔",
            Icon::ChevronLeft => "‹",
            Icon::Close => "×",
            Icon::Plus => "+",
            Icon::Minus => "−",
            Icon::Circle => "○",
            Icon::Fail => "✗",
            Icon::Unsure => "?",
            Icon::Key => "⚿",
            Icon::Bell => "◍",
            Icon::Handover => "⇄",
            Icon::Warning => "⚠",
            Icon::PanelLeft => "◧",
            Icon::PanelRight => "◨",
            Icon::Spinner => "◌",
            Icon::Delivered => "✓✓",
            Icon::Return => "↵",
            Icon::Paperclip => "⌁",
            Icon::SlashBox => "/",
            Icon::Halfway => "◐",
            Icon::Expand => "⤢",
            Icon::Shrink => "⤡",
            Icon::ZoomIn => "⊕",
            Icon::ZoomOut => "⊖",
            Icon::ZoomFit => "⛶",
            Icon::Home => "⌂",
            Icon::Camera => "◉",
            Icon::Video => "▶",
            Icon::Simulator => "⌷",
            Icon::Diff => "±",
        }
    }

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
            Icon::Stop => "icons/square.svg",
            Icon::Account => "icons/circle-user.svg",
            Icon::Lane => "icons/git-branch.svg",
            Icon::Code => "icons/code-xml.svg",
            Icon::Paperplane => "icons/send.svg",
            Icon::Folder => "icons/folder.svg",
            Icon::Branch => "icons/git-branch.svg",
            Icon::Cpu => "icons/cpu.svg",
            Icon::Turns => "icons/arrow-left-right.svg",
            Icon::Tokens => "icons/text.svg",
            Icon::Gauge => "icons/gauge.svg",
            Icon::ChevronLeft => "icons/chevron-left.svg",
            Icon::Close => "icons/x.svg",
            Icon::Plus => "icons/plus.svg",
            Icon::Minus => "icons/minus.svg",
            Icon::Circle => "icons/circle.svg",
            Icon::Fail => "icons/circle-x.svg",
            Icon::Unsure => "icons/circle-help.svg",
            Icon::Key => "icons/key-round.svg",
            Icon::Bell => "icons/bell.svg",
            Icon::Handover => "icons/arrow-left-right.svg",
            Icon::Warning => "icons/triangle-alert.svg",
            Icon::PanelLeft => "icons/panel-left.svg",
            Icon::PanelRight => "icons/panel-right.svg",
            Icon::Spinner => "icons/loader-circle.svg",
            Icon::Delivered => "icons/check-check.svg",
            Icon::Return => "icons/corner-down-left.svg",
            Icon::Paperclip => "icons/paperclip.svg",
            Icon::SlashBox => "icons/square-slash.svg",
            Icon::Halfway => "icons/contrast.svg",
            Icon::Expand => "icons/maximize-2.svg",
            Icon::Shrink => "icons/minimize-2.svg",
            Icon::ZoomIn => "icons/zoom-in.svg",
            Icon::ZoomOut => "icons/zoom-out.svg",
            Icon::ZoomFit => "icons/scan-search.svg",
            Icon::Home => "icons/house.svg",
            Icon::Camera => "icons/camera.svg",
            Icon::Video => "icons/video.svg",
            Icon::Simulator => "icons/tablet-smartphone.svg",
            Icon::Diff => "icons/diff.svg",
            Icon::Resume => "icons/rotate-cw.svg",
            Icon::Send => "icons/arrow-up.svg",
            Icon::Permission => "icons/shield.svg",
            Icon::Back => "icons/undo-2.svg",
            Icon::Sessions => "icons/messages-square.svg",
            Icon::Phone => "icons/smartphone.svg",
            Icon::More => "icons/ellipsis.svg",
            Icon::Page => "icons/app-window.svg",
            Icon::Settings => "icons/settings.svg",
            Icon::Claude => return None,
        })
    }

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

pub const SVG_SCALE: f32 = 1.25;

pub fn svg_box(points: f32) -> f32 {
    points * SVG_SCALE
}

macro_rules! icons {
    ($($name:literal),* $(,)?) => {
        [$((
            concat!("icons/", $name, ".svg"),
            include_bytes!(concat!("../assets/icons/", $name, ".svg")) as &[u8],
        )),*]
    };
}

const SVGS: [(&str, &[u8]); 71] = icons![
    "app-window",
    "arrow-down",
    "arrow-left-right",
    "arrow-up",
    "bell",
    "bot",
    "brain",
    "camera",
    "check",
    "check-check",
    "chevron-down",
    "chevron-left",
    "chevron-right",
    "circle",
    "circle-alert",
    "circle-check",
    "circle-dashed",
    "circle-help",
    "circle-user",
    "circle-x",
    "clock",
    "code-xml",
    "contrast",
    "copy",
    "corner-down-left",
    "cpu",
    "diff",
    "ellipsis",
    "file-plus",
    "file-text",
    "folder",
    "gauge",
    "git-branch",
    "house",
    "globe",
    "info",
    "key-round",
    "list-checks",
    "loader-circle",
    "maximize-2",
    "message-circle-question",
    "messages-square",
    "minimize-2",
    "minus",
    "octagon-x",
    "panel-left",
    "panel-right",
    "paperclip",
    "pencil",
    "plus",
    "puzzle",
    "rotate-cw",
    "scan-search",
    "search",
    "send",
    "settings",
    "shield",
    "smartphone",
    "sparkles",
    "square",
    "square-slash",
    "square-terminal",
    "tablet-smartphone",
    "text",
    "triangle-alert",
    "undo-2",
    "video",
    "wrench",
    "x",
    "zoom-in",
    "zoom-out",
];

pub fn asset(path: &str) -> Option<&'static [u8]> {
    SVGS.iter()
        .find(|(name, _)| *name == path)
        .map(|(_, bytes)| *bytes)
}

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
        for icon in Icon::ALL {
            assert!(
                icon.svg().is_some() || icon == Icon::Claude,
                "{icon:?} has no Lucide icon"
            );
        }
        assert_eq!(Icon::Resume.svg(), Some("icons/rotate-cw.svg"));
        assert_eq!(Icon::PanelLeft.svg(), Some("icons/panel-left.svg"));
        assert_eq!(Icon::PanelRight.svg(), Some("icons/panel-right.svg"));
        assert_eq!(Icon::Spinner.svg(), Some("icons/loader-circle.svg"));
        assert_eq!(Icon::Stop.svg(), Some("icons/square.svg"));
        assert_eq!(Icon::Delivered.svg(), Some("icons/check-check.svg"));
        assert_eq!(Icon::ArrowDown.svg(), Some("icons/arrow-down.svg"));
        assert_eq!(svg_box(12.0), 15.0);
    }
}
