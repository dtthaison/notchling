use notchling_engine::catalog::ModelInfo;
use notchling_engine::tools::agy::ConfigOption;
use notchling_locale as t;

use super::claude_picker;
use crate::tools::codex_picker::{CodexPicker, DEFAULT_TITLE, NOTE as CODEX_NOTE, NO_MODELS};

pub const MODEL: &str = "model";
pub const EFFORT: &str = "effort";

pub const RESET: &str = "";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Model,
    Effort,
    Other,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub value: String,
    pub title: String,
    pub detail: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    pub id: String,
    pub glyph: Glyph,
    pub label: String,
    pub tip: String,
    pub rows: Vec<Row>,
    pub more: Vec<Row>,
    pub note: Option<String>,
    pub empty: Option<String>,
    pub enabled: bool,
}

impl Menu {
    pub fn part(&self) -> String {
        match self.glyph {
            Glyph::Model => "Model Picker".to_owned(),
            Glyph::Effort => "Effort Picker".to_owned(),
            Glyph::Other => format!("{} Picker", title_case(&self.label_name())),
        }
    }

    pub fn menu_part(&self) -> String {
        self.part().replace(" Picker", " Menu")
    }

    fn label_name(&self) -> String {
        self.id.replace(['_', '-'], " ")
    }
}

