//! `AppURL`: `agentconsole://open[?phones=1]`, how anything asks the app to come to the front, and
//! with `phones=1` to tell every connected phone to open too.

pub const SCHEME: &str = "agentconsole";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Open { phones: bool },
}

/// `None` for anything that is not a known action. Unknown parameters are ignored.
pub fn parse(url: &str) -> Option<Action> {
    let (scheme, rest) = url.split_once("://")?;
    if !scheme.eq_ignore_ascii_case(SCHEME) {
        return None;
    }
    let end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..end];
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    if !host.eq_ignore_ascii_case("open") {
        return None;
    }
    let after = &rest[end..];
    let query = after
        .split_once('?')
        .map(|(_, q)| q.split('#').next().unwrap_or(""))
        .unwrap_or("");
    let phones = query
        .split('&')
        .filter_map(|pair| pair.split_once('=').or(Some((pair, ""))))
        .find(|(name, _)| *name == "phones")
        .is_some_and(|(_, value)| value == "1");
    Some(Action::Open { phones })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_open_and_open_with_phones() {
        assert_eq!(
            parse("agentconsole://open"),
            Some(Action::Open { phones: false })
        );
        assert_eq!(
            parse("agentconsole://open?phones=1"),
            Some(Action::Open { phones: true })
        );
        assert_eq!(
            parse("AgentConsole://OPEN?phones=1"),
            Some(Action::Open { phones: true }),
            "case-insensitive"
        );
        assert_eq!(
            parse("agentconsole://open?phones=0"),
            Some(Action::Open { phones: false })
        );
        assert_eq!(
            parse("agentconsole://open?phones=1&x=2&utm=3"),
            Some(Action::Open { phones: true }),
            "unknown parameters are ignored"
        );
    }

    #[test]
    fn junk_is_rejected() {
        for text in [
            "agentconsole://close",
            "agentconsole://",
            "agentconsole:open",
            "https://open?phones=1",
            "other://open",
            "agentconsole://evil.com/open",
        ] {
            assert_eq!(parse(text), None, "{text}");
        }
    }
}
