use crate::menu::{Dots, TrayGlyph, TrayLook};

pub const WIDTH: f64 = 22.0;
pub const HEIGHT: f64 = 16.0;
pub const STROKE: f64 = 1.6;
pub const PULSE_FRAMES: usize = 8;
pub const PULSE_SECONDS: f64 = 10.0;
pub const PULSE_PERIOD: f64 = 2.0;
pub const PULSE_LOW: f64 = 0.35;
pub const DOT_CENTERS: [(f64, f64); 3] = [(5.0, 12.0), (11.0, 8.0), (17.0, 4.0)];
pub const IDLE_RADIUS: f64 = 1.3;
pub const WORKING_RADIUS: f64 = 1.5;
pub const WAITING_RADIUS: f64 = 1.35;
pub const WAITING_STROKE: f64 = 1.0;
pub const BLOCKED_RADIUS: f64 = 2.1;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pill {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub radius: f64,
    pub turn_degrees: f64,
}

pub const PILLS: [Pill; 3] = [
    Pill {
        x: 3.2,
        y: 2.2,
        width: 3.6,
        height: 11.6,
        radius: 1.8,
        turn_degrees: 0.0,
    },
    Pill {
        x: 15.2,
        y: 2.2,
        width: 3.6,
        height: 11.6,
        radius: 1.8,
        turn_degrees: 0.0,
    },
    Pill {
        x: 9.2,
        y: 1.4,
        width: 3.6,
        height: 13.2,
        radius: 1.8,
        turn_degrees: -37.0,
    },
];

pub const TURN_CENTER: (f64, f64) = (11.0, 8.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    Green,
    Orange,
    Red,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Mark {
    Bead {
        x: f64,
        y: f64,
        radius: f64,
    },
    Ring {
        x: f64,
        y: f64,
        radius: f64,
        line: f64,
    },
}

impl Mark {
    pub fn center(&self) -> (f64, f64) {
        match *self {
            Mark::Bead { x, y, .. } | Mark::Ring { x, y, .. } => (x, y),
        }
    }

    pub fn radius(&self) -> f64 {
        match *self {
            Mark::Bead { radius, .. } | Mark::Ring { radius, .. } => radius,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    pub filled: bool,
    pub tint: Tint,
    pub marks: Vec<Mark>,
    pub alpha: f64,
}

pub fn tint_of(dots: Dots) -> Option<Tint> {
    match dots {
        Dots::Idle => None,
        Dots::Working => Some(Tint::Green),
        Dots::Waiting => Some(Tint::Orange),
        Dots::Blocked => Some(Tint::Red),
    }
}

pub fn marks_of(dots: Dots) -> Vec<Mark> {
    DOT_CENTERS
        .iter()
        .map(|&(x, y)| match dots {
            Dots::Waiting => Mark::Ring {
                x,
                y,
                radius: WAITING_RADIUS,
                line: WAITING_STROKE,
            },
            Dots::Blocked => Mark::Bead {
                x,
                y,
                radius: BLOCKED_RADIUS,
            },
            Dots::Working => Mark::Bead {
                x,
                y,
                radius: WORKING_RADIUS,
            },
            Dots::Idle => Mark::Bead {
                x,
                y,
                radius: IDLE_RADIUS,
            },
        })
        .collect()
}

pub fn pulse_alpha(frame: usize) -> f64 {
    let phase = frame as f64 / PULSE_FRAMES as f64;
    let wave = if phase < 0.5 {
        phase * 2.0
    } else {
        2.0 - phase * 2.0
    };
    1.0 - (1.0 - PULSE_LOW) * wave
}

pub fn frames_of(look: &TrayLook) -> Vec<Frame> {
    let Some(tint) = tint_of(look.dots) else {
        return Vec::new();
    };
    let count = if look.pulse { PULSE_FRAMES } else { 1 };
    (0..count)
        .map(|i| Frame {
            filled: look.glyph == TrayGlyph::Filled,
            tint,
            marks: marks_of(look.dots),
            alpha: if look.pulse { pulse_alpha(i) } else { 1.0 },
        })
        .collect()
}

pub fn pulse_step_seconds() -> f64 {
    PULSE_PERIOD / PULSE_FRAMES as f64
}

pub fn pulse_steps() -> usize {
    (PULSE_SECONDS / pulse_step_seconds()).round() as usize
}

pub fn pulse_frame_at(step: usize) -> Option<usize> {
    (step < pulse_steps()).then_some(step % PULSE_FRAMES)
}

pub fn pill_polygon(index: usize, arc_steps: usize) -> Vec<(f64, f64)> {
    let pill = PILLS[index];
    let (cx, top, bottom) = (
        pill.x + pill.width / 2.0,
        pill.y + pill.radius,
        pill.y + pill.height - pill.radius,
    );
    let turn = pill.turn_degrees.to_radians();
    let (sin, cos) = turn.sin_cos();
    let (px, py) = TURN_CENTER;
    let mut points = Vec::with_capacity(arc_steps * 2 + 2);
    for step in 0..=arc_steps {
        let angle = std::f64::consts::PI * (1.0 + step as f64 / arc_steps as f64);
        points.push((
            cx + pill.radius * angle.cos(),
            top + pill.radius * angle.sin(),
        ));
    }
    for step in 0..=arc_steps {
        let angle = std::f64::consts::PI * step as f64 / arc_steps as f64;
        points.push((
            cx + pill.radius * angle.cos(),
            bottom + pill.radius * angle.sin(),
        ));
    }
    points
        .into_iter()
        .map(|(x, y)| {
            let (dx, dy) = (x - px, y - py);
            (px + dx * cos - dy * sin, py + dx * sin + dy * cos)
        })
        .collect()
}

pub fn circle_polygon(x: f64, y: f64, radius: f64, steps: usize) -> Vec<(f64, f64)> {
    (0..steps)
        .map(|i| {
            let angle = std::f64::consts::TAU * i as f64 / steps as f64;
            (x + radius * angle.cos(), y + radius * angle.sin())
        })
        .collect()
}

#[cfg(test)]
#[path = "tray_glyph_tests.rs"]
mod tests;
