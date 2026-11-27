use std::path::Path;

use notchling_engine::search::query::category::{self, Category};
use notchling_engine::settings::Settings;

use super::model::{Item, Section, EMPTY_LIMIT, PAGE_CAP, SECRET_CAP};
use super::wire::Results;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prefs {
    pub order: Vec<Category>,
    pub on: Vec<Category>,
}

impl Default for Prefs {
    fn default() -> Prefs {
        Prefs {
            order: Category::ALL.to_vec(),
            on: Category::ALL
                .into_iter()
                .filter(|c| c.default_on())
                .collect(),
        }
    }
}

fn list(store: &Settings, key: &str) -> Option<Vec<String>> {
    store.get(key).and_then(|v| {
        v.as_array().map(|a| {
            a.iter()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        })
    })
}

impl Prefs {
    pub fn from_settings(store: &Settings) -> Prefs {
        let default = Prefs::default();
        Prefs {
            order: list(store, "searchCategoryOrder")
                .map(|ids| category::order(&ids))
                .unwrap_or(default.order),
            on: list(store, "searchCategories")
                .map(|ids| category::on(&ids))
                .unwrap_or(default.on),
        }
    }

    pub fn read(data_dir: &Path) -> Prefs {
        Settings::open(&data_dir.join("settings.json"))
            .map(|s| Prefs::from_settings(&s))
            .unwrap_or_default()
    }

    pub fn shows(&self, section: Section) -> bool {
        self.on.contains(&section.category())
    }

    pub fn corpora(&self) -> Vec<&'static str> {
        self.order
            .iter()
            .filter(|c| self.on.contains(c) && matches!(c, Category::Pages | Category::Sessions))
            .map(|c| c.id())
            .collect()
    }

    pub fn arrange(&self, results: Results, query_empty: bool) -> Vec<Item> {
        let page_cap = if query_empty { EMPTY_LIMIT } else { PAGE_CAP };
        let mut pages = Some(results.pages);
        let mut secrets = Some(results.secrets);
        let mut sessions = Some(results.sessions);
        let mut items = Vec::new();
        for category in &self.order {
            if !self.on.contains(category) {
                continue;
            }
            match category {
                Category::Pages => {
                    items.extend(
                        pages
                            .take()
                            .unwrap_or_default()
                            .into_iter()
                            .take(page_cap)
                            .map(Item::Page),
                    );
                }
                Category::Vault if !query_empty => {
                    items.extend(
                        secrets
                            .take()
                            .unwrap_or_default()
                            .into_iter()
                            .take(SECRET_CAP)
                            .map(Item::Secret),
                    );
                }
                Category::Sessions => {
                    items.extend(
                        sessions
                            .take()
                            .unwrap_or_default()
                            .into_iter()
                            .map(Item::Session),
                    );
                }
                _ => {}
            }
        }
        items
    }
}

#[cfg(test)]
#[path = "groups_tests.rs"]
mod tests;
