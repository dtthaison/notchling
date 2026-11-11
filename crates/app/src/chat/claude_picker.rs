//! The Model and Effort pickers under a Claude Code session's reply box (the Swift `ModelPicker`
//! and `EffortPicker`): the choices, the labels, which one is current, and the text a pick sends.
//! A pick is an ordinary reply, `/model <full id>` or `/effort <level>`, typed into the session as
//! in the terminal; with a value, neither goes to a terminal (`slash::route` only sends the bare
//! command there). Pure, so every system tests it.

use std::sync::OnceLock;

use notchling_engine::catalog::{model_key, Catalog, ModelInfo, Tier, Tool};

use super::format::model_name;

/// The catalog's own Claude models, parsed once: what Chat shows until the daemon has answered
/// (or when there is no daemon), so a picker is never empty.
fn built_in() -> &'static [ModelInfo] {
    static BUILT_IN: OnceLock<Vec<ModelInfo>> = OnceLock::new();
    BUILT_IN.get_or_init(|| Catalog::built_in().models(Tool::Claude, &[]))
}

/// `catalog` (the daemon's list), or the built-in one while that is empty.
pub fn or_built_in(catalog: &[ModelInfo]) -> &[ModelInfo] {
    if catalog.is_empty() {
        built_in()
    } else {
        catalog
    }
}

/// The catalog's entry for a model id as the transcript names it (a `[1m]` or date suffix is the
/// same model).
pub fn find<'a>(catalog: &'a [ModelInfo], id: &str) -> Option<&'a ModelInfo> {
    let key = model_key(id);
    or_built_in(catalog)
        .iter()
        .find(|m| model_key(&m.id) == key)
}

/// What the model button says before the transcript has named one.
pub const MODEL_PLACEHOLDER: &str = "Model";
/// What the effort button says before any reply has carried one.
pub const EFFORT_PLACEHOLDER: &str = "Effort";
/// The tooltips, the Swift `.help` texts.
pub const MODEL_TIP: &str =
    "The model this session uses. Pick another to switch it (like /model in the terminal).";
pub const EFFORT_TIP: &str = "How hard the model thinks. Pick another to switch it (like /effort in the terminal, which also makes it the default for new sessions).";
/// Said in the Effort menu for a model that has no levels.
pub const NO_EFFORTS: &str = "This model has no effort levels";
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
    pub value: String,
    pub title: String,
    /// A line about the row, or empty: a borrowed price says so here.
    pub tip: String,
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

/// The row's name: the catalog's, without the "Claude " Anthropic's own list puts in front.
fn title_of(model: &ModelInfo) -> String {
    model
        .name
        .strip_prefix("Claude ")
        .unwrap_or(&model.name)
        .to_owned()
}

/// What a row says when its price and window are another model's.
fn borrowed_tip(model: &ModelInfo) -> String {
    match model.borrowed_from.as_deref().filter(|_| model.borrowed) {
        Some(from) => format!(
            "Priced as {} until Notchling knows this model",
            model_name(from)
        ),
        None => String::new(),
    }
}

fn model_rows(catalog: &[ModelInfo], tier: Tier, current: Option<&str>) -> Vec<Row> {
    let shown = current.map(model_key);
    or_built_in(catalog)
        .iter()
        .filter(|m| m.tier == tier)
        .map(|m| Row {
            value: m.id.clone(),
            title: title_of(m),
            tip: borrowed_tip(m),
            selected: shown.as_deref() == Some(model_key(&m.id).as_str()),
        })
        .collect()
}

/// The menu's first section: the current models.
pub fn models(current: Option<&str>, catalog: &[ModelInfo]) -> Vec<Row> {
    model_rows(catalog, Tier::Current, current)
}

/// Under More Models.
pub fn older_models(current: Option<&str>, catalog: &[ModelInfo]) -> Vec<Row> {
    model_rows(catalog, Tier::Older, current)
}

/// The effort menu: the levels `model` offers, or those of the first built-in model that has any
/// while the model is unknown (nothing picked yet, or an id the catalog doesn't list). A model
/// with no levels (Haiku 4.5) has no rows.
pub fn efforts(current: Option<&str>, model: Option<&ModelInfo>) -> Vec<Row> {
    let levels = match model {
        Some(m) => m.efforts.as_slice(),
        None => built_in()
            .iter()
            .find(|m| !m.efforts.is_empty())
            .map_or(&[][..], |m| m.efforts.as_slice()),
    };
    levels
        .iter()
        .map(|level| Row {
            value: level.clone(),
            title: effort_title(level),
            tip: String::new(),
            selected: current == Some(level.as_str()),
        })
        .collect()
}

