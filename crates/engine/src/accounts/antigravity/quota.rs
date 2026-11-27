use crate::accounts::procs::Procs;
use crate::providers::usage::{GroupedGroup, GroupedUsage, UsageWindow};
use crate::providers::{HttpClient, HttpRequest, Trust};
use crate::sessions::iso::parse_iso_ms;
use regex::Regex;
use serde_json::Value;
use std::time::Duration;

const SERVICE: &str = "/exa.language_server_pb.LanguageServerService/";
const PGREP: &str = "/usr/bin/pgrep";
const PS: &str = "/bin/ps";
const LSOF: &str = "/usr/sbin/lsof";
const SHELL_TIMEOUT: Duration = Duration::from_secs(5);
const CALL_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub plan: Option<String>,
    pub usage: Option<GroupedUsage>,
}

const SERVERS: [(&[&str], bool); 3] = [
    (&["-f", "bin/language_server --standalone"], true),
    (&["-f", "bin/language_server_macos"], true),
    (&["-x", "agy"], false),
];

const PER_KIND: usize = 3;

pub fn fetch(
    procs: &dyn Procs,
    http: &dyn HttpClient,
    expected_email: Option<&str>,
    now: f64,
) -> Option<Snapshot> {
    for (pgrep, wants_token) in SERVERS {
        let Some(out) = procs.capture(PGREP, pgrep, SHELL_TIMEOUT) else {
            continue;
        };
        for pid in parse_pids(&out).into_iter().take(PER_KIND) {
            let csrf = if wants_token {
                match csrf_token(procs, &pid) {
                    Some(t) => Some(t),
                    None => continue,
                }
            } else {
                None
            };
            if let Some(snapshot) =
                fetch_from(procs, http, &pid, csrf.as_deref(), expected_email, now)
            {
                return Some(snapshot);
            }
        }
    }
    None
}

fn fetch_from(
    procs: &dyn Procs,
    http: &dyn HttpClient,
    pid: &str,
    csrf: Option<&str>,
    expected_email: Option<&str>,
    now: f64,
) -> Option<Snapshot> {
    for port in ports(procs, pid) {
        let Some(status_data) = post(http, "GetUserStatus", port, csrf) else {
            continue;
        };
        let (email, plan) = parse_status(&status_data);
        if let (Some(expected), Some(email)) = (expected_email, email.as_deref()) {
            if email != expected {
                return None;
            }
        }
        let usage =
            post(http, "RetrieveUserQuotaSummary", port, csrf).and_then(|d| parse_usage(&d, now));
        return Some(Snapshot { plan, usage });
    }
    None
}

pub fn parse_pid(out: &str) -> Option<String> {
    parse_pids(out).into_iter().next()
}

pub fn parse_pids(out: &str) -> Vec<String> {
    out.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_digit()))
        .map(String::from)
        .collect()
}

fn csrf_token(procs: &dyn Procs, pid: &str) -> Option<String> {
    let args = procs.capture(PS, &["-o", "command=", "-p", pid], SHELL_TIMEOUT)?;
    csrf_from_args(&args)
}

pub fn csrf_from_args(args: &str) -> Option<String> {
    let re = Regex::new(r"--csrf_token\s+(\S+)").ok()?;
    let token = re.captures(args)?.get(1)?.as_str();
    token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        .then(|| token.to_string())
}

pub fn lsof_args(pid: &str) -> Vec<&str> {
    vec!["-nP", "-a", "-p", pid, "-iTCP", "-sTCP:LISTEN"]
}

fn ports(procs: &dyn Procs, pid: &str) -> Vec<u16> {
    procs
        .capture(LSOF, &lsof_args(pid), SHELL_TIMEOUT)
        .map(|out| ports_from_lsof(&out))
        .unwrap_or_default()
}

pub fn ports_from_lsof(out: &str) -> Vec<u16> {
    let Ok(re) = Regex::new(r":(\d+) \(LISTEN\)") else {
        return vec![];
    };
    let mut seen: Vec<u16> = vec![];
    for caps in re.captures_iter(out) {
        if let Some(port) = caps.get(1).and_then(|m| m.as_str().parse::<u16>().ok()) {
            if port != 0 && !seen.contains(&port) {
                seen.push(port);
            }
        }
    }
    seen.sort_unstable();
    seen
}

