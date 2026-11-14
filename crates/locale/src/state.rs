use std::sync::OnceLock;

use crate::choose::Choice;
use crate::format::{format_with, Arg, Body, Table, Text};
use crate::generated::TABLES;
use crate::numbers::Icu;
use crate::pseudo;

type Tables = &'static [(&'static str, Table)];

pub(crate) struct Active {
    tag: &'static str,
    tables: Tables,
    chain: Vec<usize>,
    pseudo: bool,
    mark_fallback: bool,
    numbers: Icu,
    cache: Box<[OnceLock<&'static str>]>,
}

impl Active {
    pub(crate) fn new(choice: &Choice) -> Active {
        Active::with_tables(choice, &TABLES)
    }

    pub(crate) fn with_tables(choice: &Choice, tables: Tables) -> Active {
        let index_of = |tag: &str| tables.iter().position(|(t, _)| *t == tag);
        let mut chain = Vec::new();
        if let Some(chosen) = index_of(choice.tag) {
            chain.push(chosen);
        }
        if let Some((parent, _)) = choice.tag.rsplit_once('-') {
            if let Some(at) = index_of(parent) {
                chain.push(at);
            }
        }
        if !chain.contains(&0) {
            chain.push(0);
        }
        let transforms = choice.pseudo || choice.mark_fallback;
        let size = match tables.first() {
            Some((_, Table::Full(bodies))) if transforms => bodies.len(),
            Some((_, Table::Part(bodies))) if transforms => bodies.len(),
            _ => 0,
        };
        let cache = (0..size).map(|_| OnceLock::new()).collect();
        Active {
            tag: choice.tag,
            tables,
            chain,
            pseudo: choice.pseudo,
            mark_fallback: choice.mark_fallback,
            numbers: Icu::new(choice.tag, choice.region.as_deref()),
            cache,
        }
    }

    pub(crate) fn tag(&self) -> &'static str {
        self.tag
    }

    pub(crate) fn numbers(&self) -> &Icu {
        &self.numbers
    }

    fn find(&self, text: Text) -> Option<(Body, bool)> {
        let id = text.0 as usize;
        for (pos, index) in self.chain.iter().enumerate() {
            let found = match self.tables.get(*index) {
                Some((_, Table::Full(bodies))) => bodies.get(id).copied(),
                Some((_, Table::Part(bodies))) => bodies.get(id).copied().flatten(),
                None => None,
            };
            if let Some(body) = found {
                return Some((body, pos > 0));
            }
        }
        None
    }

    pub(crate) fn get(&self, text: Text) -> &'static str {
        let Some((body, fell)) = self.find(text) else {
            return "";
        };
        let Body::Plain(plain) = body else {
            return "";
        };
        let marked = self.mark_fallback && fell;
        if !self.pseudo && !marked {
            return plain;
        }
        let build = || -> &'static str {
            let mut out = if self.pseudo {
                pseudo::transform(plain)
            } else {
                plain.to_string()
            };
            if marked {
                out = format!("\u{2039}{out}\u{203a}");
            }
            Box::leak(out.into_boxed_str())
        };
        match self.cache.get(text.0 as usize) {
            Some(slot) => slot.get_or_init(build),
            None => build(),
        }
    }

    pub(crate) fn format(&self, text: Text, args: &[Arg<'_>]) -> String {
        let Some((body, fell)) = self.find(text) else {
            return String::new();
        };
        let mut out = format_with(&body, args, &self.numbers, self.pseudo);
        if self.pseudo {
            out = pseudo::wrap(&out);
        }
        if self.mark_fallback && fell {
            out = format!("\u{2039}{out}\u{203a}");
        }
        out
    }
}

static ACTIVE: OnceLock<Active> = OnceLock::new();
static ENGLISH: OnceLock<Active> = OnceLock::new();

pub(crate) fn current() -> &'static Active {
    match ACTIVE.get() {
        Some(active) => active,
        None => ENGLISH.get_or_init(|| {
            Active::new(&Choice {
                tag: "en",
                pseudo: false,
                region: None,
                mark_fallback: false,
            })
        }),
    }
}

pub(crate) fn set(choice: &Choice) -> bool {
    ACTIVE.set(Active::new(choice)).is_ok()
}
