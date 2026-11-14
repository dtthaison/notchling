use crate::format::Numbers;
use crate::numbers::Icu;
use crate::{units, Num};

pub fn count(n: impl Into<Num>) -> String {
    crate::format::numbers().int(n.into().0)
}

pub fn decimal(value: f64, digits: u8) -> String {
    decimal_with(crate::format::numbers(), value, digits)
}

pub(crate) fn decimal_with(icu: &Icu, value: f64, digits: u8) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    let digits = digits.min(9);
    let scaled = (value * 10f64.powi(i32::from(digits))).round() as i64;
    icu.decimal(scaled, -i16::from(digits))
}

pub fn list<S: AsRef<str>>(items: &[S]) -> String {
    join(items, units::list_separator())
}

pub(crate) fn join<S: AsRef<str>>(items: &[S], separator: &str) -> String {
    let mut out = String::new();
    for (i, item) in items.iter().enumerate() {
        if i > 0 {
            out.push_str(separator);
        }
        out.push_str(item.as_ref());
    }
    out
}
