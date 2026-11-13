use super::registry::MachineRepo;
use super::shell;
use super::step::SubRow;
use serde_json::Value;

pub fn slug(github: &str) -> String {
    github
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

pub fn alias(github: &str) -> String {
    format!("github-agents-{}", slug(github))
}

pub fn key_file(github: &str) -> String {
    format!("~/.ssh/agents-{}", slug(github))
}

pub fn remote_url(github: &str) -> String {
    format!("git@{}:{github}.git", alias(github))
}

pub fn checkout_path(repo: &MachineRepo) -> String {
    if let Some(path) = &repo.machine_path {
        if !path.is_empty() {
            return path.clone();
        }
    }
    let name = repo
        .github
        .split('/')
        .rfind(|p| !p.is_empty())
        .unwrap_or(&repo.github);
    format!("~/src/{name}")
}

fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-'
}

pub fn is_valid_repo(github: &str) -> bool {
    let Some((owner, name)) = github.split_once('/') else {
        return false;
    };
    let mut o = owner.chars();
    let owner_ok = o.next().is_some_and(|c| c.is_ascii_alphanumeric()) && o.all(name_char);
    let mut n = name.chars();
    let name_ok = n
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
        && n.all(name_char)
        && name != "."
        && name != "..";
    owner_ok && name_ok
}

pub fn is_valid_machine_name(name: &str) -> bool {
    let mut c = name.chars();
    name.len() <= 64 && c.next().is_some_and(|c| c.is_ascii_alphanumeric()) && c.all(name_char)
}

pub fn alias_pattern(alias: &str) -> String {
    format!("^[[:space:]]*Host[[:space:]]+{alias}([[:space:]]|$)")
}

pub fn ends_in_replaced_key_date(name: &str) -> bool {
    let tail: Vec<char> = name.chars().rev().take(9).collect();
    tail.len() == 9 && tail[8] == '-' && tail[..8].iter().all(|c| c.is_ascii_digit())
}

fn is_b64(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '+' || c == '/'
}

fn after_body(rest: &str) -> Option<&str> {
    let body_len = rest.chars().take_while(|c| is_b64(*c)).count();
    if !(60..=80).contains(&body_len) {
        return None;
    }
    let after = &rest[body_len..];
    let pad = after.chars().take_while(|c| *c == '=').count();
    (pad <= 3).then(|| &after[pad..])
}

pub fn is_key_body_line(text: &str) -> bool {
    text.strip_prefix("ssh-ed25519 ")
        .and_then(after_body)
        .is_some_and(str::is_empty)
}

pub fn single_ed25519_line(text: &str) -> Option<String> {
    let line = text.trim_end();
    if line.contains('\n') || line.contains('\r') {
        return None;
    }
    let after = line.strip_prefix("ssh-ed25519 ").and_then(after_body)?;
    let ok = match after.strip_prefix(' ') {
        None => after.is_empty(),
        Some(comment) => {
            (1..=100).contains(&comment.chars().count())
                && comment
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "@._-".contains(c))
        }
    };
    ok.then(|| line.to_string())
}

pub fn normalised_key(text: &str) -> Option<String> {
    let line = text.trim_end();
    if line.contains('\n') || line.contains('\r') {
        return None;
    }
    let words: Vec<&str> = line.split(' ').filter(|w| !w.is_empty()).collect();
    if words.len() < 2 || words[0] != "ssh-ed25519" {
        return None;
    }
    let normal = format!("ssh-ed25519 {}", words[1]);
    is_key_body_line(&normal).then_some(normal)
}

pub fn gh() -> String {
    shell::tool_path("gh")
}

fn gh_argv(words: &[&str]) -> Vec<String> {
    std::iter::once(gh())
        .chain(words.iter().map(|w| w.to_string()))
        .collect()
}

pub fn list_argv(github: &str) -> Vec<String> {
    gh_argv(&["api", "--paginate", &format!("repos/{github}/keys")])
}

pub fn add_argv(file: &str, github: &str, title: &str) -> Vec<String> {
    gh_argv(&[
        "repo",
        "deploy-key",
        "add",
        file,
        "-R",
        github,
        "--title",
        title,
    ])
}

pub fn delete_argv(id: &str, github: &str) -> Vec<String> {
    gh_argv(&["repo", "deploy-key", "delete", id, "-R", github])
}

pub fn parse_keys(text: &str) -> Option<Vec<Value>> {
    match serde_json::from_str::<Value>(text).ok()? {
        Value::Array(list) if list.iter().all(Value::is_object) => Some(list),
        _ => None,
    }
}

pub fn listed<'a>(pubkey: &str, keys: &'a [Value]) -> Option<&'a Value> {
    let mine = normalised_key(pubkey)?;
    keys.iter().find(|k| {
        k.get("key")
            .and_then(Value::as_str)
            .and_then(normalised_key)
            .is_some_and(|n| n == mine)
    })
}

pub fn read_only(entry: &Value) -> bool {
    entry.get("read_only").and_then(Value::as_bool) == Some(true)
}

pub fn key_id(entry: &Value) -> Option<i64> {
    let id = entry.get("id")?.as_i64()?;
    (id > 0).then_some(id)
}

pub fn can_write(github: &str) -> String {
    format!(
        "the key can write to {github}; remove it in Settings > Deploy keys on GitHub, then set up again"
    )
}

pub fn first_line(text: &str, mask: &dyn Fn(&str) -> String, limit: usize) -> String {
    mask(text)
        .split(['\n', '\r'])
        .map(|l| l.trim_matches([' ', '\t']))
        .find(|l| !l.is_empty())
        .map(|l| l.chars().take(limit).collect())
        .unwrap_or_default()
}

pub fn date_stamp(now: f64) -> String {
    let days = (now / 86_400.0).floor() as i64;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}{m:02}{d:02}")
}

pub fn not_listed_row() -> SubRow {
    SubRow::new("Key on GitHub", false, "not listed")
}
