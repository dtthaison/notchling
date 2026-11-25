use super::model::Timestamp;
use serde_json::{Map, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn now_secs() -> Timestamp {
    system_time_secs(SystemTime::now()).unwrap_or(0.0)
}

pub fn system_time_secs(t: SystemTime) -> Option<Timestamp> {
    t.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs_f64())
}

pub fn file_times(path: &str) -> Option<(Timestamp, Timestamp)> {
    let meta = std::fs::metadata(path).ok()?;
    let modified = system_time_secs(meta.modified().ok()?)?;
    let created = meta
        .created()
        .ok()
        .and_then(system_time_secs)
        .unwrap_or(modified);
    Some((modified, created))
}

pub fn modified_secs(path: &str) -> Option<Timestamp> {
    file_times(path).map(|t| t.0)
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn digits(s: &[u8], from: usize, n: usize) -> Option<i64> {
    let part = s.get(from..from + n)?;
    if !part.iter().all(u8::is_ascii_digit) {
        return None;
    }
    std::str::from_utf8(part).ok()?.parse().ok()
}

pub fn parse_date(text: &str) -> Option<Timestamp> {
    let b = text.as_bytes();
    if b.len() < 20
        || b[4] != b'-'
        || b[7] != b'-'
        || !(b[10] == b'T' || b[10] == b' ')
        || b[13] != b':'
        || b[16] != b':'
    {
        return None;
    }
    let (y, mo, d) = (digits(b, 0, 4)?, digits(b, 5, 2)?, digits(b, 8, 2)?);
    let (h, mi, sec) = (digits(b, 11, 2)?, digits(b, 14, 2)?, digits(b, 17, 2)?);
    let mut i = 19;
    let mut fraction = 0.0;
    if b.get(i) == Some(&b'.') {
        let start = i + 1;
        i = start;
        while i < b.len() && b[i].is_ascii_digit() {
            i += 1;
        }
        if i == start {
            return None;
        }
        fraction = format!("0.{}", &text[start..i]).parse().ok()?;
    }
    let offset = match b.get(i)? {
        b'Z' if i + 1 == b.len() => 0,
        sign @ (b'+' | b'-') => {
            let hh = digits(b, i + 1, 2)?;
            let colon = usize::from(b.get(i + 3) == Some(&b':'));
            let mm = digits(b, i + 3 + colon, 2)?;
            if i + 3 + colon + 2 != b.len() {
                return None;
            }
            let secs = hh * 3600 + mm * 60;
            if *sign == b'+' {
                secs
            } else {
                -secs
            }
        }
        _ => return None,
    };
    let days = days_from_civil(y, mo, d);
    Some((days * 86_400 + h * 3600 + mi * 60 + sec - offset) as f64 + fraction)
}

pub fn date_field(obj: &Value) -> Option<Timestamp> {
    obj.get("timestamp")
        .and_then(Value::as_str)
        .and_then(parse_date)
}

pub fn parse_object(line: &[u8]) -> Option<Map<String, Value>> {
    match serde_json::from_slice::<Value>(line).ok()? {
        Value::Object(map) => Some(map),
        _ => None,
    }
}

pub fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

pub fn count_of(v: &Value) -> u64 {
    match v {
        Value::Number(n) => n
            .as_u64()
            .or_else(|| n.as_f64().map(|f| f.max(0.0) as u64))
            .unwrap_or(0),
        _ => 0,
    }
}

pub fn int_of(v: &Value) -> Option<i64> {
    match v {
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f as i64)),
        _ => None,
    }
}

pub fn json_sorted(v: &Value, escape_slashes: bool) -> String {
    match v {
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            let parts: Vec<String> = keys
                .iter()
                .map(|k| {
                    format!(
                        "{}:{}",
                        json_sorted(&Value::String((*k).clone()), escape_slashes),
                        json_sorted(&map[*k], escape_slashes)
                    )
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        Value::Array(items) => {
            format!(
                "[{}]",
                items
                    .iter()
                    .map(|i| json_sorted(i, escape_slashes))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        }
        Value::String(s) => {
            let text = Value::String(s.clone()).to_string();
            if escape_slashes {
                text.replace('/', "\\/")
            } else {
                text
            }
        }
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_keys_and_escapes_slashes() {
        let v: Value =
            serde_json::from_str(r#"{"b":"a/b","a":[1,{"d":2,"c":null}]}"#).expect("json");
        assert_eq!(
            json_sorted(&v, false),
            r#"{"a":[1,{"c":null,"d":2}],"b":"a/b"}"#
        );
        assert_eq!(
            json_sorted(&v, true),
            r#"{"a":[1,{"c":null,"d":2}],"b":"a\/b"}"#
        );
    }

    #[test]
    fn parses_iso_dates() {
        assert_eq!(parse_date("1970-01-01T00:00:00Z"), Some(0.0));
        assert_eq!(
            parse_date("2026-11-01T10:00:02.500Z"),
            Some(1_793_527_202.5)
        );
        assert_eq!(
            parse_date("2026-11-01T17:00:02+07:00"),
            Some(1_793_527_202.0)
        );
        assert_eq!(
            parse_date("2026-11-05 16:57:03.335872+00:00").map(f64::floor),
            Some(1_793_897_823.0)
        );
        assert_eq!(parse_date("nope"), None);
        assert_eq!(parse_date("2026-11-01T10:00:02"), None);
    }
}