fn post(http: &dyn HttpClient, method: &str, port: u16, csrf: Option<&str>) -> Option<Vec<u8>> {
    let mut headers = vec![
        ("Content-Type".into(), "application/json".into()),
        ("Connect-Protocol-Version".into(), "1".into()),
    ];
    if let Some(csrf) = csrf {
        headers.push(("x-codeium-csrf-token".into(), csrf.to_string()));
    }
    let request = HttpRequest {
        method: "POST",
        url: format!("https://127.0.0.1:{port}{SERVICE}{method}"),
        headers,
        body: Some(b"{}".to_vec()),
        timeout: CALL_TIMEOUT,
        trust: Trust::LoopbackSelfSigned,
    };
    let reply = http.send(&request).ok()?;
    (reply.status == 200).then_some(reply.body)
}

pub fn parse_status(data: &[u8]) -> (Option<String>, Option<String>) {
    let Ok(root) = serde_json::from_slice::<Value>(data) else {
        return (None, None);
    };
    let Some(status) = root.get("userStatus") else {
        return (None, None);
    };
    let email = status
        .get("email")
        .and_then(Value::as_str)
        .map(str::to_string);
    let plan = status
        .get("userTier")
        .and_then(|t| t.get("name"))
        .and_then(Value::as_str)
        .map(|n| n.replace("Antigravity ", "").replace(" Quota", ""))
        .filter(|p| !p.is_empty());
    (email, plan)
}

