use notchling_engine::tools::codex::models::Model;
use notchling_engine::transcript::model::SessionFacts;

use super::requests::Pick;

pub const NO_MODELS: &str = "Codex hasn't listed its models yet";
pub const NOTE: &str = "Applies from the next turn.";
pub const DEFAULT_TITLE: &str = "Codex Default";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EffortRow {
    pub effort: String,
    pub title: String,
    pub selected: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModelRow {
    pub slug: String,
    pub name: String,
    pub summary: String,
    pub efforts: Vec<EffortRow>,
    pub selected: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CodexPicker {
    pub models: Vec<Model>,
    pub pick: Pick,
    pub running_model: Option<String>,
    pub running_effort: Option<String>,
}

fn capitalized(word: &str) -> String {
    let mut chars = word.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().chain(chars).collect(),
        None => String::new(),
    }
}

impl CodexPicker {
    pub fn new(models: Vec<Model>, pick: Pick, facts: &SessionFacts) -> CodexPicker {
        CodexPicker {
            models,
            pick,
            running_model: facts.model.clone(),
            running_effort: facts.effort.clone(),
        }
    }

    fn effective_model(&self) -> Option<&str> {
        self.pick.model.as_deref().or(self.running_model.as_deref())
    }

    fn model_named(&self, slug: &str) -> Option<&Model> {
        self.models.iter().find(|m| m.slug == slug)
    }

    pub fn label(&self) -> String {
        let name = match self.effective_model() {
            Some(slug) => self
                .model_named(slug)
                .map_or_else(|| slug.to_owned(), |m| m.name.clone()),
            None => "Default Model".to_owned(),
        };
        let mut parts = vec!["Codex".to_owned(), name];
        if let Some(effort) = self
            .pick
            .effort
            .as_deref()
            .or(self.running_effort.as_deref())
        {
            parts.push(effort.to_owned());
        }
        parts.join(" · ")
    }

    pub fn is_default(&self) -> bool {
        self.pick == Pick::default()
    }

    pub fn rows(&self) -> Vec<ModelRow> {
        let current = self.effective_model();
        self.models
            .iter()
            .map(|m| {
                let selected = current == Some(m.slug.as_str());
                let effort_now = self.pick.effort.as_deref().or(m.default_effort.as_deref());
                ModelRow {
                    slug: m.slug.clone(),
                    name: m.name.clone(),
                    summary: m.summary.clone(),
                    efforts: m
                        .efforts
                        .iter()
                        .map(|e| EffortRow {
                            effort: e.clone(),
                            title: if m.default_effort.as_deref() == Some(e.as_str()) {
                                format!("{} (Default)", capitalized(e))
                            } else {
                                capitalized(e)
                            },
                            selected: selected && effort_now == Some(e.as_str()),
                        })
                        .collect(),
                    selected,
                }
            })
            .collect()
    }

    pub fn choose_model(&mut self, slug: &str) {
        let keep = self.pick.effort.take().filter(|e| {
            self.model_named(slug)
                .is_some_and(|m| m.efforts.contains(e))
        });
        self.pick = Pick {
            model: Some(slug.to_owned()),
            effort: keep,
        };
    }

    pub fn choose_effort(&mut self, slug: &str, effort: &str) {
        self.pick = Pick {
            model: Some(slug.to_owned()),
            effort: Some(effort.to_owned()),
        };
    }

    pub fn reset(&mut self) {
        self.pick = Pick::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn models() -> Vec<Model> {
        vec![
            Model {
                slug: "gpt-6.1-sol".into(),
                name: "GPT-6.1 Sol".into(),
                summary: "The big one".into(),
                efforts: vec!["low".into(), "medium".into(), "high".into()],
                default_effort: Some("medium".into()),
                context_window: None,
            },
            Model {
                slug: "gpt-6.1-mini".into(),
                name: "GPT-6.1 Mini".into(),
                summary: String::new(),
                efforts: vec!["low".into(), "medium".into()],
                default_effort: Some("low".into()),
                context_window: None,
            },
        ]
    }

    #[test]
    fn the_label_names_the_pick_else_the_last_turn() {
        let mut p = CodexPicker::new(models(), Pick::default(), &SessionFacts::default());
        assert_eq!(p.label(), "Codex · Default Model");
        assert!(p.is_default());
        let ran = SessionFacts {
            model: Some("gpt-6.1-mini".into()),
            effort: Some("low".into()),
            ..SessionFacts::default()
        };
        p = CodexPicker::new(models(), Pick::default(), &ran);
        assert_eq!(p.label(), "Codex · GPT-6.1 Mini · low");
        assert!(p.rows()[1].selected && p.rows()[1].efforts[0].selected);
        p.choose_effort("gpt-6.1-sol", "high");
        assert_eq!(p.label(), "Codex · GPT-6.1 Sol · high");
        assert!(!p.is_default());
        let rows = p.rows();
        assert!(rows[0].selected && !rows[1].selected);
        assert_eq!(rows[0].efforts[1].title, "Medium (Default)");
        assert!(rows[0].efforts[2].selected);
        let other = CodexPicker::new(
            models(),
            Pick {
                model: Some("gpt-7".into()),
                effort: None,
            },
            &SessionFacts::default(),
        );
        assert_eq!(other.label(), "Codex · gpt-7");
    }

    #[test]
    fn a_new_model_keeps_the_effort_only_when_it_offers_it() {
        let mut p = CodexPicker::new(models(), Pick::default(), &SessionFacts::default());
        p.choose_effort("gpt-6.1-sol", "high");
        p.choose_model("gpt-6.1-mini");
        assert_eq!(p.pick.effort, None, "mini has no high");
        assert!(p.rows()[1].efforts[0].selected, "so its default shows");
        p.choose_effort("gpt-6.1-mini", "medium");
        p.choose_model("gpt-6.1-sol");
        assert_eq!(p.pick.effort.as_deref(), Some("medium"));
        p.reset();
        assert!(p.is_default());
        assert_eq!(p.label(), "Codex · Default Model");
    }
}
