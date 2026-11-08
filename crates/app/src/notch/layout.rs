//! The notch's geometry, pure: the black shape's size per state (`NotchModel.size(for:)`), where the
//! camera housing is, the transparent canvas around it and the hit test that decides whether the
//! canvas takes the mouse (`NotchController`). Coordinates are AppKit's: points, y grows upward from
//! the bottom of the screen.

/// What the notch shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotchState {
    /// Exactly the camera housing.
    Collapsed,
    /// An agent just replied or asked something; shown briefly.
    Peek(String),
    /// The three tabs and their rows.
    List,
    /// One agent's conversation.
    Detail(String),
}

/// How wide the notch opens (the Swift app's menu setting; Medium by default).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotchWidth {
    Small,
    #[default]
    Medium,
    Large,
}

impl NotchWidth {
    /// The agent list and short peeks.
    pub fn list(self) -> f64 {
        match self {
            NotchWidth::Small => 300.0,
            NotchWidth::Medium => 340.0,
            NotchWidth::Large => 400.0,
        }
    }

    /// The conversation and the question and permission cards, which carry more text.
    pub fn panel(self) -> f64 {
        match self {
            NotchWidth::Small => 360.0,
            NotchWidth::Medium => 420.0,
            NotchWidth::Large => 520.0,
        }
    }
}

/// A width and a height in points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub w: f64,
    pub h: f64,
}

impl Size {
    pub const fn new(w: f64, h: f64) -> Size {
        Size { w, h }
    }
}

/// The outward curves where the shape meets the top of the screen.
pub const FLARE: f64 = 8.0;
/// The tallest a peek's card opens; taller cards are cut.
pub const MAX_CARD: f64 = 520.0;
/// The tab row under the camera.
pub const TAB_BAR: f64 = 34.0;
/// The tallest the list's rows get before they scroll.
pub const MAX_LIST: f64 = 300.0;
/// The camera housing of a 14" MacBook Pro, the model's default before a screen is read.
pub const DEFAULT_NOTCH: Size = Size::new(185.0, 32.0);
/// The least the canvas covers, enough for the largest state.
pub const CANVAS: Size = Size::new(560.0, 640.0);
/// Slack below the collapsed strip, so it's easier to hit.
pub const COLLAPSED_SLACK: f64 = 4.0;

/// What a peek holds, which decides its size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PeekContent {
    /// A message of Notchling's own; `long` past 70 characters.
    Notice { long: bool },
    /// A question or permission card, by its estimated height.
    Card { height: f64 },
    /// The agent's name and, when `has_text`, its last reply.
    Plain { has_text: bool },
}

/// The notch's own height plus the flare above and below the content.
fn chrome(notch: Size) -> f64 {
    notch.h + 2.0 * FLARE
}

/// Exactly the camera housing; the pet and its count live outside it.
pub fn collapsed_size(notch: Size) -> Size {
    Size::new(notch.w + 2.0 * FLARE, notch.h)
}

/// A peek. A card's measured height (`card_height`) beats its estimate; either is capped.
pub fn peek_size(
    notch: Size,
    width: NotchWidth,
    content: PeekContent,
    card_height: Option<f64>,
) -> Size {
    let chrome = chrome(notch);
    match content {
        PeekContent::Notice { long } => {
            Size::new(width.list(), chrome + if long { 78.0 } else { 58.0 })
        }
        PeekContent::Card { height } => Size::new(
            width.panel(),
            chrome + card_height.unwrap_or(height).min(MAX_CARD) + 16.0,
        ),
        PeekContent::Plain { has_text } => {
            Size::new(width.list(), chrome + if has_text { 100.0 } else { 56.0 })
        }
    }
}

/// The open list: the tab row, then the rows of the tab on show (each `row_height` plus 4 between),
/// a "Clear all" line under the Done tab, capped at [`MAX_LIST`].
pub fn list_size(notch: Size, width: NotchWidth, rows: &[f64], done_tab: bool) -> Size {
    let content = if rows.is_empty() {
        44.0
    } else {
        let start = if done_tab { 28.0 } else { 0.0 };
        rows.iter().fold(start, |sum, h| sum + h + 4.0)
    };
    Size::new(
        width.list(),
        chrome(notch) + TAB_BAR + content.min(MAX_LIST) + 6.0,
    )
}

/// One agent's conversation, taller while a question or permission card is on show.
pub fn detail_size(notch: Size, width: NotchWidth, has_card: bool) -> Size {
    Size::new(
        width.panel(),
        chrome(notch) + if has_card { 470.0 } else { 420.0 },
    )
}

