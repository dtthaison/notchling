use super::Tool;

pub const GAPS: &str = include_str!("gaps.json");
pub const UNWIRED: &str = include_str!("unwired.json");
pub const FOUNDING: [Tool; 3] = Tool::ALL;

pub fn entries(text: &str) -> Result<Vec<String>, String> {
    let value: serde_json::Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let list = value
        .get("entries")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "no entries list".to_string())?;
    list.iter()
        .map(|v| {
            v.as_str()
                .map(str::to_string)
                .ok_or_else(|| "an entry is not a string".to_string())
        })
        .collect()
}

pub fn render(entries: &[String]) -> String {
    let mut out = String::from("{\n \"entries\": [\n");
    for (i, e) in entries.iter().enumerate() {
        out.push_str("  \"");
        out.push_str(e);
        out.push('"');
        if i + 1 < entries.len() {
            out.push(',');
        }
        out.push('\n');
    }
    out.push_str(" ]\n}\n");
    out
}
