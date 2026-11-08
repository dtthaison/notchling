//! Motion (`KitMotion`): three curves, all off under Reduce Motion.

/// What an animation is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Motion {
    /// Hover fills.
    Hover,
    /// Button press scale.
    Press,
    /// A new message, a pending reply.
    Appear,
}

/// A timing curve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    EaseOut { seconds: f32 },
    Spring { response: f32, damping: f32 },
}

impl Motion {
    pub const ALL: [Motion; 3] = [Motion::Hover, Motion::Press, Motion::Appear];

    /// The curve, or `None` (no animation) when the user asked for reduced motion.
    pub fn curve(self, reduce_motion: bool) -> Option<Curve> {
        if reduce_motion {
            return None;
        }
        Some(match self {
            Motion::Hover => Curve::EaseOut { seconds: 0.12 },
            Motion::Press => Curve::Spring {
                response: 0.25,
                damping: 0.7,
            },
            Motion::Appear => Curve::EaseOut { seconds: 0.2 },
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curves_follow_the_table() {
        assert_eq!(
            Motion::Hover.curve(false),
            Some(Curve::EaseOut { seconds: 0.12 })
        );
        assert_eq!(
            Motion::Press.curve(false),
            Some(Curve::Spring {
                response: 0.25,
                damping: 0.7
            })
        );
        assert_eq!(
            Motion::Appear.curve(false),
            Some(Curve::EaseOut { seconds: 0.2 })
        );
    }

    #[test]
    fn reduce_motion_turns_every_curve_off() {
        for m in Motion::ALL {
            assert_eq!(m.curve(true), None);
        }
    }
}
