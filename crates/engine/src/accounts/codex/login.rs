//! Reading a Codex login (`auth.json`) and the usage endpoint Codex's own /status reads
//! (`CodexLogin` in `Codex.swift`). Pure: nothing here touches a file, the Keychain or the network.

use crate::accounts::jwt;
use crate::providers::usage::{GroupedGroup, GroupedUsage, UsageWindow};
use serde_json::Value;

/// Whose login this is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    /// `<chatgpt_user_id>:<chatgpt_account_id>`: one person can be in a personal and a team
    /// workspace.
    pub id: String,
    pub email: String,
    pub plan: Option<String>,
}

fn tokens(login: &[u8]) -> Option<Value> {
    let root: Value = serde_json::from_slice(login).ok()?;
    let tokens = root.get("tokens")?;
    tokens.is_object().then(|| tokens.clone())
}

/// Whose login this is, from the id token's claims. `None` for an API key login, which has no
/// account.
pub fn identity(login: &[u8]) -> Option<Identity> {
    let tokens = tokens(login)?;
    let claims = jwt::claims(tokens.get("id_token")?.as_str()?)?;
    let auth = claims
        .get("https://api.openai.com/auth")
        .cloned()
        .unwrap_or(Value::Null);
    let text = |v: &Value, key: &str| v.get(key).and_then(Value::as_str).map(str::to_string);
    let workspace = text(&auth, "chatgpt_account_id").or_else(|| text(&tokens, "account_id"))?;
    let user = text(&auth, "chatgpt_user_id")
        .or_else(|| text(&auth, "user_id"))
        .or_else(|| text(&claims, "sub"))?;
    let email = text(&claims, "email")
        .or_else(|| {
            claims
                .get("https://api.openai.com/profile")
                .and_then(|p| text(p, "email"))
        })
        .unwrap_or_else(|| "ChatGPT Account".to_string());
    Some(Identity {
        id: format!("{user}:{workspace}"),
        email,
        plan: plan_name(text(&auth, "chatgpt_plan_type").as_deref()),
    })
}

/// The access token and workspace to call the usage endpoint with. Debug shows neither.
#[derive(Clone, PartialEq, Eq)]
pub struct Access {
    pub token: String,
    pub workspace: String,
}

impl std::fmt::Debug for Access {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Access(..)")
    }
}

/// The access token and workspace to call the usage endpoint with, while the token is still good
/// (a minute to spare). Notchling never renews a Codex token: refresh tokens rotate, and a Codex
/// session still holding the old one would be signed out. An expired token means no limits for
/// that account until Codex itself renews it.
pub fn access(login: &[u8], now: f64) -> Option<Access> {
    let tokens = tokens(login)?;
    let token = tokens.get("access_token")?.as_str()?.to_string();
    let workspace = tokens.get("account_id")?.as_str()?.to_string();
    if let Some(exp) = jwt::claims(&token).and_then(|c| c.get("exp").and_then(Value::as_f64)) {
        if exp < now + 60.0 {
            return None;
        }
    }
    Some(Access { token, workspace })
}

