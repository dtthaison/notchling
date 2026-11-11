//! The pickers under the reply box, one design for every tool (docs/chat-composer-parity.md):
//! each tool says which menus its session has (a Model menu, an Effort menu, and any other choice
//! the tool offers, such as Antigravity's Mode), and Chat draws every one the same way, a button
//! with an icon, the current choice and a chevron that opens a menu of rows. A new tool (Grok)
//! adds one function here that returns its menus, and the picks it understands in
//! `ChatView::pick`; nothing in the drawing changes. Pure, so every system tests it.

use notchling_engine::tools::agy::ConfigOption;

use super::claude_picker;
use crate::tools::codex_picker::{CodexPicker, DEFAULT_TITLE, NOTE as CODEX_NOTE, NO_MODELS};

/// The menu ids every tool uses for its model and its effort; other choices use the tool's own id.
pub const MODEL: &str = "model";
pub const EFFORT: &str = "effort";

/// A Codex row that goes back to Codex's own defaults.
pub const RESET: &str = "";

/// Which icon the button carries.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Glyph {
    Model,
    Effort,
    Other,
}

/// One choice in a menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// What a pick hands the tool: a model id, a level, an option value; [`RESET`] for a default.
    pub value: String,
    pub title: String,
    /// A line under the title (Codex's model summary), or empty.
    pub detail: String,
    pub selected: bool,
}

/// One picker: its button and its menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Menu {
    /// [`MODEL`], [`EFFORT`] or the tool's own id for another choice.
    pub id: String,
    pub glyph: Glyph,
    /// What the button says: the current choice.
    pub label: String,
    /// The button's hover text.
    pub tip: String,
    pub rows: Vec<Row>,
    /// Rows under a More Models row (older Claude models); empty for none.
    pub more: Vec<Row>,
    /// Under the rows ("Applies from the next turn.").
    pub note: Option<String>,
    /// Said in place of the rows when there are none.
    pub empty: Option<String>,
    /// False: the button shows, disabled, and the tip says why.
    pub enabled: bool,
}

impl Menu {
    /// The button's part name: "Model Picker", "Effort Picker", "Mode Picker".
    pub fn part(&self) -> String {
        match self.glyph {
            Glyph::Model => "Model Picker".to_owned(),
            Glyph::Effort => "Effort Picker".to_owned(),
            Glyph::Other => format!("{} Picker", title_case(&self.label_name())),
        }
    }

    /// The menu's part name: "Model Menu", "Effort Menu", "Mode Menu".
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
            detail: String::new(),
            selected: r.selected,
        })
        .collect()
}

/// A Claude Code session: its model and effort, picked as `/model` and `/effort` replies.
pub fn claude(model: Option<&str>, effort: Option<&str>) -> Vec<Menu> {
    vec![
        Menu {
            id: MODEL.into(),
            glyph: Glyph::Model,
            label: claude_picker::model_label(model),
            tip: claude_picker::MODEL_TIP.into(),
            rows: claude_rows(claude_picker::models(model)),
            more: claude_rows(claude_picker::older_models(model)),
            note: None,
            empty: None,
            enabled: true,
        },
        Menu {
            id: EFFORT.into(),
            glyph: Glyph::Effort,
            label: claude_picker::effort_label(effort),
            tip: claude_picker::EFFORT_TIP.into(),
            rows: claude_rows(claude_picker::efforts(effort)),
            more: Vec::new(),
            note: None,
            empty: None,
            enabled: true,
        },
    ]
}

const CODEX_MODEL_TIP: &str =
    "The model this Codex session's next turns use. Codex Default follows Codex's own setting.";
const CODEX_EFFORT_TIP: &str = "How hard the model reasons on this session's next turns.";

/// A Codex session: its models (Codex's own list, Codex Default first) and the efforts the model
/// in use offers. A pick rides on the next turn.
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
                .unwrap_or_else(|| "Default Model".to_owned())
        },
        |m| m.name.clone(),
    );
    // The effort the next turn uses: the pick, else the last turn's, else the model's default.
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
        .unwrap_or_else(|| claude_picker::EFFORT_PLACEHOLDER.to_owned());
    vec![
        Menu {
            id: MODEL.into(),
            glyph: Glyph::Model,
            label: model_label,
            tip: CODEX_MODEL_TIP.into(),
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
            tip: CODEX_EFFORT_TIP.into(),
            enabled: !efforts.is_empty(),
            rows: efforts,
            more: Vec::new(),
            note: Some(CODEX_NOTE.into()),
            empty: Some("Pick a model first.".into()),
        },
    ]
}

const AGY_NOT_STARTED: &str =
    "Your first message starts the session; then this can be picked here.";
const AGY_LOADING: &str = "Antigravity is still opening this session.";

/// An Antigravity session: each option the session offers, its model as the Model menu, its
/// thinking level as the Effort menu, and any other (Mode) as a menu of its own. Before the
/// session starts, or while it loads, the Model and Effort buttons show disabled and say why.
pub fn antigravity(options: &[ConfigOption], started: bool, loading: bool) -> Vec<Menu> {
    let why = if !started {
        Some(AGY_NOT_STARTED)
    } else if loading || options.is_empty() {
        Some(AGY_LOADING)
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
            tip: format!("{} for this session's next turns.", o.name),
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
            off(MODEL, Glyph::Model, claude_picker::MODEL_PLACEHOLDER),
            off(EFFORT, Glyph::Effort, claude_picker::EFFORT_PLACEHOLDER),
        ];
    }
    // Model first, then effort, then the rest in the tool's order.
    let mut menus: Vec<Menu> = options.iter().map(menu).collect();
    menus.sort_by_key(|m| match m.glyph {
        Glyph::Model => 0,
        Glyph::Effort => 1,
        Glyph::Other => 2,
    });
    menus
}

/// The menu id of a render's forced picker.
pub fn id_of(kind: claude_picker::Kind) -> &'static str {
    match kind {
        claude_picker::Kind::Model => MODEL,
        claude_picker::Kind::Effort => EFFORT,
    }
}

/// The Antigravity option id a menu id stands for.
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
        let claude = claude(Some("claude-opus-5-5"), Some("high"));
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
    fn codex_lists_its_default_then_its_models_and_the_efforts_of_the_one_in_use() {
        let models = vec![Model {
            slug: "gpt-6.1-sol".into(),
            name: "GPT-6.1 Sol".into(),
            summary: "Frontier".into(),
            efforts: vec!["medium".into(), "high".into()],
            default_effort: Some("medium".into()),
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
        assert_eq!(draft[0].tip, AGY_NOT_STARTED);
    }
}