/// A row: one line for a finished agent or one without a prompt yet, two otherwise.
pub fn row_height(completed: bool, has_prompt: bool) -> f64 {
    if completed || !has_prompt {
        44.0
    } else {
        58.0
    }
}

/// A question card's estimate: header, up to three lines of question, an option row each, the
/// answer line.
pub fn question_height(question_chars: usize, options: usize) -> f64 {
    let lines = (question_chars / 60 + 1).min(3) as f64;
    28.0 + lines * 17.0 + options as f64 * 36.0 + 40.0 + 8.0
}

/// A permission card's estimate: header, the tool line, up to four lines of summary, two or three
/// options, the answer line.
pub fn permission_height(summary_chars: usize, can_always_allow: bool) -> f64 {
    let lines = (summary_chars / 62 + 1).min(4) as f64;
    let options = if can_always_allow { 3.0 } else { 2.0 };
    28.0 + 20.0 + lines * 16.0 + 14.0 + options * 36.0 + 42.0 + 28.0
}

/// A rectangle in AppKit screen coordinates (y up).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Rect {
    pub const fn new(x: f64, y: f64, w: f64, h: f64) -> Rect {
        Rect { x, y, w, h }
    }

    pub fn max_x(&self) -> f64 {
        self.x + self.w
    }

    pub fn max_y(&self) -> f64 {
        self.y + self.h
    }

    pub fn mid_x(&self) -> f64 {
        self.x + self.w / 2.0
    }
}

/// What AppKit says about a display.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenInfo {
    pub frame: Rect,
    /// The top of the area windows may use (below the menu bar).
    pub visible_max_y: f64,
    /// `safeAreaInsets.top`: the camera housing's height, 0 without one.
    pub safe_top: f64,
    /// The menu bar areas left and right of the camera; `None` without a notch.
    pub aux_left_width: Option<f64>,
    pub aux_right_width: Option<f64>,
}

/// Where the shape hangs from: the housing's size and the horizontal centre of the camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub screen: Rect,
    pub notch: Size,
    pub center_x: f64,
}

/// The camera housing: the frame's width minus the two menu bar areas beside it, at the top of the
/// frame. `None` on a display without one.
pub fn notch_rect(s: &ScreenInfo) -> Option<Rect> {
    let (left, right) = (s.aux_left_width?, s.aux_right_width?);
    let w = s.frame.w - left - right;
    if w <= 0.0 || s.safe_top <= 0.0 {
        return None;
    }
    Some(Rect::new(
        s.frame.x + left,
        s.frame.max_y() - s.safe_top,
        w,
        s.safe_top,
    ))
}

/// The geometry for a display (`screensChanged`): the camera housing when there is one, else a
/// zero-width strip as tall as the menu bar (at least 24) in the middle of the screen.
pub fn geometry(s: &ScreenInfo) -> Geometry {
    match notch_rect(s) {
        Some(n) => Geometry {
            screen: s.frame,
            notch: Size::new(n.w, n.h),
            center_x: n.mid_x(),
        },
        None => Geometry {
            screen: s.frame,
            notch: Size::new(0.0, (s.frame.max_y() - s.visible_max_y).max(24.0)),
            center_x: s.frame.mid_x(),
        },
    }
}

/// The transparent canvas: as wide as the screen on both sides of the camera (so the pet can roam
/// the whole bar), at least [`CANVAS`], centred on the camera and hanging from the top of the screen.
/// It changes only with the display; the black shape inside it is what animates.
pub fn canvas_frame(g: &Geometry) -> Rect {
    let span = (g.center_x - g.screen.x).max(g.screen.max_x() - g.center_x);
    let w = CANVAS.w.max(2.0 * span);
    Rect::new(
        g.center_x - w / 2.0,
        g.screen.max_y() - CANVAS.h,
        w,
        CANVAS.h,
    )
}

/// The black shape's area on screen for a size, with a little slack below the collapsed strip.
pub fn shape_rect(g: &Geometry, size: Size, collapsed: bool) -> Rect {
    let extra = if collapsed { COLLAPSED_SLACK } else { 0.0 };
    Rect::new(
        g.center_x - size.w / 2.0,
        g.screen.max_y() - size.h - extra,
        size.w,
        size.h + extra,
    )
}

