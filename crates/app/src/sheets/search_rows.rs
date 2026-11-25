use notchling_engine::search::query::category;
use notchling_engine::search::query::category::Category;
use notchling_locale::{self as t, Text};
use serde_json::{json, Value};

use super::settings::{choice, value, Field, Row};

pub const RANGE_KEY: &str = "searchIndexRange";
pub const CATEGORIES_KEY: &str = "searchCategories";
pub const ORDER_KEY: &str = "searchCategoryOrder";

pub const SHOWN: [Category; 3] = [Category::Sessions, Category::Pages, Category::Vault];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub category: Category,
    pub title: &'static str,
    pub on: bool,
    pub first: bool,
    pub last: bool,
}

const RANGES: [(&str, Text); 3] = [
    ("14d", t::settings::SEARCH_INDEX_RANGE_V14D),
    ("30d", t::settings::SEARCH_INDEX_RANGE_V30D),
    ("all", t::settings::SEARCH_INDEX_RANGE_ALL),
];

pub fn is_search_key(key: &str) -> bool {
    key == RANGE_KEY || key == CATEGORIES_KEY
}

pub fn title(category: Category) -> &'static str {
    match category {
        Category::Sessions => t::settings::search_category_sessions(),
        Category::Settings => t::settings::search_category_settings(),
        Category::Pages => t::settings::search_category_pages(),
        Category::Inbox => t::settings::search_category_inbox(),
        Category::Actions => t::settings::search_category_actions(),
        Category::Vault => t::settings::search_category_vault(),
        Category::Activity | Category::Machines => category.id(),
    }
}

fn stored(read: &dyn Fn(&str) -> Option<Value>, key: &str) -> Vec<String> {
    value(read, key)
        .and_then(|v| {
            v.as_array().map(|a| {
                a.iter()
                    .filter_map(|s| s.as_str().map(str::to_owned))
                    .collect()
            })
        })
        .unwrap_or_default()
}

pub fn categories(read: &dyn Fn(&str) -> Option<Value>) -> Vec<(Category, bool)> {
    let on = category::on(&stored(read, CATEGORIES_KEY));
    category::order(&stored(read, ORDER_KEY))
        .into_iter()
        .map(|c| (c, on.contains(&c)))
        .collect()
}

pub fn rows(read: &dyn Fn(&str) -> Option<Value>) -> Vec<Row> {
    vec![
        choice(read, RANGE_KEY, t::settings::search_index_range(), &RANGES),
        Row {
            key: CATEGORIES_KEY,
            title: t::settings::search_categories(),
            field: Field::Categories(categories(read)),
        },
    ]
}

pub fn lines(rows: &[(Category, bool)]) -> Vec<Line> {
    let shown: Vec<&(Category, bool)> = rows.iter().filter(|(c, _)| SHOWN.contains(c)).collect();
    shown
        .iter()
        .enumerate()
        .map(|(i, (category, on))| Line {
            category: *category,
            title: title(*category),
            on: *on,
            first: i == 0,
            last: i + 1 == shown.len(),
        })
        .collect()
}

pub fn toggle(rows: &[(Category, bool)], id: Category) -> Vec<String> {
    rows.iter()
        .filter(|(c, on)| *on != (*c == id))
        .map(|(c, _)| c.id().to_owned())
        .collect()
}

pub fn move_row(rows: &[(Category, bool)], id: Category, by: i32) -> Vec<String> {
    let mut order: Vec<Category> = rows.iter().map(|(c, _)| *c).collect();
    let shown: Vec<usize> = (0..order.len())
        .filter(|i| SHOWN.contains(&order[*i]))
        .collect();
    let slot = order
        .iter()
        .position(|c| *c == id)
        .and_then(|at| shown.iter().position(|i| *i == at));
    if let Some(slot) = slot {
        let to_slot = (slot as i64 + i64::from(by)).clamp(0, shown.len() as i64 - 1) as usize;
        let moved = order.remove(shown[slot]);
        order.insert(shown[to_slot], moved);
    }
    order.iter().map(|c| c.id().to_owned()).collect()
}

pub fn change(rows: &[(Category, bool)], option: &str) -> Option<(&'static str, Value)> {
    let (action, id) = option.split_once(':')?;
    let id =
        Category::from_id(id).filter(|c| SHOWN.contains(c) && rows.iter().any(|(r, _)| r == c))?;
    match action {
        "toggle" => Some((CATEGORIES_KEY, json!(toggle(rows, id)))),
        "up" => Some((ORDER_KEY, json!(move_row(rows, id, -1)))),
        "down" => Some((ORDER_KEY, json!(move_row(rows, id, 1)))),
        _ => None,
    }
}

#[cfg(test)]
#[path = "search_rows_tests.rs"]
mod tests;
