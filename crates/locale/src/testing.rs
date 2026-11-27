use crate::format::{format_with, Arg, Body, Cat, Numbers, Part, Table};
use crate::generated::{KEYS, TABLES};
use crate::numbers::Icu;

pub(crate) struct PlainNumbers(pub(crate) Icu);

impl Numbers for PlainNumbers {
    fn int(&self, n: i64) -> String {
        n.to_string()
    }

    fn category(&self, n: i64) -> Cat {
        self.0.category(n)
    }
}

fn bodies(tag: &str) -> Vec<Option<Body>> {
    match TABLES.iter().find(|(t, _)| *t == tag) {
        Some((_, Table::Full(all))) => all.iter().copied().map(Some).collect(),
        Some((_, Table::Part(all))) => all.to_vec(),
        None => Vec::new(),
    }
}

fn mark(parts: &[Part], numeric: &mut Vec<bool>) {
    for part in parts {
        let (at, num) = match part {
            Part::Lit(_) => continue,
            Part::Str(i) => (*i as usize, false),
            Part::Int(i) => (*i as usize, true),
        };
        if numeric.len() <= at {
            numeric.resize(at + 1, false);
        }
        numeric[at] |= num;
    }
}

pub(crate) fn numeric_of(body: &Body) -> Vec<bool> {
    let mut numeric = Vec::new();
    match body {
        Body::Plain(_) => {}
        Body::Parts(parts) => mark(parts, &mut numeric),
        Body::Plural(selector, variants) => {
            for (_, parts) in variants.iter() {
                mark(parts, &mut numeric);
            }
            let at = *selector as usize;
            if numeric.len() <= at {
                numeric.resize(at + 1, false);
            }
            numeric[at] = true;
        }
    }
    numeric
}

fn name_of(cat: Cat) -> &'static str {
    match cat {
        Cat::Zero => "zero",
        Cat::One => "one",
        Cat::Two => "two",
        Cat::Few => "few",
        Cat::Many => "many",
        Cat::Other => "other",
    }
}

pub fn messages(tag: &str) -> Vec<(&'static str, Vec<bool>)> {
    bodies(tag)
        .iter()
        .enumerate()
        .filter_map(|(i, body)| body.as_ref().map(|b| (KEYS[i].0, numeric_of(b))))
        .collect()
}

pub fn plural_categories(tag: &str) -> Vec<(&'static str, Vec<&'static str>)> {
    bodies(tag)
        .iter()
        .enumerate()
        .filter_map(|(i, body)| match body {
            Some(Body::Plural(_, variants)) => Some((
                KEYS[i].0,
                variants.iter().map(|(c, _)| name_of(*c)).collect(),
            )),
            _ => None,
        })
        .collect()
}

pub fn reachable_categories(tag: &str) -> Vec<&'static str> {
    let icu = Icu::new(tag, None);
    let mut found: Vec<&'static str> = Vec::new();
    for n in (0..=200).chain([1000, 1_000_000]) {
        let name = name_of(icu.category(n));
        if !found.contains(&name) {
            found.push(name);
        }
    }
    found
}

pub fn format_plain_numbers(tag: &str, key: &str, args: &[Arg<'_>]) -> Option<String> {
    let at = KEYS.binary_search_by(|(k, _)| (*k).cmp(key)).ok()?;
    let body = bodies(tag).get(at).copied().flatten()?;
    Some(format_with(
        &body,
        args,
        &PlainNumbers(Icu::new(tag, None)),
        false,
    ))
}