/// Whether the mouse is over the shape. Half-open on the right, so a boundary point belongs to one
/// side only; the top edge is inside, since a pointer pushed to the top of the screen reports exactly
/// that y and `NSMouseInRect` counts it in.
pub fn hit_test(shape: Rect, mouse: (f64, f64)) -> bool {
    let (mx, my) = mouse;
    mx >= shape.x && mx < shape.max_x() && my >= shape.y && my <= shape.max_y()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTCH: Size = DEFAULT_NOTCH;
    /// 32 + 2 × 8.
    const CHROME: f64 = 48.0;

    #[test]
    fn collapsed_is_the_housing_plus_the_flares() {
        assert_eq!(collapsed_size(NOTCH), Size::new(201.0, 32.0));
        // No notch: a zero-width strip still gets its flares.
        assert_eq!(collapsed_size(Size::new(0.0, 24.0)), Size::new(16.0, 24.0));
    }

    #[test]
    fn widths_follow_the_setting() {
        assert_eq!(
            [NotchWidth::Small, NotchWidth::Medium, NotchWidth::Large]
                .map(|w| (w.list(), w.panel())),
            [(300.0, 360.0), (340.0, 420.0), (400.0, 520.0)]
        );
        assert_eq!(NotchWidth::default(), NotchWidth::Medium);
    }

    #[test]
    fn a_plain_peek_is_list_wide_and_taller_with_text() {
        let w = NotchWidth::Medium;
        let bare = peek_size(NOTCH, w, PeekContent::Plain { has_text: false }, None);
        let text = peek_size(NOTCH, w, PeekContent::Plain { has_text: true }, None);
        assert_eq!(bare, Size::new(340.0, CHROME + 56.0));
        assert_eq!(text, Size::new(340.0, CHROME + 100.0));
    }

    #[test]
    fn a_notice_is_taller_when_long() {
        let w = NotchWidth::Medium;
        assert_eq!(
            peek_size(NOTCH, w, PeekContent::Notice { long: false }, None).h,
            CHROME + 58.0
        );
        assert_eq!(
            peek_size(NOTCH, w, PeekContent::Notice { long: true }, None).h,
            CHROME + 78.0
        );
    }

    #[test]
    fn a_card_peek_is_panel_wide_measured_beats_estimate_and_both_are_capped() {
        let w = NotchWidth::Large;
        let card = PeekContent::Card { height: 200.0 };
        assert_eq!(
            peek_size(NOTCH, w, card, None),
            Size::new(520.0, CHROME + 216.0)
        );
        assert_eq!(peek_size(NOTCH, w, card, Some(150.0)).h, CHROME + 166.0);
        let huge = PeekContent::Card { height: 2000.0 };
        assert_eq!(peek_size(NOTCH, w, huge, None).h, CHROME + MAX_CARD + 16.0);
        assert_eq!(
            peek_size(NOTCH, w, card, Some(900.0)).h,
            CHROME + MAX_CARD + 16.0
        );
    }

    // Port of NotchForeignTests.testModelCountsAndAnswersPermission's size check: a permission peek
    // is taller than a plain one.
    #[test]
    fn a_permission_peek_is_taller_than_a_plain_one() {
        let w = NotchWidth::Medium;
        let permission = PeekContent::Card {
            height: permission_height(40, false),
        };
        let plain = PeekContent::Plain { has_text: true };
        assert!(peek_size(NOTCH, w, permission, None).h > peek_size(NOTCH, w, plain, None).h);
    }

    #[test]
    fn the_list_adds_rows_and_caps_them() {
        let w = NotchWidth::Medium;
        assert_eq!(
            list_size(NOTCH, w, &[], false),
            Size::new(340.0, CHROME + 34.0 + 44.0 + 6.0)
        );
        assert_eq!(
            list_size(NOTCH, w, &[44.0, 58.0], false).h,
            CHROME + 34.0 + (48.0 + 62.0) + 6.0
        );
        // The Done tab has a "Clear all" line.
        assert_eq!(
            list_size(NOTCH, w, &[44.0], true).h,
            CHROME + 34.0 + (28.0 + 48.0) + 6.0
        );
        assert_eq!(
            list_size(NOTCH, w, &[58.0; 20], false).h,
            CHROME + 34.0 + MAX_LIST + 6.0
        );
    }

    #[test]
    fn detail_is_taller_with_a_card() {
        let w = NotchWidth::Medium;
        assert_eq!(
            detail_size(NOTCH, w, false),
            Size::new(420.0, CHROME + 420.0)
        );
        assert_eq!(
            detail_size(NOTCH, w, true),
            Size::new(420.0, CHROME + 470.0)
        );
    }

    #[test]
    fn rows_are_two_lines_only_with_a_prompt_and_not_done() {
        assert_eq!(row_height(false, true), 58.0);
        assert_eq!(row_height(false, false), 44.0);
        assert_eq!(row_height(true, true), 44.0);
    }

    #[test]
    fn card_estimates_grow_with_text_and_options_up_to_a_limit() {
        assert_eq!(question_height(10, 2), 28.0 + 17.0 + 72.0 + 48.0);
        assert!(question_height(10, 4) > question_height(10, 2));
        assert!(question_height(130, 2) > question_height(10, 2));
        assert_eq!(question_height(1000, 2), question_height(180, 2));
        assert_eq!(
            permission_height(10, false),
            28.0 + 20.0 + 16.0 + 14.0 + 72.0 + 70.0
        );
        assert!(permission_height(10, true) > permission_height(10, false));
        assert_eq!(
            permission_height(1000, false),
            permission_height(248, false)
        );
    }

    fn macbook() -> ScreenInfo {
        ScreenInfo {
            frame: Rect::new(0.0, 0.0, 1512.0, 982.0),
            visible_max_y: 950.0,
            safe_top: 32.0,
            aux_left_width: Some(662.0),
            aux_right_width: Some(662.0),
        }
    }

    // Ported from the spike's notch_rect tests.
    #[test]
    fn notch_rect_on_a_macbook() {
        assert_eq!(
            notch_rect(&macbook()),
            Some(Rect::new(662.0, 950.0, 188.0, 32.0))
        );
    }

    #[test]
    fn no_notch_rect_without_a_housing() {
        let s = ScreenInfo {
            frame: Rect::new(0.0, 0.0, 2560.0, 1440.0),
            visible_max_y: 1415.0,
            safe_top: 0.0,
            aux_left_width: None,
            aux_right_width: None,
        };
        assert_eq!(notch_rect(&s), None);
        // The fallback: a zero-width strip as tall as the menu bar, in the middle.
        let g = geometry(&s);
        assert_eq!(g.notch, Size::new(0.0, 25.0));
        assert_eq!(g.center_x, 1280.0);
        let short_bar = ScreenInfo {
            visible_max_y: 1430.0,
            ..s
        };
        assert_eq!(geometry(&short_bar).notch.h, 24.0);
    }

    #[test]
    fn the_canvas_is_screen_wide_centred_on_the_camera_and_hangs_from_the_top() {
        let g = geometry(&macbook());
        assert_eq!(g.notch, Size::new(188.0, 32.0));
        assert_eq!(g.center_x, 756.0);
        let c = canvas_frame(&g);
        assert_eq!(c, Rect::new(0.0, 982.0 - 640.0, 1512.0, 640.0));
        assert_eq!(c.mid_x(), g.center_x);
        // A small screen still gets the least canvas.
        let tiny = Geometry {
            screen: Rect::new(0.0, 0.0, 400.0, 700.0),
            notch: Size::new(0.0, 24.0),
            center_x: 200.0,
        };
        assert_eq!(canvas_frame(&tiny).w, CANVAS.w);
        // A camera off centre (a second display to the right): the canvas covers the far side.
        let off = Geometry {
            screen: Rect::new(1512.0, 0.0, 1000.0, 800.0),
            notch: Size::new(0.0, 24.0),
            center_x: 1812.0,
        };
        let c = canvas_frame(&off);
        assert_eq!((c.w, c.mid_x()), (1400.0, 1812.0));
    }

    #[test]
    fn the_shape_hangs_from_the_top_centred_with_slack_only_when_collapsed() {
        let g = geometry(&macbook());
        let closed = shape_rect(&g, collapsed_size(g.notch), true);
        assert_eq!(closed, Rect::new(756.0 - 102.0, 982.0 - 36.0, 204.0, 36.0));
        let open = shape_rect(&g, Size::new(340.0, 132.0), false);
        assert_eq!(open, Rect::new(586.0, 850.0, 340.0, 132.0));
    }

    // Ported from the spike's hit_test tests.
    #[test]
    fn hit_test_edges() {
        let shape = Rect::new(100.0, 900.0, 200.0, 32.0);
        assert!(hit_test(shape, (100.0, 900.0)), "the corner is inside");
        assert!(
            !hit_test(shape, (300.0, 916.0)),
            "the right edge is outside"
        );
        assert!(!hit_test(shape, (99.9, 916.0)));
        assert!(!hit_test(shape, (150.0, 899.0)));
    }

    #[test]
    fn hit_test_top_edge_is_inside() {
        let shape = Rect::new(100.0, 900.0, 200.0, 32.0);
        assert!(hit_test(shape, (150.0, 932.0)));
        assert!(!hit_test(shape, (150.0, 932.5)));
    }

    #[test]
    fn the_collapsed_slack_takes_a_pointer_just_below_the_strip() {
        let g = geometry(&macbook());
        let closed = shape_rect(&g, collapsed_size(g.notch), true);
        assert!(hit_test(closed, (756.0, 982.0 - 34.0)));
        assert!(!hit_test(closed, (756.0, 982.0 - 37.0)));
        // Beside the housing the click falls through.
        assert!(!hit_test(closed, (756.0 - 120.0, 970.0)));
    }
}
