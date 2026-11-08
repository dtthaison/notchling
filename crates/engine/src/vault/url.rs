//! The little URL reading the vault needs, as Foundation's `URL(string:)` gave the Swift app: a
//! host to match a task by, an origin to tell where a secret is sent, and the page Renew opens.
//! Text without a scheme is read as `https://`. Anything `URL(string:)` would refuse (spaces, a
//! port that isn't a number) has no host here either.

/// The parts of `scheme://[user@]host[:port][/path…]`.
struct Parts {
    scheme: String,
    host: String,
    port: Option<u16>,
}

/// Characters a URL may hold unescaped (RFC 3986's unreserved, reserved and `%`).
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

/// `api.github.com` from `https://api.github.com/v3` or `api.github.com`, without `www.`; `None`
/// for text that names no host (a bare word, or something that isn't a URL).
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

/// `https://api.github.com`, `http://localhost:8080`: scheme, host without `www.`, and the port
/// unless it is the scheme's default. `None` when it has no host.
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

/// The page Renew opens for a secret with this URL: GitHub's token settings for a GitHub secret,
/// else the URL itself. Only http(s) with a host: the URL comes from an agent, and `file:`,
/// `smb:` or an app's scheme must never be handed to the system to open.
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

/// The secret's URL when Renew won't open it (not http or https), to show as plain text.
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
