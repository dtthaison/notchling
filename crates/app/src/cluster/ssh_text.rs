use super::trust::{TrustFail, TrustFlow};
use notchling_engine::machines::ssh_key::{file_name, is_public, KeyIssue};
use notchling_engine::machines::ssh_test::{SshTest, Why};
use notchling_kit::Ink;
use notchling_locale as t;

pub fn test_text(result: &SshTest) -> String {
    match result {
        SshTest::Connected { user } => t::cluster::ssh_test_connected(user),
        SshTest::NewHost { user } => t::cluster::ssh_test_new_host(user),
        SshTest::Denied => t::cluster::ssh_test_denied().to_owned(),
        SshTest::HostKeyChanged => t::cluster::ssh_test_host_key_changed().to_owned(),
        SshTest::JumpUntrusted { host } => t::cluster::ssh_test_jump_untrusted(host),
        SshTest::Unreachable(Why::TimedOut) => t::cluster::ssh_test_timed_out().to_owned(),
        SshTest::Unreachable(Why::Refused) => t::cluster::ssh_test_refused().to_owned(),
        SshTest::Unreachable(Why::NoName) => t::cluster::ssh_test_no_name().to_owned(),
        SshTest::Unreachable(Why::NoRoute) => t::cluster::ssh_test_no_route().to_owned(),
        SshTest::Unreachable(Why::Other) => t::cluster::ssh_test_no_answer().to_owned(),
        SshTest::WrongNetwork { .. } => t::cluster::ssh_test_wrong_network().to_owned(),
        SshTest::Key(issue) => key_issue_text(*issue).to_owned(),
    }
}

pub fn address_test_text(result: &SshTest) -> String {
    match result {
        SshTest::WrongNetwork { wanted, active } => {
            t::cluster::ssh_test_wrong_network_named(wanted, active)
        }
        other => test_text(other),
    }
}

pub fn test_ink(result: &SshTest) -> Ink {
    if result.passed() {
        Ink::Done
    } else {
        Ink::Needs
    }
}

pub fn key_issue_text(issue: KeyIssue) -> &'static str {
    match issue {
        KeyIssue::BadShape => t::cluster::ssh_key_shape(),
        KeyIssue::Missing => t::cluster::ssh_key_missing(),
        KeyIssue::NotAFile => t::cluster::ssh_key_not_file(),
        KeyIssue::TooOpen => t::cluster::ssh_key_too_open(),
        KeyIssue::Unreadable => t::cluster::ssh_key_unreadable(),
    }
}

pub fn key_title(key: Option<&str>) -> String {
    match key.map(file_name).filter(|n| !n.is_empty()) {
        Some(name) => t::cluster::ssh_key_named(&name),
        None => t::cluster::ssh_key_default().to_owned(),
    }
}

pub fn key_note(key: Option<&str>, issue: Option<KeyIssue>) -> Option<(String, Ink)> {
    if let Some(issue) = issue {
        return Some((key_issue_text(issue).to_owned(), Ink::Needs));
    }
    key.filter(|k| is_public(k))
        .map(|_| (t::cluster::ssh_key_public().to_owned(), Ink::TextMuted))
}

pub fn trusts_here(result: &SshTest) -> bool {
    matches!(result, SshTest::NewHost { .. })
}

pub fn trust_lines(flow: &TrustFlow) -> Vec<(String, Ink)> {
    match flow {
        TrustFlow::Scanning => vec![(t::cluster::ssh_trust_scanning().to_owned(), Ink::TextMuted)],
        TrustFlow::Review(keys) => {
            let mut lines: Vec<(String, Ink)> = keys
                .iter()
                .map(|k| {
                    (
                        t::cluster::ssh_trust_key(&k.label(), &k.fingerprint),
                        Ink::Text,
                    )
                })
                .collect();
            lines.push((t::cluster::ssh_trust_check().to_owned(), Ink::Needs));
            lines
        }
        TrustFlow::Saving(_) => vec![(t::cluster::ssh_trust_saving().to_owned(), Ink::TextMuted)],
        TrustFlow::Failed(TrustFail::Jump) => {
            vec![(t::cluster::ssh_trust_jump().to_owned(), Ink::Needs)]
        }
        TrustFlow::Failed(TrustFail::Nothing) => {
            vec![(t::cluster::ssh_trust_nothing().to_owned(), Ink::Needs)]
        }
        TrustFlow::Failed(TrustFail::Write(why)) => {
            vec![(t::cluster::ssh_trust_write_failed(why), Ink::Needs)]
        }
    }
}
