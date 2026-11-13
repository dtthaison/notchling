#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NotchState {
    Collapsed,
    Peek(String),
    List,
    Detail(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NotchWidth {
    Small,
    #[default]
    Medium,
    Large,
}

impl NotchWidth {
    pub fn from_key(key: &str) -> Option<NotchWidth> {
        match key {
            "small" => Some(NotchWidth::Small),
            "medium" => Some(NotchWidth::Medium),
            "large" => Some(NotchWidth::Large),
            _ => None,
        }
    }

    pub fn list(self) -> f64 {
        match self {
            NotchWidth::Small => 300.0,
            NotchWidth::Medium => 340.0,
            NotchWidth::Large => 400.0,
        }
    }

    pub fn panel(self) -> f64 {
        match self {
            NotchWidth::Small => 360.0,
            NotchWidth::Medium => 420.0,
            NotchWidth::Large => 520.0,
        }
    }
}

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

pub const FLARE: f64 = 8.0;
pub const MAX_CARD: f64 = 520.0;
pub const TAB_BAR: f64 = 34.0;
pub const MAX_LIST: f64 = 300.0;
pub const DEFAULT_NOTCH: Size = Size::new(185.0, 32.0);
pub const CANVAS_WIDTH: f64 = 560.0;
pub const CANVAS_SLACK: f64 = 16.0;
pub const PET_REACH: f64 = 48.0;
pub const COLLAPSED_SLACK: f64 = 4.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PeekContent {
    Notice { long: bool },
    Card { height: f64 },
    Plain { has_text: bool },
}

fn chrome(notch: Size) -> f64 {
    notch.h + 2.0 * FLARE
}

pub fn collapsed_size(notch: Size) -> Size {
    Size::new(notch.w + 2.0 * FLARE, notch.h)
}

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

pub fn detail_size(notch: Size, width: NotchWidth, has_card: bool) -> Size {
    Size::new(
        width.panel(),
        chrome(notch) + if has_card { 470.0 } else { 420.0 },
    )
}

pub fn row_height(completed: bool, has_prompt: bool) -> f64 {
    if completed || !has_prompt {
        44.0
    } else {
        58.0
    }
}

pub fn question_height(question_chars: usize, options: usize) -> f64 {
    let lines = (question_chars / 60 + 1).min(3) as f64;
    28.0 + lines * 17.0 + options as f64 * 36.0 + 40.0 + 8.0
}

pub fn permission_height(summary_chars: usize, can_always_allow: bool) -> f64 {
    let lines = (summary_chars / 62 + 1).min(4) as f64;
    let options = if can_always_allow { 3.0 } else { 2.0 };
    28.0 + 20.0 + lines * 16.0 + 14.0 + options * 36.0 + 42.0 + 28.0
}

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScreenInfo {
    pub frame: Rect,
    pub visible_max_y: f64,
    pub safe_top: f64,
    pub aux_left_width: Option<f64>,
    pub aux_right_width: Option<f64>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Geometry {
    pub screen: Rect,
    pub notch: Size,
    pub center_x: f64,
}

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

pub fn canvas_size(notch: Size) -> Size {
    let closed = collapsed_size(notch);
    let largest = NotchWidth::Large;
    let card = PeekContent::Card { height: MAX_CARD };
    let tallest = [
        peek_size(notch, largest, card, None).h,
        detail_size(notch, largest, true).h,
        list_size(notch, largest, &[44.0; 16], true).h,
        closed.h + PET_REACH,
    ]
    .into_iter()
    .fold(0.0, f64::max);
    Size::new(
        CANVAS_WIDTH.max(closed.w + 2.0 * PET_REACH),
        tallest + CANVAS_SLACK,
    )
}

pub fn canvas_frame(g: &Geometry) -> Rect {
    let c = canvas_size(g.notch);
    Rect::new(g.center_x - c.w / 2.0, g.screen.max_y() - c.h, c.w, c.h)
}

pub fn shape_rect(g: &Geometry, size: Size, collapsed: bool) -> Rect {
    let extra = if collapsed { COLLAPSED_SLACK } else { 0.0 };
    Rect::new(
        g.center_x - size.w / 2.0,
        g.screen.max_y() - size.h - extra,
        size.w,
        size.h + extra,
    )
}

pub fn hit_test(shape: Rect, mouse: (f64, f64)) -> bool {
    let (mx, my) = mouse;
    mx >= shape.x && mx < shape.max_x() && my >= shape.y && my <= shape.max_y()
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOTCH: Size = DEFAULT_NOTCH;
    const CHROME: f64 = 48.0;

    #[test]
    fn collapsed_is_the_housing_plus_the_flares() {
        assert_eq!(collapsed_size(NOTCH), Size::new(201.0, 32.0));
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
    fn the_canvas_fits_the_largest_shape_whatever_the_width_setting() {
        let c = canvas_size(NOTCH);
        assert_eq!(c, Size::new(560.0, CHROME + MAX_CARD + 16.0 + CANVAS_SLACK));
        for width in [NotchWidth::Small, NotchWidth::Medium, NotchWidth::Large] {
            let shapes = [
                collapsed_size(NOTCH),
                peek_size(NOTCH, width, PeekContent::Card { height: 9999.0 }, None),
                peek_size(NOTCH, width, PeekContent::Notice { long: true }, None),
                peek_size(NOTCH, width, PeekContent::Plain { has_text: true }, None),
                list_size(NOTCH, width, &[58.0; 40], true),
                detail_size(NOTCH, width, true),
            ];
            for s in shapes {
                assert!(
                    s.w + 2.0 * FLARE <= c.w && s.h < c.h,
                    "{width:?}: {s:?} in {c:?}"
                );
            }
        }
        let closed = collapsed_size(NOTCH);
        assert!(closed.w + 2.0 * PET_REACH <= c.w && closed.h + PET_REACH < c.h);
        assert_eq!(canvas_size(Size::new(200.0, 40.0)).h, c.h + 8.0);
        assert_eq!(canvas_size(Size::new(0.0, 24.0)).h, c.h - 8.0);
        assert_eq!(
            canvas_size(Size::new(600.0, 32.0)).w,
            616.0 + 2.0 * PET_REACH
        );
    }

    #[test]
    fn the_canvas_is_centred_on_the_camera_and_hangs_from_the_top() {
        let g = geometry(&macbook());
        assert_eq!(g.notch, Size::new(188.0, 32.0));
        assert_eq!(g.center_x, 756.0);
        let c = canvas_frame(&g);
        let size = canvas_size(g.notch);
        assert_eq!(
            c,
            Rect::new(756.0 - size.w / 2.0, 982.0 - size.h, size.w, size.h)
        );
        assert_eq!(c.mid_x(), g.center_x);
        assert_eq!(c.max_y(), g.screen.max_y());
        let wide = Geometry {
            screen: Rect::new(0.0, 0.0, 5120.0, 2880.0),
            notch: g.notch,
            center_x: 2560.0,
        };
        assert_eq!(canvas_frame(&wide).w, size.w);
        let off = Geometry {
            screen: Rect::new(1512.0, 0.0, 1000.0, 800.0),
            notch: Size::new(0.0, 24.0),
            center_x: 1812.0,
        };
        let c = canvas_frame(&off);
        assert_eq!((c.w, c.mid_x(), c.max_y()), (CANVAS_WIDTH, 1812.0, 800.0));
        let open = shape_rect(&g, Size::new(520.0, 584.0), false);
        let canvas = canvas_frame(&g);
        assert!(open.x >= canvas.x && open.max_x() <= canvas.max_x() && open.y >= canvas.y);
    }

    #[test]
    fn the_shape_hangs_from_the_top_centred_with_slack_only_when_collapsed() {
        let g = geometry(&macbook());
        let closed = shape_rect(&g, collapsed_size(g.notch), true);
        assert_eq!(closed, Rect::new(756.0 - 102.0, 982.0 - 36.0, 204.0, 36.0));
        let open = shape_rect(&g, Size::new(340.0, 132.0), false);
        assert_eq!(open, Rect::new(586.0, 850.0, 340.0, 132.0));
    }

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
        assert!(!hit_test(closed, (756.0 - 120.0, 970.0)));
    }
}
