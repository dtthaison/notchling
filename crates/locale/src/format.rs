use crate::generated::{EN, KEYS, TABLES};
use crate::{pseudo, state};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Text(pub(crate) u16);

impl Text {
    pub fn get(self) -> &'static str {
        state::current().get(self)
    }

    pub fn english(self) -> &'static str {
        match EN.get(self.0 as usize) {
            Some(Body::Plain(text)) => text,
            _ => "",
        }
    }

    pub fn key(self) -> &'static str {
        KEYS.get(self.0 as usize).map(|(key, _)| *key).unwrap_or("")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Num(pub(crate) i64);

macro_rules! num_from {
    ($($ty:ty),*) => {
        $(impl From<$ty> for Num {
            fn from(value: $ty) -> Num {
                Num(i64::from(value))
            }
        })*
    };
}

macro_rules! num_from_wide {
    ($($ty:ty),*) => {
        $(impl From<$ty> for Num {
            fn from(value: $ty) -> Num {
                Num(i64::try_from(value).unwrap_or(i64::MAX))
            }
        })*
    };
}

num_from!(i8, i16, i32, i64, u8, u16, u32);
num_from_wide!(u64, usize);

impl From<isize> for Num {
    fn from(value: isize) -> Num {
        Num(i64::try_from(value).unwrap_or(if value < 0 { i64::MIN } else { i64::MAX }))
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Arg<'a> {
    Str(&'a str),
    Num(Num),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Cat {
    Zero,
    One,
    Two,
    Few,
    Many,
    Other,
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Part {
    Lit(&'static str),
    Str(u8),
    Int(u8),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Body {
    Plain(&'static str),
    Parts(&'static [Part]),
    Plural(u8, &'static [(Cat, &'static [Part])]),
}

#[allow(dead_code)]
#[derive(Clone, Copy, Debug)]
pub(crate) enum Table {
    Full(&'static [Body]),
    Part(&'static [Option<Body>]),
}

pub(crate) trait Numbers {
    fn int(&self, n: i64) -> String;
    fn category(&self, n: i64) -> Cat;
}

#[cfg(test)]
pub(crate) fn format_body(body: &Body, args: &[Arg<'_>], numbers: &dyn Numbers) -> String {
    format_with(body, args, numbers, false)
}

pub(crate) fn format_with(
    body: &Body,
    args: &[Arg<'_>],
    numbers: &dyn Numbers,
    accent: bool,
) -> String {
    match body {
        Body::Plain(text) => (*text).to_string(),
        Body::Parts(parts) => render_parts(parts, args, numbers, accent),
        Body::Plural(selector, variants) => {
            let n = match args.get(*selector as usize) {
                Some(Arg::Num(n)) => n.0,
                _ => 0,
            };
            let cat = numbers.category(n);
            let chosen = variants
                .iter()
                .find(|(c, _)| *c == cat)
                .or_else(|| variants.iter().find(|(c, _)| *c == Cat::Other))
                .or_else(|| variants.first());
            match chosen {
                Some((_, parts)) => render_parts(parts, args, numbers, accent),
                None => String::new(),
            }
        }
    }
}

fn render_parts(parts: &[Part], args: &[Arg<'_>], numbers: &dyn Numbers, accent: bool) -> String {
    let mut out = String::new();
    for part in parts {
        match part {
            Part::Lit(text) if accent => out.push_str(&pseudo::accent(text)),
            Part::Lit(text) => out.push_str(text),
            Part::Str(i) => match args.get(*i as usize) {
                Some(Arg::Str(text)) => out.push_str(text),
                Some(Arg::Num(n)) => out.push_str(&n.0.to_string()),
                None => {}
            },
            Part::Int(i) => match args.get(*i as usize) {
                Some(Arg::Num(n)) => out.push_str(&numbers.int(n.0)),
                Some(Arg::Str(text)) => out.push_str(text),
                None => {}
            },
        }
    }
    out
}

pub fn format(text: Text, args: &[Arg<'_>]) -> String {
    state::current().format(text, args)
}

pub fn lookup(key: &str) -> Option<Text> {
    let at = KEYS.binary_search_by(|(k, _)| (*k).cmp(key)).ok()?;
    match EN.get(at) {
        Some(Body::Plain(_)) => Some(KEYS[at].1),
        _ => None,
    }
}

pub fn shipped() -> Vec<&'static str> {
    TABLES.iter().map(|(tag, _)| *tag).collect()
}
