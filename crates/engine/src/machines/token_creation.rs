use super::step::Opener;
use crate::vault::{expiry, VaultRequest};

const TOKEN_DAYS: i64 = 366;

fn encode(text: &str) -> String {
    let mut out = String::new();
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn page_url(secret: &str, machine: &str, repos: &[String]) -> String {
    let mut items = vec![
        ("name", secret.to_string()),
        (
            "description",
            format!("Issues only, for the sd worker on {machine}"),
        ),
    ];
    if let Some(owner) = repos
        .first()
        .and_then(|r| r.split('/').find(|p| !p.is_empty()))
    {
        items.push(("target_name", owner.to_string()));
    }
    items.push(("expires_in", TOKEN_DAYS.to_string()));
    items.push(("issues", "write".to_string()));
    let query: Vec<String> = items
        .iter()
        .map(|(k, v)| format!("{k}={}", encode(v)))
        .collect();
    format!(
        "https://github.com/settings/personal-access-tokens/new?{}",
        query.join("&")
    )
}

pub fn clicks(repos: &[String], all_repos: bool) -> Vec<String> {
    vec![
        if all_repos {
            "Repository access: All repositories (this job allows it).".to_string()
        } else {
            format!(
                "Repository access: choose Only select repositories, then pick {}.",
                repos.join(", ")
            )
        },
        "Permissions: Issues Read and write. Leave Contents at No access.".into(),
        "Generate token, copy it, and delete any old token for this machine on the same page."
            .into(),
        "Paste it into Notchling's form and Save.".into(),
    ]
}

pub fn request(
    secret: &str,
    machine: &str,
    repos: &[String],
    all_repos: bool,
    today: i64,
) -> VaultRequest {
    let steps: Vec<String> = clicks(repos, all_repos)
        .iter()
        .enumerate()
        .map(|(i, c)| format!("{}. {c}", i + 1))
        .collect();
    let mut how = vec![page_url(secret, machine, repos)];
    how.extend(steps);
    VaultRequest {
        name: secret.into(),
        kind: Some("token".into()),
        url: Some("https://github.com".into()),
        note: Some(format!(
            "Issues-only token for the sd worker on {machine}: {}",
            repos.join(", ")
        )),
        how_to: Some(how.join("\n")),
        expires: Some(expiry::day_text(today + TOKEN_DAYS)),
        reason: Some(format!("Machines: the GitHub issues token for {machine}")),
    }
}

pub fn start(
    opener: &dyn Opener,
    secret: &str,
    machine: &str,
    repos: &[String],
    all_repos: bool,
    now: f64,
) {
    let today = expiry::today_local(now);
    opener.open_page(&page_url(secret, machine, repos));
    opener.open_form(&request(secret, machine, repos, all_repos, today));
}
