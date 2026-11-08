//! Reads the signed-in account's limits from Antigravity's local language server, the same source
//! as its own View Usage panel (`AntigravityQuota`). The server runs only while Antigravity is
//! open, listens on a random local port behind a CSRF token, and speaks Connect (JSON over HTTPS
//! with a self-signed certificate), so this finds the process, its token and ports, then calls
//! each port until one answers.
//!
//! The processes go through [`Procs`] and the calls through [`HttpClient`] (loopback-only trust),
//! so a test runs neither. The CSRF token comes from the server's own command line and goes only
//! into a request header; it is never stored, logged or returned.

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

/// The account's plan and limits, read together from the local server.
#[derive(Clone, Debug, PartialEq)]
pub struct Snapshot {
    pub plan: Option<String>,
    pub usage: Option<GroupedUsage>,
}

/// `expected_email` guards against reading while Antigravity is still loading another account: if
/// the server reports a different email, it hasn't caught up, so skip this round.
pub fn fetch(
    procs: &dyn Procs,
    http: &dyn HttpClient,
    expected_email: Option<&str>,
    now: f64,
) -> Option<Snapshot> {
    let pid = find_pid(procs)?;
    let csrf = csrf_token(procs, &pid)?;
    for port in ports(procs, &pid) {
        let Some(status_data) = post(http, "GetUserStatus", port, &csrf) else {
            continue;
        };
        let (email, plan) = parse_status(&status_data);
        if let (Some(expected), Some(email)) = (expected_email, email.as_deref()) {
            if email != expected {
                return None;
            }
        }
        let usage = post(http, "RetrieveUserQuotaSummary", port, &csrf)
            .and_then(|d| parse_usage(&d, now));
        return Some(Snapshot { plan, usage });
    }
    None
}

fn find_pid(procs: &dyn Procs) -> Option<String> {
    let out = procs.capture(
        PGREP,
        &["-f", "bin/language_server --standalone"],
        SHELL_TIMEOUT,
    )?;
    parse_pid(&out)
}

/// The first line, if it is a process id.
pub fn parse_pid(out: &str) -> Option<String> {
    let first = out.lines().next()?.trim();
    (!first.is_empty() && first.bytes().all(|b| b.is_ascii_digit())).then(|| first.to_string())
}

/// From the server's own command line (`--csrf_token <uuid>`).
fn csrf_token(procs: &dyn Procs, pid: &str) -> Option<String> {
    let args = procs.capture(PS, &["-o", "command=", "-p", pid], SHELL_TIMEOUT)?;
    csrf_from_args(&args)
}

pub fn csrf_from_args(args: &str) -> Option<String> {
    let re = Regex::new(r"--csrf_token\s+(\S+)").ok()?;
    let token = re.captures(args)?.get(1)?.as_str();
    // It goes into a header: a plain token only.
    token
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        .then(|| token.to_string())
}

/// The lsof arguments: `-a` ANDs the filters, so this is the one process's TCP listeners only;
/// without it lsof lists every listener on the Mac (the port probe once had this bug).
pub fn lsof_args(pid: &str) -> Vec<&str> {
    vec!["-nP", "-a", "-p", pid, "-iTCP", "-sTCP:LISTEN"]
}

fn ports(procs: &dyn Procs, pid: &str) -> Vec<u16> {
    procs
        .capture(LSOF, &lsof_args(pid), SHELL_TIMEOUT)
        .map(|out| ports_from_lsof(&out))
        .unwrap_or_default()
}

/// The server picks random ports, so read the ones it's listening on. Lowest first: the server
/// opens its HTTPS port, then a plain HTTP one just above it.
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

/// One Connect unary call; the body only on a 200.
fn post(http: &dyn HttpClient, method: &str, port: u16, csrf: &str) -> Option<Vec<u8>> {
    let request = HttpRequest {
        method: "POST",
        url: format!("https://127.0.0.1:{port}{SERVICE}{method}"),
        headers: vec![
            ("Content-Type".into(), "application/json".into()),
            ("Connect-Protocol-Version".into(), "1".into()),
            ("x-codeium-csrf-token".into(), csrf.to_string()),
        ],
        body: Some(b"{}".to_vec()),
        timeout: CALL_TIMEOUT,
        trust: Trust::LoopbackSelfSigned,
    };
    let reply = http.send(&request).ok()?;
    (reply.status == 200).then_some(reply.body)
}

/// The signed-in account's email and plan from GetUserStatus. The plan is `userTier.name` (the real
/// Antigravity tier, e.g. "Antigravity Starter Quota"), not `planInfo.planName` (which is the
/// underlying Codeium plan and always reads "Pro"). Shortened to just the tier word.
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

/// `RetrieveUserQuotaSummary`: groups of buckets, each a weekly or five-hour window.
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
        assert!(args.contains(&"-a"), "without -a lsof ORs its filters and lists every listener");
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
        assert_eq!((bars[1].0.as_str(), bars[1].1.percent), ("5h", 0.0), "clamped");
        assert_eq!(bars[2].1.percent, 100.0, "clamped");
        assert!(parse_usage(b"{}", 0.0).is_none());
        assert!(parse_usage(br#"{"response":{"groups":[]}}"#, 0.0).is_none());
    }

    #[test]
    fn fetch_calls_each_port_with_the_csrf_header_over_loopback_only() {
        let procs = rig();
        let http = FakeHttp::new();
        http.push(Err("closed".into())); // the lower port refuses
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
        assert!(reqs[1].url.starts_with("https://127.0.0.1:50731/") && reqs[1].url.ends_with("GetUserStatus"));
        assert!(reqs[2].url.ends_with("RetrieveUserQuotaSummary"));
        for r in &reqs {
            assert_eq!(r.trust, Trust::LoopbackSelfSigned);
            assert_eq!(r.method, "POST");
            assert_eq!(r.body.as_deref(), Some(&b"{}"[..]));
            assert!(r.headers.contains(&("x-codeium-csrf-token".into(), "abc-123".into())));
            assert!(r.headers.contains(&("Connect-Protocol-Version".into(), "1".into())));
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

    #[test]
    fn no_server_no_token_or_no_port_means_no_limits_and_no_request() {
        let http = FakeHttp::new();
        assert!(fetch(&FakeProcs::new(), &http, None, 0.0).is_none());
        let no_token = FakeProcs::new()
            .answer("/usr/bin/pgrep -f bin/language_server --standalone", "4242\n")
            .answer("/bin/ps -o command= -p 4242", "language_server --standalone\n");
        assert!(fetch(&no_token, &http, None, 0.0).is_none());
        let no_ports = FakeProcs::new()
            .answer("/usr/bin/pgrep -f bin/language_server --standalone", "4242\n")
            .answer("/bin/ps -o command= -p 4242", "x --csrf_token t1\n");
        assert!(fetch(&no_ports, &http, None, 0.0).is_none());
        assert_eq!(http.calls(), 0);
    }
}
