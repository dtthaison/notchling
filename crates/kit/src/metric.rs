pub const SIDEBAR_WIDTH: f32 = 272.0;
pub const TITLE_BAR: f32 = 38.0;
pub const CONTROL: f32 = 28.0;
pub const SESSION_ROW: f32 = 52.0;
pub const SESSION_ROW_ACCOUNT: f32 = 52.0;
pub const SESSION_LINE_HEADING: f32 = 16.0;
pub const SESSION_LINE_META: f32 = 13.0;
pub const SPINNER_MINI: f32 = 10.0;
pub const SPINNER_SMALL: f32 = 16.0;
pub const TOOL_ICON: f32 = 14.0;
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_MD: f32 = 10.0;
pub const RADIUS_CARD: f32 = 14.0;
pub const RADIUS_COMPOSER: f32 = 18.0;
pub const TERMINAL_FONT: f32 = 12.5;
pub const TERMINAL_SHEET: (f32, f32) = (900.0, 600.0);
pub const SIDE_PADDING: f32 = 16.0;
pub const AVATAR: f32 = 30.0;
pub const OPTION_BADGE: f32 = 22.0;
pub const PILL_HEIGHT: f32 = 20.0;
pub const SEND_DISC: f32 = 30.0;
pub const METER_BAR: f32 = 3.0;
pub const PANEL_EDGE_HIT: f32 = 9.0;
pub const PANEL_EDGE_LINE: f32 = 2.0;
pub const PANEL_EDGE_BEFORE: f32 = 2.0;
pub const WINDOW_DEFAULT: (f32, f32) = (1040.0, 720.0);
pub const WINDOW_MIN: (f32, f32) = (720.0, 460.0);
pub const BUTTON_PADDING: f32 = 12.0;

pub const MENU_PADDING: f32 = 4.0;
pub const MENU_ROW_X: f32 = 8.0;
pub const MENU_ROW_Y: f32 = 3.0;
pub const MENU_ROW_GAP: f32 = 8.0;
pub const MENU_MIN_WIDTH: f32 = 112.0;
pub const MENU_MAX_WIDTH: f32 = 260.0;
pub const MENU_MAX_HEIGHT: f32 = 420.0;
pub const MENU_MARK: f32 = 12.0;
pub const MENU_ICON: f32 = 11.0;
pub const MENU_GAP: f32 = 6.0;
pub const MENU_RULE_Y: f32 = 2.0;
pub const MENU_RADIUS: f32 = RADIUS_CARD;

pub const SPACE: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 20.0, 24.0];

pub fn space(steps: usize) -> f32 {
    4.0 * steps as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_is_fours() {
        for (i, value) in SPACE.iter().enumerate() {
            assert_eq!(*value, space(i + 1));
            assert_eq!(*value % 4.0, 0.0);
        }
    }

    #[test]
    fn values_follow_the_table() {
        assert_eq!(SIDEBAR_WIDTH, 272.0);
        assert_eq!(TITLE_BAR, 38.0);
        assert_eq!((SESSION_ROW, CONTROL), (52.0, 28.0));
        assert_eq!(
            (RADIUS_SM, RADIUS_MD, RADIUS_CARD, RADIUS_COMPOSER),
            (6.0, 10.0, 14.0, 18.0)
        );
        assert_eq!((MENU_PADDING, MENU_ROW_X, MENU_ROW_Y), (4.0, 8.0, 3.0));
        assert!(WINDOW_MIN.0 <= WINDOW_DEFAULT.0 && WINDOW_MIN.1 <= WINDOW_DEFAULT.1);
    }
}
