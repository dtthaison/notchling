struct Parts {
    scheme: String,
    host: String,
    port: Option<u16>,
}

fn url_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c)
}

fn parse(text: &str) -> Option<Parts> {
    let text = text.trim();
    if text.is_empty() || !text.chars().all(url_char) {
        return None;
    }
    let full = if text.contains("://") {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    let (scheme, rest) = full.split_once("://")?;
    let scheme_ok = scheme
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic())
        && scheme
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-.".contains(c));
    if !scheme_ok {
        return None;
    }
    let authority = rest.split(['/', '?', '#']).next().unwrap_or("");
    let authority = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    let (host, port) = if let Some(inner) = authority.strip_prefix('[') {
        let (v6, after) = inner.split_once(']')?;
        (format!("[{v6}]"), after.strip_prefix(':'))
    } else {
        match authority.split_once(':') {
            Some((h, p)) => (h.to_string(), Some(p)),
            None => (authority.to_string(), None),
        }
    };
    let port = match port {
        None | Some("") => None,
        Some(p) if p.chars().all(|c| c.is_ascii_digit()) => Some(p.parse::<u16>().ok()?),
        Some(_) => return None,
    };
    Some(Parts {
        scheme: scheme.to_lowercase(),
        host: host.to_lowercase(),
        port,
    })
}

pub fn host_of(text: &str) -> Option<String> {
    let trimmed = text.trim();
    if !trimmed.contains("://") && !trimmed.contains('.') {
        return None;
    }
    let host = parse(trimmed)?.host;
    if host.is_empty() {
        return None;
    }
    Some(
        host.strip_prefix("www.")
            .map(str::to_string)
            .unwrap_or(host),
    )
}

pub fn origin_of(text: &str) -> Option<String> {
    let host = host_of(text)?;
    let parts = parse(text)?;
    let default = match parts.scheme.as_str() {
        "https" => Some(443),
        "http" => Some(80),
        _ => None,
    };
    let mut origin = format!("{}://{host}", parts.scheme);
    if let Some(port) = parts.port.filter(|p| Some(*p) != default) {
        origin.push_str(&format!(":{port}"));
    }
    Some(origin)
}

pub fn renew_url(url: Option<&str>) -> Option<String> {
    let text = url?.trim();
    if text.is_empty() {
        return None;
    }
    if let Some(host) = host_of(text) {
        if host == "github.com" || host.ends_with(".github.com") {
            return Some("https://github.com/settings/personal-access-tokens".into());
        }
    }
    let full = if text.contains("://") {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    let parts = parse(&full)?;
    if !matches!(parts.scheme.as_str(), "http" | "https") || parts.host.is_empty() {
        return None;
    }
    Some(full)
}

pub fn renew_refusal(url: Option<&str>) -> Option<String> {
    let text = url?.trim();
    if text.is_empty() || renew_url(Some(text)).is_some() {
        return None;
    }
    Some(text.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hosts() {
        assert_eq!(
            host_of("https://www.Example.com/x").as_deref(),
            Some("example.com")
        );
        assert_eq!(host_of("github"), None);
        assert_eq!(host_of("api.github.com").as_deref(), Some("api.github.com"));
        assert_eq!(host_of("publish to registry.npmjs.org"), None, "spaces");
        assert_eq!(host_of("https://"), None);
        assert_eq!(
            host_of("https://me@host.io:8443/p").as_deref(),
            Some("host.io")
        );
        assert_eq!(host_of("x.io:notaport"), None);
    }

    #[test]
    fn origins() {
        assert_eq!(
            origin_of("https://github.com:443").as_deref(),
            Some("https://github.com")
        );
        assert_eq!(
            origin_of("http://localhost:8080/x").as_deref(),
            Some("http://localhost:8080")
        );
        assert_eq!(
            origin_of("github.com/v3").as_deref(),
            Some("https://github.com")
        );
    }
}
