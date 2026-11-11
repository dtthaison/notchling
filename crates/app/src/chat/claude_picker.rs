//! The Model and Effort pickers under a Claude Code session's reply box (the Swift `ModelPicker`
//! and `EffortPicker`): the choices, the labels, which one is current, and the text a pick sends.
//! A pick is an ordinary reply, `/model <full id>` or `/effort <level>`, typed into the session as
//! in the terminal; with a value, neither goes to a terminal (`slash::route` only sends the bare
//! command there). Pure, so every system tests it.

use super::format::model_name;

/// The current models, in the order of Claude Code's own `/model` list (Swift `ModelPicker.choices`).
pub const MODELS: [&str; 4] = [
    "claude-opus-5-5",
    "claude-fable-5-2",
    "claude-sonnet-5-5",
    "claude-haiku-5-5",
];

/// Older ones, under More Models (Swift `ModelPicker.older`).
pub const OLDER_MODELS: [&str; 9] = [
    "claude-haiku-4-5",
    "claude-sonnet-5",
    "claude-opus-5",
    "claude-fable-5-1",
    "claude-fable-5",
    "claude-opus-4-8",
    "claude-opus-4-7",
    "claude-opus-4-6",
    "claude-sonnet-4-6",
];

/// The effort levels, lowest first (Swift `EffortPicker.levels`).
pub const LEVELS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// What the model button says before the transcript has named one.
pub const MODEL_PLACEHOLDER: &str = "Model";
/// What the effort button says before any reply has carried one.
pub const EFFORT_PLACEHOLDER: &str = "Effort";
/// The tooltips, the Swift `.help` texts.
pub const MODEL_TIP: &str =
    "The model this session uses. Pick another to switch it (like /model in the terminal).";
pub const EFFORT_TIP: &str = "How hard the model thinks. Pick another to switch it (like /effort in the terminal, which also makes it the default for new sessions).";
/// The row that opens the older models.
pub const MORE_MODELS: &str = "More Models";

/// Which picker.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Model,
    Effort,
}

impl Kind {
    /// `NOTCHLING_RENDER_PICKER`'s values.
    pub fn parse(value: &str) -> Option<Kind> {
        match value {
            "model" => Some(Kind::Model),
            "effort" => Some(Kind::Effort),
            _ => None,
        }
    }
}

/// One row of a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// What a pick sends after the command: the full model id or the level.
    pub value: &'static str,
    pub title: String,
    pub selected: bool,
}