pub fn parse_usage(data: &[u8], now: f64) -> Option<GroupedUsage> {
    let root: Value = serde_json::from_slice(data).ok()?;
    let groups = root.get("response")?.get("groups")?.as_array()?;
    let out: Vec<GroupedGroup> = groups
        .iter()
        .filter_map(|group| {
            let name = group.get("displayName")?.as_str()?.to_string();
            let buckets = group.get("buckets")?.as_array()?;
            let bars = buckets
                .iter()
                .map(|bucket| {
                    let remaining = bucket
                        .get("remainingFraction")
                        .and_then(Value::as_f64)
                        .unwrap_or(1.0);
                    let percent = ((1.0 - remaining) * 100.0).clamp(0.0, 100.0);
                    let label = if bucket.get("window").and_then(Value::as_str) == Some("weekly") {
                        "Week"
                    } else {
                        "5h"
                    };
                    let resets_at = bucket
                        .get("resetTime")
                        .and_then(Value::as_str)
                        .and_then(parse_iso_ms)
                        .map(|ms| ms as f64 / 1000.0);
                    (label.to_string(), UsageWindow { percent, resets_at })
                })
                .collect();
            Some(GroupedGroup { name, bars })
        })
        .collect();
    (!out.is_empty()).then_some(GroupedUsage {
        groups: out,
        fetched: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::accounts::procs::FakeProcs;
    use crate::providers::FakeHttp;

    const LSOF_OUT: &str = "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nlanguage_ 4242 me 12u IPv4 0x1 0t0 TCP 127.0.0.1:50731 (LISTEN)\nlanguage_ 4242 me 13u IPv4 0x2 0t0 TCP 127.0.0.1:50730 (LISTEN)\nlanguage_ 4242 me 14u IPv6 0x3 0t0 TCP [::1]:50730 (LISTEN)\n";

    fn rig() -> FakeProcs {
        FakeProcs::new()
            .answer(
                "/usr/bin/pgrep -f bin/language_server --standalone",
                "4242\n4343\n",
            )
            .answer(
                "/bin/ps -o command= -p 4242",
                "/Applications/Antigravity.app/bin/language_server --standalone --csrf_token abc-123 --x y\n",
            )
            .answer("/usr/sbin/lsof -nP -a -p 4242 -iTCP -sTCP:LISTEN", LSOF_OUT)
    }

    const STATUS: &str = r#"{"userStatus":{"email":"a@example.com","userTier":{"name":"Antigravity Starter Quota"},"planInfo":{"planName":"Pro"}}}"#;
    const QUOTA: &str = r#"{"response":{"groups":[{"displayName":"Gemini","buckets":[
      {"window":"weekly","remainingFraction":0.75,"resetTime":"2026-11-10T00:00:00Z"},
      {"window":"fiveHour","remainingFraction":1.5},{"remainingFraction":-1}]}]}}"#;

    #[test]
    fn parses_the_pid_the_csrf_token_and_the_ports() {
        assert_eq!(parse_pid("4242\n4343\n").as_deref(), Some("4242"));
        assert_eq!(parse_pid("").as_deref(), None);
        assert_eq!(parse_pid("12 34").as_deref(), None);
        assert_eq!(
            csrf_from_args("a --csrf_token  abc-123 --b").as_deref(),
            Some("abc-123")
        );
        assert_eq!(csrf_from_args("a --csrf_token bad;token"), None);
        assert_eq!(csrf_from_args("no token here"), None);
        assert_eq!(ports_from_lsof(LSOF_OUT), vec![50730, 50731]);
        assert!(ports_from_lsof("nothing").is_empty());
    }

    #[test]
    fn lsof_is_asked_for_one_process_with_a_and_nothing_else() {
        let args = lsof_args("4242");
        assert!(
            args.contains(&"-a"),
            "without -a lsof ORs its filters and lists every listener"
        );
        let at = |s: &str| args.iter().position(|a| *a == s).unwrap();
        assert!(at("-a") < at("-p") && at("-a") < at("-iTCP"));
        assert_eq!(args[at("-p") + 1], "4242");
    }

    #[test]
    fn status_and_usage() {
        let (email, plan) = parse_status(STATUS.as_bytes());
        assert_eq!(email.as_deref(), Some("a@example.com"));
        assert_eq!(plan.as_deref(), Some("Starter"));
        assert_eq!(parse_status(b"{}"), (None, None));
        let usage = parse_usage(QUOTA.as_bytes(), 5.0).unwrap();
        let bars = &usage.groups[0].bars;
        assert_eq!(usage.groups[0].name, "Gemini");
        assert_eq!(
            (bars[0].0.as_str(), bars[0].1.percent, bars[0].1.resets_at),
            ("Week", 25.0, Some(1_794_268_800.0))
        );
        assert_eq!(
            (bars[1].0.as_str(), bars[1].1.percent),
            ("5h", 0.0),
            "clamped"
        );
        assert_eq!(bars[2].1.percent, 100.0, "clamped");
        assert!(parse_usage(b"{}", 0.0).is_none());
        assert!(parse_usage(br#"{"response":{"groups":[]}}"#, 0.0).is_none());
    }

    #[test]
    fn fetch_calls_each_port_with_the_csrf_header_over_loopback_only() {
        let procs = rig();
        let http = FakeHttp::new();
        http.push(Err("closed".into()));
        http.ok(200, STATUS);
        http.ok(200, QUOTA);
        let snap = fetch(&procs, &http, Some("a@example.com"), 9.0).unwrap();
        assert_eq!(snap.plan.as_deref(), Some("Starter"));
        assert_eq!(snap.usage.unwrap().fetched, 9.0);
        let reqs = http.requests();
        assert_eq!(reqs.len(), 3);
        assert_eq!(
            reqs[0].url,
            "https://127.0.0.1:50730/exa.language_server_pb.LanguageServerService/GetUserStatus"
        );
        assert!(
            reqs[1].url.starts_with("https://127.0.0.1:50731/")
                && reqs[1].url.ends_with("GetUserStatus")
        );
        assert!(reqs[2].url.ends_with("RetrieveUserQuotaSummary"));
        for r in &reqs {
            assert_eq!(r.trust, Trust::LoopbackSelfSigned);
            assert_eq!(r.method, "POST");
            assert_eq!(r.body.as_deref(), Some(&b"{}"[..]));
            assert!(r
                .headers
                .contains(&("x-codeium-csrf-token".into(), "abc-123".into())));
            assert!(r
                .headers
                .contains(&("Connect-Protocol-Version".into(), "1".into())));
        }
        assert!(!format!("{reqs:?}").contains("abc-123"));
    }

    #[test]
    fn it_skips_the_round_while_antigravity_is_on_another_account() {
        let procs = rig();
        let http = FakeHttp::new();
        http.ok(200, STATUS);
        assert!(fetch(&procs, &http, Some("someone-else@example.com"), 0.0).is_none());
        assert_eq!(http.calls(), 1, "no usage call after the mismatch");
    }

    const AGY_QUOTA: &str = include_str!("testdata/agy-quota.json");
    const IDE_PGREP: &str = "/usr/bin/pgrep -f bin/language_server_macos";
    const AGY_PGREP: &str = "/usr/bin/pgrep -x agy";
    const AGY_LSOF: &str = "COMMAND PID USER FD TYPE DEVICE SIZE/OFF NODE NAME\nagy 777 me 12u IPv4 0x1 0t0 TCP 127.0.0.1:55483 (LISTEN)\nagy 777 me 13u IPv4 0x2 0t0 TCP 127.0.0.1:55484 (LISTEN)\n";

    fn agy_only() -> FakeProcs {
        FakeProcs::new()
            .answer(AGY_PGREP, "777\n778\n")
            .answer("/usr/sbin/lsof -nP -a -p 777 -iTCP -sTCP:LISTEN", AGY_LSOF)
    }

    #[test]
    fn the_agy_cli_answers_without_a_token() {
        let procs = agy_only();
        let http = FakeHttp::new();
        http.ok(200, STATUS);
        http.ok(200, AGY_QUOTA);
        let snap = fetch(&procs, &http, Some("a@example.com"), 1.0).unwrap();
        let usage = snap.usage.unwrap();
        let names: Vec<&str> = usage.groups.iter().map(|g| g.name.as_str()).collect();
        assert_eq!(
            names,
            ["Gemini Models", "Claude and GPT models", "Gemini 4 Argon"]
        );
        let bars: Vec<&str> = usage.groups[0].bars.iter().map(|b| b.0.as_str()).collect();
        assert_eq!(bars, ["Week", "5h"]);
        assert_eq!(usage.groups[0].bars[0].1.percent, 0.0);
        let reqs = http.requests();
        assert_eq!(reqs.len(), 2);
        assert!(reqs[0].url.starts_with("https://127.0.0.1:55483/"));
        for r in &reqs {
            assert_eq!(r.trust, Trust::LoopbackSelfSigned);
            assert!(
                !r.headers.iter().any(|(k, _)| k == "x-codeium-csrf-token"),
                "agy takes no token, and no other server's goes to it"
            );
        }
        assert!(
            !procs
                .calls()
                .iter()
                .any(|c| format!("{c:?}").contains("/bin/ps")),
            "agy's command line isn't read for a token"
        );
    }

    #[test]
    fn the_ide_server_is_found_with_its_token() {
        let procs = FakeProcs::new()
            .answer(IDE_PGREP, "28025\n")
            .answer(
                "/bin/ps -o command= -p 28025",
                "/Applications/Antigravity IDE.app/Contents/Resources/app/extensions/antigravity/bin/language_server_macos_arm --csrf_token ide-1 --subclient_type ide\n",
            )
            .answer("/usr/sbin/lsof -nP -a -p 28025 -iTCP -sTCP:LISTEN", LSOF_OUT);
        let http = FakeHttp::new();
        http.ok(200, STATUS);
        http.ok(200, QUOTA);
        assert!(fetch(&procs, &http, None, 0.0).unwrap().usage.is_some());
        assert!(http.requests()[0]
            .headers
            .contains(&("x-codeium-csrf-token".into(), "ide-1".into())));
    }

    #[test]
    fn a_server_not_signed_in_or_on_another_account_falls_through_to_agy() {
        let procs = FakeProcs::new()
            .answer(IDE_PGREP, "28025\n")
            .answer("/bin/ps -o command= -p 28025", "x --csrf_token ide-1\n")
            .answer(
                "/usr/sbin/lsof -nP -a -p 28025 -iTCP -sTCP:LISTEN",
                LSOF_OUT,
            )
            .answer(AGY_PGREP, "777\n")
            .answer("/usr/sbin/lsof -nP -a -p 777 -iTCP -sTCP:LISTEN", AGY_LSOF);
        let http = FakeHttp::new();
        http.ok(500, r#"{"code":"unknown"}"#);
        http.ok(200, r#"{"userStatus":{"email":"other@example.com"}}"#);
        http.ok(200, STATUS);
        http.ok(200, AGY_QUOTA);
        let snap = fetch(&procs, &http, Some("a@example.com"), 0.0).unwrap();
        assert_eq!(snap.usage.unwrap().groups.len(), 3);
        let reqs = http.requests();
        assert_eq!(reqs.len(), 4);
        assert!(reqs[2].url.contains(":55483/"));
    }

    #[test]
    fn several_pids_are_read_in_order() {
        assert_eq!(parse_pids("1\n\n22\nnope\n333\n"), ["1", "22", "333"]);
        assert!(parse_pids("").is_empty());
    }

    #[test]
    fn no_server_no_token_or_no_port_means_no_limits_and_no_request() {
        let http = FakeHttp::new();
        assert!(fetch(&FakeProcs::new(), &http, None, 0.0).is_none());
        let no_token = FakeProcs::new()
            .answer(
                "/usr/bin/pgrep -f bin/language_server --standalone",
                "4242\n",
            )
            .answer(
                "/bin/ps -o command= -p 4242",
                "language_server --standalone\n",
            );
        assert!(fetch(&no_token, &http, None, 0.0).is_none());
        let no_ports = FakeProcs::new()
            .answer(
                "/usr/bin/pgrep -f bin/language_server --standalone",
                "4242\n",
            )
            .answer("/bin/ps -o command= -p 4242", "x --csrf_token t1\n");
        assert!(fetch(&no_ports, &http, None, 0.0).is_none());
        assert_eq!(http.calls(), 0);
    }
}