/// The model button: the name (the catalog's, else made from the id), or a placeholder.
pub fn model_label(current: Option<&str>, catalog: &[ModelInfo]) -> String {
    current.map_or_else(
        || MODEL_PLACEHOLDER.to_owned(),
        |id| find(catalog, id).map_or_else(|| model_name(id), title_of),
    )
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

    use notchling_engine::catalog::LiveModel;

    fn built_in_list() -> Vec<ModelInfo> {
        Catalog::built_in().models(Tool::Claude, &[])
    }

    /// The built-in catalog with Anthropic's list naming a Fable 5.3, as the daemon would serve it.
    fn with_fable_5_3() -> Vec<ModelInfo> {
        let live = LiveModel {
            id: "claude-fable-5-3".into(),
            name: Some("Claude Fable 5.3".into()),
            ..LiveModel::default()
        };
        Catalog::built_in().models(Tool::Claude, &[live])
    }

    fn titles(rows: Vec<Row>) -> Vec<String> {
        rows.into_iter().map(|r| r.title).collect()
    }

    #[test]
    fn the_choices_come_from_the_catalog_in_order() {
        let catalog = built_in_list();
        assert_eq!(
            titles(models(None, &catalog)),
            ["Opus 5.5", "Fable 5.2", "Sonnet 5.5", "Haiku 5.5"]
        );
        assert_eq!(
            titles(older_models(None, &catalog)),
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
        let opus = find(&catalog, "claude-opus-5-5");
        let levels = titles(efforts(None, opus));
        assert_eq!(levels, ["Low", "Medium", "High", "Extra High", "Max"]);
    }

    #[test]
    fn a_live_model_takes_its_family_place_and_the_old_one_goes_under_more_models() {
        let catalog = with_fable_5_3();
        assert_eq!(
            titles(models(None, &catalog)),
            ["Opus 5.5", "Fable 5.3", "Sonnet 5.5", "Haiku 5.5"]
        );
        assert_eq!(titles(older_models(None, &catalog))[0], "Fable 5.2");
    }

    #[test]
    fn a_borrowed_model_says_so_in_its_tip() {
        let catalog = with_fable_5_3();
        let rows = models(None, &catalog);
        let fable = rows.iter().find(|r| r.title == "Fable 5.3").unwrap();
        assert_eq!(fable.value, "claude-fable-5-3");
        assert!(fable.tip.contains("Fable 5.2"), "{}", fable.tip);
        assert!(fable.tip.contains("until Notchling knows"), "{}", fable.tip);
        // A model the catalog knows says nothing.
        assert!(rows
            .iter()
            .filter(|r| r.title != "Fable 5.3")
            .all(|r| r.tip.is_empty()));
        assert!(older_models(None, &catalog)
            .iter()
            .all(|r| r.tip.is_empty()));
    }

    #[test]
    fn an_empty_catalog_falls_back_to_the_built_in_one() {
        // The app has not heard from the daemon yet: never an empty picker.
        assert_eq!(models(None, &[]), models(None, &built_in_list()));
        assert_eq!(
            older_models(None, &[]),
            older_models(None, &built_in_list())
        );
        assert_eq!(models(None, &[]).len(), 4);
        assert_eq!(model_label(Some("claude-opus-5-5"), &[]), "Opus 5.5");
        assert_eq!(efforts(None, None).len(), 5);
    }

    #[test]
    fn a_model_without_efforts_has_no_levels() {
        let catalog = built_in_list();
        let haiku = find(&catalog, "claude-haiku-4-5");
        assert!(haiku.is_some_and(|m| m.efforts.is_empty()));
        assert!(efforts(Some("high"), haiku).is_empty());
    }

    #[test]
    fn the_current_model_is_checked_by_its_id() {
        let catalog = built_in_list();
        let rows = models(Some("claude-sonnet-5-5"), &catalog);
        let on: Vec<&str> = rows
            .iter()
            .filter(|r| r.selected)
            .map(|r| r.title.as_str())
            .collect();
        assert_eq!(on, ["Sonnet 5.5"]);
        // A [1m] or date suffix is the same model.
        for id in ["claude-sonnet-5-5[1m]", "claude-sonnet-5-5-20261001"] {
            assert_eq!(
                models(Some(id), &catalog)
                    .iter()
                    .filter(|r| r.selected)
                    .count(),
                1,
                "{id}"
            );
        }
        // An id that is on neither list checks nothing.
        assert!(models(Some("claude-opus-9-9"), &catalog)
            .iter()
            .all(|r| !r.selected));
        assert!(models(None, &catalog).iter().all(|r| !r.selected));
        // An older model is checked under More Models only.
        assert!(models(Some("claude-opus-4-8"), &catalog)
            .iter()
            .all(|r| !r.selected));
        assert_eq!(
            older_models(Some("claude-opus-4-8"), &catalog)
                .iter()
                .filter(|r| r.selected)
                .count(),
            1
        );
    }

    #[test]
    fn the_current_effort_is_checked() {
        let catalog = built_in_list();
        let opus = find(&catalog, "claude-opus-5-5");
        let rows = efforts(Some("xhigh"), opus);
        assert_eq!(
            rows.iter().find(|r| r.selected).map(|r| r.title.as_str()),
            Some("Extra High")
        );
        assert!(efforts(None, opus).iter().all(|r| !r.selected));
        assert!(efforts(Some("ultra"), opus).iter().all(|r| !r.selected));
    }

    #[test]
    fn labels_name_the_current_value_or_the_placeholder() {
        let catalog = with_fable_5_3();
        assert_eq!(model_label(Some("claude-opus-5-5"), &catalog), "Opus 5.5");
        // Anthropic's own "Claude " is left off, as the rows do.
        assert_eq!(model_label(Some("claude-fable-5-3"), &catalog), "Fable 5.3");
        // An id the catalog doesn't list is named from the id.
        assert_eq!(model_label(Some("claude-opus-9-9"), &catalog), "Opus 9.9");
        assert_eq!(model_label(None, &catalog), "Model");
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
        let catalog = with_fable_5_3();
        for row in models(None, &catalog)
            .into_iter()
            .chain(older_models(None, &catalog))
        {
            assert_eq!(route(&model_text(&row.value)), Route::Session);
        }
        for row in efforts(None, None) {
            assert_eq!(route(&effort_text(&row.value)), Route::Session);
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
