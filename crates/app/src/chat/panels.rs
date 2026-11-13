use std::path::Path;

use notchling_engine::settings::Settings;
use serde_json::{json, Value};

pub const KEY: &str = "chatPanels";

pub const SIDEBAR_DEFAULT: f32 = 272.0;
pub const SIDEBAR_MIN: f32 = 200.0;
pub const SIDEBAR_MAX: f32 = 420.0;
pub const DETAILS_DEFAULT: f32 = 260.0;
pub const DETAILS_MIN: f32 = 240.0;
pub const DETAILS_MAX: f32 = 520.0;
pub const COLUMN_MIN: f32 = 360.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Sidebar,
    Details,
}

impl Side {
    pub fn default_width(self) -> f32 {
        match self {
            Side::Sidebar => SIDEBAR_DEFAULT,
            Side::Details => DETAILS_DEFAULT,
        }
    }

    pub fn min(self) -> f32 {
        match self {
            Side::Sidebar => SIDEBAR_MIN,
            Side::Details => DETAILS_MIN,
        }
    }

    pub fn max(self) -> f32 {
        match self {
            Side::Sidebar => SIDEBAR_MAX,
            Side::Details => DETAILS_MAX,
        }
    }

    pub fn clamp(self, width: f32) -> f32 {
        width.clamp(self.min(), self.max())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Widths {
    pub sidebar: f32,
    pub details: f32,
}

impl Default for Widths {
    fn default() -> Self {
        Widths {
            sidebar: SIDEBAR_DEFAULT,
            details: DETAILS_DEFAULT,
        }
    }
}

impl Widths {
    pub fn get(&self, side: Side) -> f32 {
        match side {
            Side::Sidebar => self.sidebar,
            Side::Details => self.details,
        }
    }

    pub fn set(&mut self, side: Side, width: f32) {
        match side {
            Side::Sidebar => self.sidebar = width,
            Side::Details => self.details = width,
        }
    }
}

pub fn ratio(width: f32, window: f32) -> f32 {
    if window > 0.0 {
        width / window
    } else {
        0.0
    }
}

pub fn width_from(side: Side, ratio: f32, window: f32) -> f32 {
    if ratio.is_finite() && ratio > 0.0 && window > 0.0 {
        side.clamp(ratio * window)
    } else {
        side.default_width()
    }
}

pub fn shown(preferred: Widths, sidebar_on: bool, details_on: bool, window: f32) -> Widths {
    let mut sidebar = if sidebar_on {
        Side::Sidebar.clamp(preferred.sidebar)
    } else {
        0.0
    };
    let mut details = if details_on {
        Side::Details.clamp(preferred.details)
    } else {
        0.0
    };
    let over = sidebar + details + COLUMN_MIN - window;
    if over > 0.0 {
        let slack_sidebar = (sidebar - Side::Sidebar.min()).max(0.0);
        let slack_details = (details - Side::Details.min()).max(0.0);
        let slack = slack_sidebar + slack_details;
        if slack > 0.0 {
            let take = over.min(slack);
            sidebar -= take * slack_sidebar / slack;
            details -= take * slack_details / slack;
        }
    }
    Widths { sidebar, details }
}

pub fn dragged(side: Side, pointer_x: f32, window: f32, other: f32) -> f32 {
    let wanted = match side {
        Side::Sidebar => pointer_x,
        Side::Details => window - pointer_x,
    };
    let room = (window - COLUMN_MIN - other).max(side.min());
    wanted.clamp(side.min(), side.max().min(room))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Saved {
    pub sidebar: Option<f32>,
    pub details: Option<f32>,
    pub sidebar_hidden: bool,
    pub details_hidden: bool,
}

impl Saved {
    pub fn of(preferred: Widths, sidebar_hidden: bool, details_hidden: bool, window: f32) -> Saved {
        let part = |side: Side| {
            let width = preferred.get(side);
            ((width - side.default_width()).abs() > 0.5).then(|| ratio(width, window))
        };
        Saved {
            sidebar: part(Side::Sidebar),
            details: part(Side::Details),
            sidebar_hidden,
            details_hidden,
        }
    }

    pub fn widths(&self, window: f32) -> Widths {
        let part = |side: Side, ratio: Option<f32>| {
            ratio.map_or(side.default_width(), |r| width_from(side, r, window))
        };
        Widths {
            sidebar: part(Side::Sidebar, self.sidebar),
            details: part(Side::Details, self.details),
        }
    }

    pub fn to_value(&self) -> Value {
        json!({"sidebar": self.sidebar, "details": self.details,
               "sidebarHidden": self.sidebar_hidden, "detailsHidden": self.details_hidden})
    }

    pub fn from_value(value: Option<&Value>) -> Option<Saved> {
        let v = value?.as_object()?;
        let ratio = |k: &str| {
            v.get(k)
                .and_then(Value::as_f64)
                .map(|n| n as f32)
                .filter(|n| n.is_finite() && *n > 0.0 && *n < 1.0)
        };
        let flag = |k: &str| v.get(k).and_then(Value::as_bool).unwrap_or(false);
        Some(Saved {
            sidebar: ratio("sidebar"),
            details: ratio("details"),
            sidebar_hidden: flag("sidebarHidden"),
            details_hidden: flag("detailsHidden"),
        })
    }
}

pub fn load(settings: &Path) -> Option<Saved> {
    let store = Settings::open(settings).ok()?;
    Saved::from_value(store.get(KEY).as_ref())
}

pub fn save(settings: &Path, saved: &Saved) -> Result<(), String> {
    let mut store = Settings::open(settings).map_err(|e| e.to_string())?;
    store.set(KEY, saved.to_value()).map_err(|e| e.to_string())
}

#[cfg(test)]
#[path = "panels_tests.rs"]
mod tests;