/// "Extra High" for xhigh, else the level capitalised (Swift `EffortPicker.title`).
pub fn effort_title(level: &str) -> String {
    if level == "xhigh" {
        return "Extra High".to_owned();
    }
    let mut chars = level.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn model_rows(ids: &[&'static str], current: Option<&str>) -> Vec<Row> {
    // Compared by display name, as the Swift menu does.
    let shown = current.map(model_name);
    ids.iter()
        .map(|id| {
            let title = model_name(id);
            Row {
                value: id,
                selected: shown.as_deref() == Some(title.as_str()),
                title,
            }
        })
        .collect()
}

/// The menu's first section: the current models.
pub fn models(current: Option<&str>) -> Vec<Row> {
    model_rows(&MODELS, current)
}

/// Under More Models.
pub fn older_models(current: Option<&str>) -> Vec<Row> {
    model_rows(&OLDER_MODELS, current)
}

/// The effort menu.
pub fn efforts(current: Option<&str>) -> Vec<Row> {
    LEVELS
        .iter()
        .map(|level| Row {
            value: level,
            title: effort_title(level),
            selected: current == Some(*level),
        })
        .collect()
}

/// The model button: the name, or a placeholder.
pub fn model_label(current: Option<&str>) -> String {
    current.map_or_else(|| MODEL_PLACEHOLDER.to_owned(), model_name)
}

/// The effort button: the level's title, or a placeholder.
pub fn effort_label(current: Option<&str>) -> String {
    current.map_or_else(|| EFFORT_PLACEHOLDER.to_owned(), effort_title)
}

/// The reply a model pick sends.
pub fn model_text(id: &str) -> String {
    format!("/model {id}")
}

/// The reply an effort pick sends.
pub fn effort_text(level: &str) -> String {
    format!("/effort {level}")
}

/// A pick shown at once, before the next reply's line carries it (the Swift `modelHint` and
/// `effortHint`): kept while the transcript still says what it said when the pick was made, and
/// for [`HINT_MS`] at most.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub picked: String,
    pub was: Option<String>,
    pub at_ms: i64,
}

/// How long a pick stands in for the transcript (the Swift 30 s).
pub const HINT_MS: i64 = 30_000;

impl Hint {
    pub fn new(picked: &str, fact: Option<&str>, at_ms: i64) -> Hint {
        Hint {
            picked: picked.to_owned(),
            was: fact.map(str::to_owned),
            at_ms,
        }
    }

    /// What to show now: the pick while the fact is unchanged and the pick is fresh, else the fact.
    pub fn shown<'a>(
        hint: Option<&'a Hint>,
        fact: Option<&'a str>,
        now_ms: i64,
    ) -> Option<&'a str> {
        match hint {
            Some(h) if h.was.as_deref() == fact && now_ms - h.at_ms <= HINT_MS => {
                Some(h.picked.as_str())
            }
            _ => fact,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notchling_engine::control::slash::{route, Route};

    #[test]
    fn the_choices_are_the_swift_lists_in_order() {
        let names: Vec<String> = models(None).into_iter().map(|r| r.title).collect();
        assert_eq!(names, ["Opus 5.5", "Fable 5.2", "Sonnet 5.5", "Haiku 5.5"]);
        let older: Vec<String> = older_models(None).into_iter().map(|r| r.title).collect();
        assert_eq!(
            older,
            [
                "Haiku 4.5",
                "Sonnet 5",
                "Opus 5",
                "Fable 5.1",
                "Fable 5",
                "Opus 4.8",
                "Opus 4.7",
                "Opus 4.6",
                "Sonnet 4.6"
            ]
        );
        let levels: Vec<String> = efforts(None).into_iter().map(|r| r.title).collect();
        assert_eq!(levels, ["Low", "Medium", "High", "Extra High", "Max"]);
    }

    #[test]
    fn the_current_model_is_checked_by_its_name() {
        let rows = models(Some("claude-sonnet-5-5"));
        let on: Vec<&str> = rows
            .iter()
            .filter(|r| r.selected)
            .map(|r| r.title.as_str())
            .collect();
        assert_eq!(on, ["Sonnet 5.5"]);
        // An id that is on neither list checks nothing.
        assert!(models(Some("claude-opus-9-9")).iter().all(|r| !r.selected));
        assert!(models(None).iter().all(|r| !r.selected));
        // An older model is checked under More Models only.
        assert!(models(Some("claude-opus-4-8")).iter().all(|r| !r.selected));
        assert_eq!(
            older_models(Some("claude-opus-4-8"))
                .iter()
                .filter(|r| r.selected)
                .count(),
            1
        );
    }

    #[test]
    fn the_current_effort_is_checked() {
        let rows = efforts(Some("xhigh"));
        assert_eq!(
            rows.iter().find(|r| r.selected).map(|r| r.title.as_str()),
            Some("Extra High")
        );
        assert!(efforts(None).iter().all(|r| !r.selected));
        assert!(efforts(Some("ultra")).iter().all(|r| !r.selected));
    }

    #[test]
    fn labels_name_the_current_value_or_the_placeholder() {
        assert_eq!(model_label(Some("claude-opus-5-5")), "Opus 5.5");
        assert_eq!(model_label(None), "Model");
        assert_eq!(effort_label(Some("medium")), "Medium");
        assert_eq!(effort_label(Some("xhigh")), "Extra High");
        assert_eq!(effort_label(None), "Effort");
    }

    #[test]
    fn a_pick_sends_the_command_with_its_value() {
        assert_eq!(model_text("claude-fable-5-2"), "/model claude-fable-5-2");
        assert_eq!(effort_text("xhigh"), "/effort xhigh");
        // Every choice carries an argument, so the engine types it into the session as a reply and
        // never opens a terminal for it.
        for row in models(None).into_iter().chain(older_models(None)) {
            assert_eq!(route(&model_text(row.value)), Route::Session);
        }
        for row in efforts(None) {
            assert_eq!(route(&effort_text(row.value)), Route::Session);
        }
        // The bare command, which a pick never sends, would.
        assert_eq!(route("/effort"), Route::Terminal);
    }

    #[test]
    fn a_pick_shows_until_the_transcript_changes() {
        let hint = Hint::new("high", Some("medium"), 1_000);
        assert_eq!(
            Hint::shown(Some(&hint), Some("medium"), 2_000),
            Some("high")
        );
        // The next reply carried the new level, or another change: the fact wins.
        assert_eq!(Hint::shown(Some(&hint), Some("high"), 2_000), Some("high"));
        assert_eq!(Hint::shown(Some(&hint), Some("max"), 2_000), Some("max"));
        assert_eq!(Hint::shown(None, Some("max"), 2_000), Some("max"));
        // A pick the session never took stops standing in after 30 s.
        assert_eq!(
            Hint::shown(Some(&hint), Some("medium"), 1_000 + HINT_MS + 1),
            Some("medium")
        );
        // From no fact at all.
        let first = Hint::new("low", None, 0);
        assert_eq!(Hint::shown(Some(&first), None, 1), Some("low"));
        assert_eq!(Hint::shown(None, None, 1), None);
    }

    #[test]
    fn kinds_parse_from_the_render_switch() {
        assert_eq!(Kind::parse("model"), Some(Kind::Model));
        assert_eq!(Kind::parse("effort"), Some(Kind::Effort));
        assert_eq!(Kind::parse("1"), None);
    }
}
