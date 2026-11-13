#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
    pub fragment: Option<String>,
}

impl Url {
    pub fn parse(text: &str) -> Option<Url> {
        if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return None;
        }
        let (scheme, rest) = text.split_once("://")?;
        let scheme_ok = !scheme.is_empty()
            && scheme.starts_with(|c: char| c.is_ascii_alphabetic())
            && scheme
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'));
        if !scheme_ok {
            return None;
        }
        let (rest, fragment) = match rest.split_once('#') {
            Some((head, tail)) => (head, Some(tail.to_string())),
            None => (rest, None),
        };
        let (rest, query) = match rest.split_once('?') {
            Some((head, tail)) => (head, Some(tail.to_string())),
            None => (rest, None),
        };
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        let host_port = authority.rsplit('@').next().unwrap_or_default();
        let (host, port) = split_host_port(host_port)?;
        if host.is_empty() {
            return None;
        }
        Some(Url {
            scheme: scheme.to_ascii_lowercase(),
            host: host.to_string(),
            port,
            path: path.to_string(),
            query,
            fragment,
        })
    }

    pub fn parse_local(text: &str) -> Option<Url> {
        let url = Url::parse(text)?;
        let web = url.scheme == "http" || url.scheme == "https";
        (web && (url.host == "localhost" || url.host == "127.0.0.1")).then_some(url)
    }

    pub fn is_http(&self) -> bool {
        self.scheme == "http" || self.scheme == "https"
    }

    pub fn effective_port(&self) -> u16 {
        self.port
            .unwrap_or(if self.scheme == "https" { 443 } else { 80 })
    }

    pub fn origin(&self) -> String {
        format!("{}://{}:{}", self.scheme, self.host, self.effective_port())
    }

    pub fn render(&self) -> String {
        let mut text = format!("{}://{}", self.scheme, self.host);
        if let Some(port) = self.port {
            text.push_str(&format!(":{port}"));
        }
        text.push_str(&self.path);
        if let Some(query) = &self.query {
            text.push('?');
            text.push_str(query);
        }
        if let Some(fragment) = &self.fragment {
            text.push('#');
            text.push_str(fragment);
        }
        text
    }
}

fn split_host_port(text: &str) -> Option<(&str, Option<u16>)> {
    if let Some(inner) = text.strip_prefix('[') {
        let (host, after) = inner.split_once(']')?;
        return match after {
            "" => Some((host, None)),
            _ => Some((host, parse_port(after.strip_prefix(':')?)?)),
        };
    }
    match text.rsplit_once(':') {
        Some((host, port)) => Some((host, parse_port(port)?)),
        None => Some((text, None)),
    }
}

fn parse_port(text: &str) -> Option<Option<u16>> {
    if text.is_empty() {
        return Some(None);
    }
    if !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>().ok().map(Some)
}
