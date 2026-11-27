const SUFFIX: &str = ".ts.net";

pub fn same_name(a: &str, b: &str) -> bool {
    a.trim().to_lowercase() == b.trim().to_lowercase()
}

pub fn short_host(host: &str) -> &str {
    if !host.to_ascii_lowercase().ends_with(SUFFIX) {
        return host;
    }
    match host.split('.').next() {
        Some(first) if !first.is_empty() && first.len() < host.len() => first,
        _ => host,
    }
}
