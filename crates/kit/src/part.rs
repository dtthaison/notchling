use crate::appearance::Appearance;
use crate::color::Color;
use crate::ink::Ink;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PartState {
    Rest,
    Hover,
    Pressed,
    Selected,
    Focused,
    Disabled,
}

impl PartState {
    pub const ALL: [PartState; 6] = [
        PartState::Rest,
        PartState::Hover,
        PartState::Pressed,
        PartState::Selected,
        PartState::Focused,
        PartState::Disabled,
    ];

    pub fn name(self) -> &'static str {
        match self {
            PartState::Rest => "Rest",
            PartState::Hover => "Hover",
            PartState::Pressed => "Pressed",
            PartState::Selected => "Selected",
            PartState::Focused => "Focused",
            PartState::Disabled => "Disabled",
        }
    }

    fn lit(self) -> bool {
        matches!(self, PartState::Hover | PartState::Pressed)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ButtonKind {
    Primary,
    Secondary,
    Ghost,
}

impl ButtonKind {
    pub const ALL: [ButtonKind; 3] = [
        ButtonKind::Primary,
        ButtonKind::Secondary,
        ButtonKind::Ghost,
    ];

    pub fn name(self) -> &'static str {
        match self {
            ButtonKind::Primary => "Primary",
            ButtonKind::Secondary => "Secondary",
            ButtonKind::Ghost => "Ghost",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Border {
    pub color: Color,
    pub width: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Look {
    pub fill: Option<Color>,
    pub text: Color,
    pub border: Option<Border>,
    pub ring: Option<Border>,
    pub opacity: f32,
    pub scale: f32,
}

impl Look {
    pub fn edge(&self) -> Option<Border> {
        self.ring.or(self.border)
    }
}

pub fn button_look(kind: ButtonKind, state: PartState, appearance: Appearance) -> Look {
    let ink = |i: Ink| i.color(appearance);
    let fill = match kind {
        ButtonKind::Primary => Some(ink(Ink::Accent).with_alpha(match state {
            PartState::Pressed => 0.8,
            PartState::Hover => 0.9,
            _ => 1.0,
        })),
        ButtonKind::Secondary => Some(if state.lit() {
            ink(Ink::Raised)
        } else {
            ink(Ink::Card)
        }),
        ButtonKind::Ghost => state.lit().then(|| ink(Ink::Raised)),
    };
    Look {
        fill,
        text: ink(if kind == ButtonKind::Primary {
            Ink::OnAccent
        } else {
            Ink::Text
        }),
        border: (kind == ButtonKind::Secondary).then(|| Border {
            color: ink(Ink::Hairline),
            width: 1.0,
        }),
        ring: (state == PartState::Focused).then(|| Border {
            color: ink(Ink::Accent),
            width: 2.0,
        }),
        opacity: if state == PartState::Disabled {
            0.4
        } else {
            1.0
        },
        scale: if state == PartState::Pressed {
            0.97
        } else {
            1.0
        },
    }
}

pub fn row_look(state: PartState, focused: bool, appearance: Appearance) -> Look {
    let ink = |i: Ink| i.color(appearance);
    let fill = match state {
        PartState::Selected => Some(ink(Ink::Selection)),
        PartState::Pressed => Some(ink(Ink::Raised)),
        PartState::Hover => Some(ink(Ink::Raised).with_alpha(0.6)),
        _ => None,
    };
    let selected = state == PartState::Selected;
    Look {
        fill,
        text: ink(Ink::Text),
        border: selected.then(|| Border {
            color: ink(Ink::Accent).with_alpha(0.35),
            width: 1.0,
        }),
        ring: (focused && selected).then(|| Border {
            color: ink(Ink::Accent),
            width: 2.0,
        }),
        opacity: 1.0,
        scale: 1.0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptionLook {
    pub look: Look,
    pub badge_fill: Color,
    pub badge_text: Color,
}

pub fn option_look(state: PartState, selected: bool, appearance: Appearance) -> OptionLook {
    let ink = |i: Ink| i.color(appearance);
    let hot = selected || state.lit();
    OptionLook {
        look: Look {
            fill: Some(if state.lit() {
                ink(Ink::Raised)
            } else {
                ink(Ink::Card)
            }),
            text: ink(Ink::Text),
            border: Some(Border {
                color: if selected || state == PartState::Pressed {
                    ink(Ink::Accent)
                } else {
                    ink(Ink::Hairline)
                },
                width: 1.0,
            }),
            ring: None,
            opacity: if state == PartState::Disabled {
                0.4
            } else {
                1.0
            },
            scale: 1.0,
        },
        badge_fill: ink(if hot { Ink::Accent } else { Ink::Selection }),
        badge_text: ink(if hot { Ink::OnAccent } else { Ink::AccentText }),
    }
}

pub fn field_look(focused: bool, appearance: Appearance) -> Look {
    let ink = |i: Ink| i.color(appearance);
    Look {
        fill: Some(ink(Ink::Card)),
        text: ink(Ink::Text),
        border: Some(if focused {
            Border {
                color: ink(Ink::Accent),
                width: 1.5,
            }
        } else {
            Border {
                color: ink(Ink::Hairline),
                width: 1.0,
            }
        }),
        ring: None,
        opacity: 1.0,
        scale: 1.0,
    }
}

pub fn initials(name: &str) -> String {
    let letters: String = name
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(2)
        .filter_map(|w| w.chars().next())
        .collect();
    if letters.is_empty() {
        "?".to_owned()
    } else {
        letters.to_uppercase()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use Appearance::{Dark, Light};
    use PartState::*;

    #[test]
    fn primary_button_dims_when_lit_and_keeps_dark_text() {
        let accent = Ink::Accent.color(Dark);
        assert_eq!(
            button_look(ButtonKind::Primary, Rest, Dark).fill,
            Some(accent)
        );
        assert_eq!(
            button_look(ButtonKind::Primary, Hover, Dark).fill,
            Some(accent.with_alpha(0.9))
        );
        assert_eq!(
            button_look(ButtonKind::Primary, Pressed, Dark).fill,
            Some(accent.with_alpha(0.8))
        );
        for a in Appearance::ALL {
            assert_eq!(
                button_look(ButtonKind::Primary, Rest, a).text,
                Ink::OnAccent.color(a)
            );
            assert_eq!(button_look(ButtonKind::Primary, Rest, a).border, None);
        }
    }

    #[test]
    fn secondary_has_hairline_and_ghost_has_nothing_at_rest() {
        let s = button_look(ButtonKind::Secondary, Rest, Light);
        assert_eq!(s.fill, Some(Ink::Card.color(Light)));
        assert_eq!(
            s.border,
            Some(Border {
                color: Ink::Hairline.color(Light),
                width: 1.0
            })
        );
        assert_eq!(
            button_look(ButtonKind::Secondary, Hover, Light).fill,
            Some(Ink::Raised.color(Light))
        );
        let g = button_look(ButtonKind::Ghost, Rest, Light);
        assert_eq!((g.fill, g.border), (None, None));
        assert_eq!(
            button_look(ButtonKind::Ghost, Hover, Light).fill,
            Some(Ink::Raised.color(Light))
        );
    }

    #[test]
    fn pressed_scales_focused_rings_disabled_fades() {
        for kind in ButtonKind::ALL {
            assert_eq!(button_look(kind, Pressed, Dark).scale, 0.97);
            assert_eq!(button_look(kind, Rest, Dark).scale, 1.0);
            assert_eq!(button_look(kind, Disabled, Dark).opacity, 0.4);
            assert_eq!(
                button_look(kind, Focused, Dark).ring,
                Some(Border {
                    color: Ink::Accent.color(Dark),
                    width: 2.0
                })
            );
            assert_eq!(button_look(kind, Hover, Dark).ring, None);
        }
        let focused = button_look(ButtonKind::Secondary, Focused, Dark);
        assert_eq!(focused.edge().map(|b| b.width), Some(2.0));
    }

    #[test]
    fn row_states_follow_the_table() {
        assert_eq!(row_look(Rest, false, Dark).fill, None);
        assert_eq!(
            row_look(Hover, false, Dark).fill,
            Some(Ink::Raised.color(Dark).with_alpha(0.6))
        );
        assert_eq!(
            row_look(Pressed, false, Dark).fill,
            Some(Ink::Raised.color(Dark))
        );
        let selected = row_look(Selected, false, Dark);
        assert_eq!(selected.fill, Some(Ink::Selection.color(Dark)));
        assert_eq!(
            selected.border,
            Some(Border {
                color: Ink::Accent.color(Dark).with_alpha(0.35),
                width: 1.0
            })
        );
        assert_eq!(selected.ring, None);
        assert_eq!(
            row_look(Selected, true, Dark).ring.map(|b| b.width),
            Some(2.0)
        );
        assert_eq!(row_look(Rest, true, Dark).ring, None);
    }

    #[test]
    fn option_badge_turns_accent_when_hot() {
        let rest = option_look(Rest, false, Light);
        assert_eq!(rest.badge_fill, Ink::Selection.color(Light));
        assert_eq!(rest.badge_text, Ink::AccentText.color(Light));
        assert_eq!(rest.look.fill, Some(Ink::Card.color(Light)));
        assert_eq!(
            rest.look.border.map(|b| b.color),
            Some(Ink::Hairline.color(Light))
        );
        let hover = option_look(Hover, false, Light);
        assert_eq!(hover.badge_fill, Ink::Accent.color(Light));
        assert_eq!(hover.look.fill, Some(Ink::Raised.color(Light)));
        assert_eq!(
            option_look(Pressed, false, Light)
                .look
                .border
                .map(|b| b.color),
            Some(Ink::Accent.color(Light))
        );
        let ticked = option_look(Rest, true, Light);
        assert_eq!(ticked.badge_fill, Ink::Accent.color(Light));
        assert_eq!(
            ticked.look.border.map(|b| b.color),
            Some(Ink::Accent.color(Light))
        );
        assert_eq!(option_look(Disabled, false, Light).look.opacity, 0.4);
    }

    #[test]
    fn field_border_thickens_with_focus() {
        assert_eq!(field_look(false, Dark).border.map(|b| b.width), Some(1.0));
        let focused = field_look(true, Dark).border;
        assert_eq!(
            focused,
            Some(Border {
                color: Ink::Accent.color(Dark),
                width: 1.5
            })
        );
    }

    #[test]
    fn initials_take_two_words() {
        assert_eq!(initials("design system"), "DS");
        assert_eq!(initials("task-status-check"), "TS");
        assert_eq!(initials("wifi"), "W");
        assert_eq!(initials("  "), "?");
        assert_eq!(initials("9 lives"), "9L");
        assert_eq!(initials("é"), "É");
    }
}