/// "plus" → "Plus", "team_plus" → "Team Plus"; `None` for none or empty.
pub fn plan_name(raw: Option<&str>) -> Option<String> {
    let raw = raw.filter(|r| !r.is_empty())?;
    Some(
        raw.split('_')
            .map(|word| {
                let mut chars = word.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().chain(chars).collect::<String>(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

/// What `GET /backend-api/wham/usage` says: the plan and each limit window, as groups of bars.
pub fn usage(data: &[u8], now: f64) -> (Option<String>, Option<GroupedUsage>) {
    let Ok(root) = serde_json::from_slice::<Value>(data) else {
        return (None, None);
    };
    let mut groups: Vec<GroupedGroup> = vec![];
    let mut add = |name: &str, limit: Option<&Value>| {
        let Some(limit) = limit.filter(|l| l.is_object()) else {
            return;
        };
        let bars: Vec<(String, UsageWindow)> = ["primary_window", "secondary_window"]
            .into_iter()
            .filter_map(|key| {
                let w = limit.get(key).filter(|w| w.is_object())?;
                let percent = w
                    .get("used_percent")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0)
                    .clamp(0.0, 100.0);
                let resets_at = w.get("reset_at").and_then(Value::as_f64).or_else(|| {
                    w.get("reset_after_seconds")
                        .and_then(Value::as_f64)
                        .map(|after| now + after)
                });
                let label = window_label(w.get("limit_window_seconds").and_then(Value::as_f64));
                Some((label, UsageWindow { percent, resets_at }))
            })
            .collect();
        if !bars.is_empty() {
            groups.push(GroupedGroup {
                name: name.to_string(),
                bars,
            });
        }
    };
    add("Codex", root.get("rate_limit"));
    add("Code Review", root.get("code_review_rate_limit"));
    if let Some(extras) = root.get("additional_rate_limits").and_then(Value::as_array) {
        for extra in extras {
            let name = ["limit_name", "name", "metered_feature"]
                .into_iter()
                .find_map(|k| extra.get(k).and_then(Value::as_str))
                .unwrap_or("Other");
            add(name, extra.get("rate_limit").or(Some(extra)));
        }
    }
    let plan = plan_name(root.get("plan_type").and_then(Value::as_str));
    let usage = (!groups.is_empty()).then_some(GroupedUsage {
        groups,
        fetched: now,
    });
    (plan, usage)
}

/// "5h" (the bar views read it as the short window), "Week", "Month", else hours or days.
pub fn window_label(seconds: Option<f64>) -> String {
    let Some(seconds) = seconds.filter(|s| *s > 0.0) else {
        return "Limit".to_string();
    };
    let hours = (seconds / 3600.0).round() as i64;
    match hours {
        5 => "5h".to_string(),
        168 => "Week".to_string(),
        672..=744 => "Month".to_string(),
        h if h < 48 => format!("{h}h"),
        h => format!("{} days", h / 24),
    }
}

/// What a switch interrupts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RunningWork {
    /// The ChatGPT app is open.
    pub app: bool,
    /// Codex's managed app server runs.
    pub server: bool,
    /// `codex` processes that are not the app server: CLI sessions and Chat turns.
    pub sessions: usize,
}

/// From `ps -axo command=` lines and whether the ChatGPT app runs (`runningWork`).
pub fn running_work(ps: &str, app: bool) -> RunningWork {
    let mut work = RunningWork {
        app,
        ..RunningWork::default()
    };
    for line in ps.lines() {
        let command = line
            .split(' ')
            .find(|s| !s.is_empty())
            .and_then(|s| s.rsplit('/').next())
            .unwrap_or("");
        if command != "codex" {
            continue;
        }
        if line.contains("app-server") {
            if line.contains("--managed-daemon") {
                work.server = true;
            }
        } else {
            work.sessions += 1;
        }
    }
    work
}

/// The confirmation text for a switch, if it would interrupt anything.
pub fn switch_warning(work: &RunningWork) -> Option<String> {
    let mut parts: Vec<String> = vec![];
    if work.app {
        parts.push("The ChatGPT app quits and opens again on the other account.".into());
    }
    if work.server {
        parts.push("Codex's background server restarts, so its running tasks stop.".into());
    }
    match work.sessions {
        0 => {}
        1 => parts.push(
            "A Codex session is open in a terminal. It keeps the old account until you start it again."
                .into(),
        ),
        n => parts.push(format!(
            "{n} Codex sessions are open in terminals. They keep the old account until you start them again."
        )),
    }
    (!parts.is_empty()).then(|| parts.join(" "))
}

/// A switch only runs on its own while nothing would be interrupted: no CLI or Chat turn (`codex`
/// processes), no ChatGPT app to quit. The managed app server alone doesn't count; it restarts
/// quietly.
pub fn idle_for_auto_switch(work: &RunningWork) -> bool {
    !work.app && work.sessions == 0
}

#[cfg(test)]
mod tests {
    use super::super::testkit::login;
    use super::*;

    const NOW: f64 = 1_000_000.0;

    #[test]
    fn identity_comes_from_the_id_token() {
        assert_eq!(
            identity(&login(NOW + 86400.0)),
            Some(Identity {
                id: "user-1:ws-1".into(),
                email: "a@example.com".into(),
                plan: Some("Plus".into())
            })
        );
    }

    #[test]
    fn an_api_key_login_has_no_account() {
        let data = br#"{"auth_mode":"apikey","OPENAI_API_KEY":"sk-x","tokens":null}"#;
        assert_eq!(identity(data), None);
        assert_eq!(identity(b"not json"), None);
    }

    #[test]
    fn an_expired_access_token_is_not_used_and_never_renewed() {
        let ok = access(&login(NOW + 86400.0), NOW).unwrap();
        assert_eq!(ok.workspace, "ws-1");
        assert!(access(&login(NOW - 10.0), NOW).is_none());
        assert!(
            access(&login(NOW + 30.0), NOW).is_none(),
            "under a minute left"
        );
        assert!(!format!("{ok:?}").contains("eyJ"));
    }

    #[test]
    fn usage_as_groups() {
        let body = br#"{"plan_type":"pro","rate_limit":{"allowed":true,"primary_window":{"used_percent":42,"limit_window_seconds":18000,"reset_at":1795593916},
        "secondary_window":{"used_percent":7.5,"limit_window_seconds":604800,"reset_after_seconds":3600}},
        "code_review_rate_limit":null,"additional_rate_limits":[{"limit_name":"GPT-5.5 Spark","rate_limit":{"primary_window":{"used_percent":100,"limit_window_seconds":2592000}}}]}"#;
        let (plan, usage) = usage(body, NOW);
        assert_eq!(plan.as_deref(), Some("Pro"));
        let groups = usage.unwrap().groups;
        let names: Vec<_> = groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(names, vec!["Codex", "GPT-5.5 Spark"]);
        let labels: Vec<_> = groups[0].bars.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(labels, vec!["5h", "Week"]);
        assert_eq!(
            groups[0].bars[0].1,
            UsageWindow {
                percent: 42.0,
                resets_at: Some(1795593916.0)
            }
        );
        assert_eq!(groups[0].bars[1].1.resets_at, Some(NOW + 3600.0));
        assert_eq!(groups[1].bars[0].0, "Month");
    }

    #[test]
    fn usage_from_nonsense_is_nothing() {
        assert_eq!(usage(b"[]", NOW), (None, None));
        assert_eq!(usage(b"nope", NOW), (None, None));
        let (plan, groups) = usage(br#"{"plan_type":"free"}"#, NOW);
        assert_eq!((plan.as_deref(), groups), (Some("Free"), None));
    }

    #[test]
    fn window_labels() {
        assert_eq!(window_label(Some(2_592_000.0)), "Month");
        assert_eq!(window_label(Some(3600.0)), "1h");
        assert_eq!(window_label(Some(86400.0 * 3.0)), "3 days");
        assert_eq!(window_label(None), "Limit");
        assert_eq!(window_label(Some(0.0)), "Limit");
        assert_eq!(plan_name(Some("team_plus")).as_deref(), Some("Team Plus"));
        assert_eq!(plan_name(Some("")), None);
    }

    #[test]
    fn switch_warning_text() {
        assert_eq!(switch_warning(&RunningWork::default()), None);
        assert_eq!(
            switch_warning(&RunningWork {
                app: false,
                server: true,
                sessions: 2
            })
            .as_deref(),
            Some("Codex's background server restarts, so its running tasks stop. 2 Codex sessions are open in terminals. They keep the old account until you start them again.")
        );
        assert!(switch_warning(&RunningWork {
            app: true,
            server: false,
            sessions: 1
        })
        .unwrap()
        .starts_with("The ChatGPT app quits"));
    }

    #[test]
    fn idle_for_auto_switch_ignores_the_server_only() {
        let work = |app, server, sessions| RunningWork {
            app,
            server,
            sessions,
        };
        assert!(idle_for_auto_switch(&work(false, false, 0)));
        assert!(idle_for_auto_switch(&work(false, true, 0)));
        assert!(!idle_for_auto_switch(&work(false, false, 1)));
        assert!(!idle_for_auto_switch(&work(true, false, 0)));
    }

    #[test]
    fn running_work_reads_ps_lines() {
        let ps = "/sbin/launchd\n/Users/me/.local/bin/codex --model x\n/opt/homebrew/bin/codex app-server --managed-daemon\n/opt/homebrew/bin/codex app-server\nvim codex\n";
        assert_eq!(
            running_work(ps, true),
            RunningWork {
                app: true,
                server: true,
                sessions: 1
            }
        );
        assert_eq!(running_work("", false), RunningWork::default());
    }
}