fn title_case(text: &str) -> String {
    text.split_whitespace()
        .map(|w| {
            let mut c = w.chars();
            match c.next() {
                Some(f) => f.to_uppercase().chain(c).collect::<String>(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn claude_rows(rows: Vec<claude_picker::Row>) -> Vec<Row> {
    rows.into_iter()
        .map(|r| Row {
            value: r.value.to_owned(),
            title: r.title,
            detail: r.tip,
            selected: r.selected,
        })
        .collect()
}

pub fn claude(model: Option<&str>, effort: Option<&str>, catalog: &[ModelInfo]) -> Vec<Menu> {
    let in_use = model.and_then(|id| claude_picker::find(catalog, id));
    let efforts = claude_picker::efforts(effort, in_use);
    let no_levels = efforts.is_empty().then(|| t::chat::no_efforts().into());
    vec![
        Menu {
            id: MODEL.into(),
            glyph: Glyph::Model,
            label: claude_picker::model_label(model, catalog),
            tip: t::chat::model_tip().into(),
            rows: claude_rows(claude_picker::models(model, catalog)),
            more: claude_rows(claude_picker::older_models(model, catalog)),
            note: None,
            empty: None,
            enabled: true,
        },
        Menu {
            id: EFFORT.into(),
            glyph: Glyph::Effort,
            label: claude_picker::effort_label(effort),
            tip: t::chat::effort_tip().into(),
            rows: claude_rows(efforts),
            more: Vec::new(),
            note: None,
            empty: no_levels,
            enabled: true,
        },
    ]
}

pub fn codex(picker: &CodexPicker) -> Vec<Menu> {
    let rows = picker.rows();
    let mut models = vec![Row {
        value: RESET.into(),
        title: DEFAULT_TITLE.into(),
        detail: String::new(),
        selected: picker.is_default(),
    }];
    models.extend(rows.iter().map(|m| Row {
        value: m.slug.clone(),
        title: m.name.clone(),
        detail: m.summary.clone(),
        selected: m.selected && !picker.is_default(),
    }));
    let current = rows.iter().find(|m| m.selected);
    let model_label = current.map_or_else(
        || {
            picker
                .running_model
                .clone()
                .or_else(|| picker.pick.model.clone())
                .unwrap_or_else(|| t::chat::default_model().to_owned())
        },
        |m| m.name.clone(),
    );
    let default_effort = current
        .and_then(|m| picker.models.iter().find(|x| x.slug == m.slug))
        .and_then(|m| m.default_effort.clone());
    let effort_now = picker
        .pick
        .effort
        .clone()
        .or_else(|| picker.running_effort.clone())
        .or(default_effort);
    let efforts: Vec<Row> = current
        .map(|m| {
            m.efforts
                .iter()
                .map(|e| Row {
                    value: e.effort.clone(),
                    title: e.title.clone(),
                    detail: String::new(),
                    selected: effort_now.as_deref() == Some(e.effort.as_str()),
                })
                .collect()
        })
        .unwrap_or_default();
    let effort_label = efforts
        .iter()
        .find(|e| e.selected)
        .map(|e| e.title.clone())
        .or_else(|| {
            picker
                .pick
                .effort
                .clone()
                .or_else(|| picker.running_effort.clone())
                .map(|e| claude_picker::effort_title(&e))
        })
        .unwrap_or_else(|| t::chat::effort_placeholder().to_owned());
    vec![
        Menu {
            id: MODEL.into(),
            glyph: Glyph::Model,
            label: model_label,
            tip: t::chat::codex_model_tip().into(),
            rows: if picker.models.is_empty() {
                Vec::new()
            } else {
                models
            },
            more: Vec::new(),
            note: Some(CODEX_NOTE.into()),
            empty: Some(NO_MODELS.into()),
            enabled: true,
        },
        Menu {
            id: EFFORT.into(),
            glyph: Glyph::Effort,
            label: effort_label,
            tip: t::chat::codex_effort_tip().into(),
            enabled: !efforts.is_empty(),
            rows: efforts,
            more: Vec::new(),
            note: Some(CODEX_NOTE.into()),
            empty: Some(t::chat::pick_model_first().into()),
        },
    ]
}

pub fn antigravity(options: &[ConfigOption], started: bool, loading: bool) -> Vec<Menu> {
    let why = if !started {
        Some(t::chat::agy_not_started())
    } else if loading || options.is_empty() {
        Some(t::chat::agy_opening())
    } else {
        None
    };
    let menu = |o: &ConfigOption| {
        let (id, glyph) = match o.id.as_str() {
            "model" => (MODEL.to_owned(), Glyph::Model),
            "thought_level" => (EFFORT.to_owned(), Glyph::Effort),
            other => (other.to_owned(), Glyph::Other),
        };
        Menu {
            id,
            glyph,
            label: o.current_name().to_owned(),
            tip: t::chat::agy_option_tip(&o.name),
            rows: o
                .values
                .iter()
                .map(|v| Row {
                    value: v.value.clone(),
                    title: v.name.clone(),
                    detail: String::new(),
                    selected: v.value == o.current,
                })
                .collect(),
            more: Vec::new(),
            note: Some(crate::tools::agy_options::NOTE.into()),
            empty: None,
            enabled: true,
        }
    };
    if let Some(why) = why {
        let off = |id: &str, glyph, label: &str| Menu {
            id: id.into(),
            glyph,
            label: label.into(),
            tip: why.into(),
            rows: Vec::new(),
            more: Vec::new(),
            note: None,
            empty: None,
            enabled: false,
        };
        return vec![
            off(MODEL, Glyph::Model, t::chat::model_placeholder()),
            off(EFFORT, Glyph::Effort, t::chat::effort_placeholder()),
        ];
    }
    let mut menus: Vec<Menu> = options.iter().map(menu).collect();
    menus.sort_by_key(|m| match m.glyph {
        Glyph::Model => 0,
        Glyph::Effort => 1,
        Glyph::Other => 2,
    });
    menus
}

pub fn id_of(kind: claude_picker::Kind) -> &'static str {
    match kind {
        claude_picker::Kind::Model => MODEL,
        claude_picker::Kind::Effort => EFFORT,
    }
}

pub fn agy_option(menu: &str) -> &str {
    match menu {
        MODEL => "model",
        EFFORT => "thought_level",
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::requests::Pick;
    use notchling_engine::tools::agy::ConfigValue;
    use notchling_engine::tools::codex::models::Model;
    use notchling_engine::transcript::model::SessionFacts;

    fn option(id: &str, name: &str, current: &str, values: &[(&str, &str)]) -> ConfigOption {
        ConfigOption {
            id: id.into(),
            name: name.into(),
            current: current.into(),
            values: values
                .iter()
                .map(|(v, n)| ConfigValue {
                    value: (*v).into(),
                    name: (*n).into(),
                })
                .collect(),
        }
    }

    #[test]
    fn every_tool_has_a_model_and_an_effort_picker_with_the_same_parts() {
        let claude = claude(Some("claude-opus-5-5"), Some("high"), &[]);
        let codex = codex(&CodexPicker::default());
        let agy = antigravity(&[], false, false);
        for menus in [&claude, &codex, &agy] {
            assert_eq!(menus[0].id, MODEL);
            assert_eq!(menus[0].part(), "Model Picker");
            assert_eq!(menus[1].id, EFFORT);
            assert_eq!(menus[1].part(), "Effort Picker");
            assert_eq!(menus[1].menu_part(), "Effort Menu");
        }
        assert_eq!(claude[0].label, "Opus 5.5");
        assert_eq!(claude[1].label, "High");
        assert!(!claude[0].more.is_empty());
    }

    #[test]
    fn claude_menus_follow_the_catalog_and_the_model_in_use() {
        use notchling_engine::catalog::{Catalog, LiveModel, Tool as CatalogTool};
        let live = LiveModel {
            id: "claude-fable-5-3".into(),
            name: Some("Claude Fable 5.3".into()),
            ..LiveModel::default()
        };
        let catalog = Catalog::built_in().models(CatalogTool::Claude, &[live]);
        let menus = claude(Some("claude-fable-5-3"), Some("high"), &catalog);
        let fable = menus[0].rows.iter().find(|r| r.title == "Fable 5.3");
        assert!(fable.is_some_and(|r| r.detail.contains("Fable 5.2")));
        assert_eq!(menus[0].label, "Fable 5.3");
        assert_eq!(menus[1].rows.len(), 5);
        assert!(menus[1].empty.is_none());
        let menus = claude(Some("claude-haiku-4-5"), None, &catalog);
        assert!(menus[1].rows.is_empty());
        assert_eq!(menus[1].empty.as_deref(), Some(t::chat::no_efforts()));
        assert_eq!(claude(None, None, &[])[0].rows.len(), 4);
    }

    #[test]
    fn codex_lists_its_default_then_its_models_and_the_efforts_of_the_one_in_use() {
        let models = vec![Model {
            slug: "gpt-6.1-sol".into(),
            name: "GPT-6.1 Sol".into(),
            summary: "Frontier".into(),
            efforts: vec!["medium".into(), "high".into()],
            default_effort: Some("medium".into()),
            context_window: None,
        }];
        let ran = SessionFacts {
            model: Some("gpt-6.1-sol".into()),
            effort: Some("high".into()),
            ..SessionFacts::default()
        };
        let picker = CodexPicker::new(models.clone(), Pick::default(), &ran);
        let menus = codex(&picker);
        assert_eq!(menus[0].label, "GPT-6.1 Sol");
        assert_eq!(menus[0].rows[0].value, RESET);
        assert!(menus[0].rows[0].selected, "nothing picked: Codex Default");
        assert_eq!(menus[0].rows[1].detail, "Frontier");
        assert_eq!(menus[1].rows.len(), 2);
        assert!(menus[1].enabled);
        assert_eq!(menus[1].label, "High");
        let empty = codex(&CodexPicker::new(Vec::new(), Pick::default(), &ran));
        assert!(empty[0].rows.is_empty());
        assert_eq!(empty[0].label, "gpt-6.1-sol");
        assert!(!empty[1].enabled);
    }

    #[test]
    fn antigravity_maps_its_options_onto_model_effort_and_its_own_menus() {
        let options = vec![
            option(
                "mode",
                "Mode",
                "auto",
                &[("auto", "Auto"), ("plan", "Plan")],
            ),
            option(
                "model",
                "Model",
                "gemini-4-argon",
                &[
                    ("gemini-4-argon", "Gemini 4 Argon"),
                    ("gemini-3", "Gemini 3"),
                ],
            ),
            option(
                "thought_level",
                "Thinking Level",
                "medium",
                &[("medium", "Medium")],
            ),
        ];
        let menus = antigravity(&options, true, false);
        let ids: Vec<&str> = menus.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, [MODEL, EFFORT, "mode"]);
        assert_eq!(menus[0].label, "Gemini 4 Argon");
        assert_eq!(menus[2].part(), "Mode Picker");
        assert!(menus[0].rows[0].selected);
        assert_eq!(agy_option(EFFORT), "thought_level");
        let draft = antigravity(&options, false, false);
        assert!(draft.iter().all(|m| !m.enabled));
        assert_eq!(draft[0].tip, t::chat::agy_not_started());
    }
}
