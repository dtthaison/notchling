use notchling_locale as t;
use std::sync::OnceLock;

use notchling_engine::catalog::{model_key, Catalog, ModelInfo, Tier, Tool};

use super::format::model_name;

fn built_in() -> &'static [ModelInfo] {
    static BUILT_IN: OnceLock<Vec<ModelInfo>> = OnceLock::new();
    BUILT_IN.get_or_init(|| Catalog::built_in().models(Tool::Claude, &[]))
}

pub fn or_built_in(catalog: &[ModelInfo]) -> &[ModelInfo] {
    if catalog.is_empty() {
        built_in()
    } else {
        catalog
    }
}

pub fn find<'a>(catalog: &'a [ModelInfo], id: &str) -> Option<&'a ModelInfo> {
    let key = model_key(id);
    or_built_in(catalog)
        .iter()
        .find(|m| model_key(&m.id) == key)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Model,
    Effort,
}

impl Kind {
    pub fn parse(value: &str) -> Option<Kind> {
        match value {
            "model" => Some(Kind::Model),
            "effort" => Some(Kind::Effort),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub value: String,
    pub title: String,
    pub tip: String,
    pub selected: bool,
}

pub fn effort_title(level: &str) -> String {
    if level == "xhigh" {
        return t::chat::effort_extra_high().to_owned();
    }
    let mut chars = level.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

fn title_of(model: &ModelInfo) -> String {
    model.name.clone()
}

fn borrowed_tip(model: &ModelInfo) -> String {
    match model.borrowed_from.as_deref().filter(|_| model.borrowed) {
        Some(from) => t::chat::priced_as(&model_name(from)),
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

pub fn models(current: Option<&str>, catalog: &[ModelInfo]) -> Vec<Row> {
    model_rows(catalog, Tier::Current, current)
}

pub fn older_models(current: Option<&str>, catalog: &[ModelInfo]) -> Vec<Row> {
    model_rows(catalog, Tier::Older, current)
}

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

pub fn model_label(current: Option<&str>, catalog: &[ModelInfo]) -> String {
    current.map_or_else(
        || t::chat::model_placeholder().to_owned(),
        |id| find(catalog, id).map_or_else(|| model_name(id), title_of),
    )
}

pub fn effort_label(current: Option<&str>) -> String {
    current.map_or_else(|| t::chat::effort_placeholder().to_owned(), effort_title)
}

pub fn model_text(id: &str) -> String {
    super::markers::model_command(id)
}

pub fn effort_text(level: &str) -> String {
    super::markers::effort_command(level)
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Hint {
    pub picked: String,
    pub was: Option<String>,
    pub at_ms: i64,
}

pub const HINT_MS: i64 = 30_000;

impl Hint {
    pub fn new(picked: &str, fact: Option<&str>, at_ms: i64) -> Hint {
        Hint {
            picked: picked.to_owned(),
            was: fact.map(str::to_owned),
            at_ms,
        }
    }

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
        assert!(models(Some("claude-opus-9-9"), &catalog)
            .iter()
            .all(|r| !r.selected));
        assert!(models(None, &catalog).iter().all(|r| !r.selected));
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
        assert_eq!(model_label(Some("claude-fable-5-3"), &catalog), "Fable 5.3");
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
        assert_eq!(route("/effort"), Route::Terminal);
    }

    #[test]
    fn a_pick_shows_until_the_transcript_changes() {
        let hint = Hint::new("high", Some("medium"), 1_000);
        assert_eq!(
            Hint::shown(Some(&hint), Some("medium"), 2_000),
            Some("high")
        );
        assert_eq!(Hint::shown(Some(&hint), Some("high"), 2_000), Some("high"));
        assert_eq!(Hint::shown(Some(&hint), Some("max"), 2_000), Some("max"));
        assert_eq!(Hint::shown(None, Some("max"), 2_000), Some("max"));
        assert_eq!(
            Hint::shown(Some(&hint), Some("medium"), 1_000 + HINT_MS + 1),
            Some("medium")
        );
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
