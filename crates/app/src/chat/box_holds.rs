use notchling_locale as t;
use serde_json::Value;

pub const SHOWN_MAX: usize = 160;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoxHolds {
    pub row: String,
    pub text: Option<String>,
}

impl BoxHolds {
    pub fn from_data(row: &str, data: Option<&Value>) -> Option<BoxHolds> {
        let data = data?;
        if data.get("why").and_then(Value::as_str) != Some("not_cleared") {
            return None;
        }
        let text = data
            .get("box")
            .and_then(Value::as_str)
            .filter(|text| !text.trim().is_empty())
            .map(str::to_owned);
        Some(BoxHolds {
            row: row.to_owned(),
            text,
        })
    }

    pub fn line(&self) -> String {
        match &self.text {
            Some(text) => t::chat::box_holds(&one_line(text)),
            None => t::chat::box_holds_unread().to_owned(),
        }
    }
}

pub fn one_line(text: &str) -> String {
    let joined = text.split_whitespace().collect::<Vec<_>>().join(" ");
    match joined.char_indices().nth(SHOWN_MAX) {
        Some((end, _)) => format!("{}…", joined[..end].trim_end()),
        None => joined,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_not_cleared_failure_names_the_text_the_box_holds() {
        let data = json!({"why": "not_cleared", "box": "my own draft"});
        assert_eq!(
            BoxHolds::from_data("job-a", Some(&data)),
            Some(BoxHolds {
                row: "job-a".into(),
                text: Some("my own draft".into())
            })
        );
    }

    #[test]
    fn a_box_the_daemon_could_not_read_is_still_one_to_clear() {
        let data = json!({"why": "not_cleared"});
        let held = BoxHolds::from_data("job-a", Some(&data)).unwrap();
        assert_eq!(held.text, None);
        assert_eq!(held.line(), t::chat::box_holds_unread());
        let blank = json!({"why": "not_cleared", "box": "  \n "});
        assert_eq!(
            BoxHolds::from_data("job-a", Some(&blank)).unwrap().text,
            None
        );
    }

    #[test]
    fn any_other_failure_is_not_one() {
        assert_eq!(BoxHolds::from_data("job-a", None), None);
        assert_eq!(BoxHolds::from_data("job-a", Some(&json!({}))), None);
        assert_eq!(
            BoxHolds::from_data("job-a", Some(&json!({"why": "not_sent", "box": "x"}))),
            None
        );
        assert_eq!(
            BoxHolds::from_data("job-a", Some(&json!("not_cleared"))),
            None
        );
    }

    #[test]
    fn the_text_is_shown_on_one_line_and_cut_when_long() {
        assert_eq!(
            one_line("a draft\n  over\ttwo lines "),
            "a draft over two lines"
        );
        let long = "word ".repeat(100);
        let shown = one_line(&long);
        assert!(shown.ends_with('…'));
        assert_eq!(shown.chars().count(), SHOWN_MAX);
        let held = BoxHolds {
            row: "job-a".into(),
            text: Some("a draft".into()),
        };
        assert_eq!(
            held.line(),
            "The session's input box holds other text, so nothing was sent: a draft"
        );
    }
}
