//! Layer 3: sizes (`KitMetric`), in points. Spec: docs/chat-kit.md, Metric.

pub const SIDEBAR_WIDTH: f32 = 272.0;
/// The traffic lights sit in the sidebar's top; nothing else goes there.
pub const TITLE_BAR: f32 = 38.0;
/// Buttons and fields.
pub const CONTROL: f32 = 28.0;
pub const SESSION_ROW: f32 = 52.0;
/// The sidebar's session row, which has a third line for the account: one height whether or not
/// the account is known, so the list doesn't jump when the accounts load.
pub const SESSION_ROW_ACCOUNT: f32 = 62.0;
/// A session row's tool icon.
pub const TOOL_ICON: f32 = 14.0;
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_MD: f32 = 10.0;
pub const RADIUS_CARD: f32 = 14.0;
pub const RADIUS_COMPOSER: f32 = 18.0;
pub const TERMINAL_FONT: f32 = 12.5;
pub const TERMINAL_SHEET: (f32, f32) = (900.0, 600.0);
/// The conversation, the sidebar header and the inspector share it.
pub const SIDE_PADDING: f32 = 16.0;
pub const AVATAR: f32 = 30.0;
/// A question's number badge.
pub const OPTION_BADGE: f32 = 22.0;
pub const PILL_HEIGHT: f32 = 20.0;
/// The send disc in the composer.
pub const SEND_DISC: f32 = 30.0;
/// The header's context and limit bars.
pub const METER_BAR: f32 = 3.0;
pub const WINDOW_DEFAULT: (f32, f32) = (1040.0, 720.0);
pub const WINDOW_MIN: (f32, f32) = (720.0, 460.0);
/// Horizontal padding inside a button.
pub const BUTTON_PADDING: f32 = 12.0;

/// The 4 pt grid: 4, 8, 12, 16, 20, 24.
pub const SPACE: [f32; 6] = [4.0, 8.0, 12.0, 16.0, 20.0, 24.0];

/// `space(1)` is 4, `space(6)` is 24; past the grid it keeps counting in fours.
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
        assert!(WINDOW_MIN.0 <= WINDOW_DEFAULT.0 && WINDOW_MIN.1 <= WINDOW_DEFAULT.1);
    }
}
