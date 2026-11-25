use std::collections::BTreeMap;

use notchling_engine::vault::entry::{
    Field as Changed, HostChange, Kind, PrefillChanges, VaultEntry, VaultRequest, VaultValue,
};
use serde_json::{json, Value};

use super::wire::{PendingRequest, Verified};
use crate::views::ViewSpec;

pub const NOW: f64 = 1_794_096_000.0;
pub const TODAY: i64 = 20_765;

pub const VIEWS: [ViewSpec; 9] = [
    ViewSpec {
        name: "vault-sheet",
        size: (540.0, 560.0),
        about: "the Secret Vault sheet: three saved secrets, no values (made-up data)",
    },
    ViewSpec {
        name: "vault-sheet-fields",
        size: (540.0, 560.0),
        about: "the Secret Vault sheet with each value line: the username plain, the password and tokens masked, each with its eye and copy buttons (made-up data)",
    },
    ViewSpec {
        name: "vault-sheet-revealed",
        size: (540.0, 560.0),
        about: "the Secret Vault sheet with one token shown (eye-slash) and the others masked (made-up data)",
    },
    ViewSpec {
        name: "vault-form",
        size: (540.0, 700.0),
        about: "the vault's Edit Secret form, the value unchanged (made-up data)",
    },
    ViewSpec {
        name: "vault-form-lines",
        size: (540.0, 760.0),
        about: "the vault's Add Secret form with a multi-line value, masked, four lines (made-up data)",
    },
    ViewSpec {
        name: "vault-approval",
        size: super::approval::PROMPT_SIZE,
        about: "the approval prompt: an agent waits to use two secrets, one more waiting (made-up data)",
    },
    ViewSpec {
        name: "vault-request",
        size: (540.0, 760.0),
        about: "an agent's request form over a saved secret, with its changes marked (made-up data)",
    },
    ViewSpec {
        name: "vault-request-refused",
        size: (540.0, 760.0),
        about: "an agent's request form after the daemon refused the save: the entry changed since the card (made-up data)",
    },
    ViewSpec {
        name: "vault-remote",
        size: (540.0, 560.0),
        about: "the Secret Vault sheet on a Mac that isn't the vault's machine: the Vault on mac-mini tab (made-up data)",
    },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    List,
    Fields,
    Revealed,
    Form,
    FormLines,
    Approval,
    Request,
    RequestRefused,
    Remote,
}

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "vault-sheet" => Shown::List,
        "vault-sheet-fields" => Shown::Fields,
        "vault-sheet-revealed" => Shown::Revealed,
        "vault-form" => Shown::Form,
        "vault-form-lines" => Shown::FormLines,
        "vault-approval" => Shown::Approval,
        "vault-request" => Shown::Request,
        "vault-request-refused" => Shown::RequestRefused,
        "vault-remote" => Shown::Remote,
        _ => return None,
    })
}

pub const FAKE_KEY: &str = "-----BEGIN PRIVATE KEY-----\nNLFAKE0aZk1q2Xx9fPzT0bYw4Lr8mNcV3uHj5Ka1dEo6sGt\nNLFAKE1pL0cT8yUeB4nHs2JdX6fMq9ZrW3gVk5Ao7iCtYb1D\n-----END PRIVATE KEY-----\n";

pub const FAKE_USER: &str = "fake-user";
pub const FAKE_SECRET: &str = "fake-secret-value-0123";

pub struct FakeReader;

impl super::reader::SecretReader for FakeReader {
    fn read(&self, _name: &str, _rev: &str) -> Result<VaultValue, String> {
        Ok(VaultValue {
            username: FAKE_USER.into(),
            secret: FAKE_SECRET.into(),
        })
    }
}

pub fn entries() -> Vec<VaultEntry> {
    let mut github = VaultEntry::new("github", Kind::Token);
    github.url = Some("https://api.github.com".into());
    github.note = "GitHub API for my repos".into();
    github.how_to = Some(
        "GitHub > Settings > Developer settings > Fine-grained tokens. Scopes: contents (read), \
         issues (write). https://github.com/settings/tokens"
            .into(),
    );
    github.expires = Some("2026-12-08".into());
    github.updated = "2026-11-01T09:00:00Z".into();
    let mut registry = VaultEntry::new("registry", Kind::Login);
    registry.url = Some("https://registry.example.com".into());
    registry.note = "The private npm registry".into();
    registry.ask = Some(true);
    registry.expires = Some("2026-11-01".into());
    let mut weather = VaultEntry::new("weather-api", Kind::Token);
    weather.note = "Open weather data, read only".into();
    vec![github, registry, weather]
}

pub fn approvals() -> Vec<Value> {
    vec![
        json!({"id": "ap-1", "names": ["github", "registry"],
               "command": "gh api repos/example/notchling/releases --paginate \\\n  | jq '.[].tag_name'",
               "folder": "/Users/me/work/notchling",
               "verified": {"exe": "/bin/zsh", "pid": 4242, "cwd": "/private/tmp/scratch",
                            "parents": ["zsh", "claude", "Terminal"], "folder_matches": false}}),
        json!({"id": "ap-2", "names": ["weather-api"], "command": "curl $WEATHER_API_URL",
               "folder": "/Users/me/work/weather"}),
    ]
}

pub fn pending_request() -> PendingRequest {
    let saved = entries().remove(0);
    let request = VaultRequest {
        name: "github".into(),
        url: Some("https://api.github.example".into()),
        note: Some("GitHub API for releases and issues".into()),
        reason: Some("to publish the 1.2 release notes".into()),
        ..VaultRequest::default()
    };
    PendingRequest {
        id: "req-1".into(),
        entry: saved.prefilled(&request),
        existing: true,
        changes: PrefillChanges {
            old: BTreeMap::from([
                (Changed::Url, "https://api.github.com".to_owned()),
                (Changed::Note, "GitHub API for my repos".to_owned()),
            ]),
            host_change: Some(HostChange {
                from: Some("https://api.github.com".into()),
                to: Some("https://api.github.example".into()),
            }),
        },
        request,
        session: Some("s-chat".into()),
        snapshot: None,
        verified: Some(Verified {
            exe: Some("/usr/local/bin/notchling".into()),
            pid: Some(4242),
            cwd: Some("/Users/me/work/notchling".into()),
            parents: vec!["zsh".into(), "claude".into(), "Terminal".into()],
            folder_matches: Some(true),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::switches::Switches;
    use crate::vault::approval::{pending, Prompt, FOLDER_WARNING};
    use crate::vault::form::Form;
    use crate::vault::list::list;

    #[test]
    fn every_view_has_its_data() {
        for spec in VIEWS {
            assert!(shown(spec.name).is_some(), "{}", spec.name);
            assert!(crate::views::spec(spec.name).is_some(), "{}", spec.name);
        }
        assert_eq!(shown("chat"), None);
        let l = list(&entries(), &Switches::all_on(), TODAY, None);
        assert_eq!(l.rows.len(), 3);
        assert!(l.rows[1].expiry.as_ref().is_some_and(|e| e.expired));
        let p = Prompt::new(&pending(&approvals())[0]);
        assert_eq!(p.warning, Some(FOLDER_WARNING));
        assert!(p.command.contains('⏎'), "{}", p.command);
        let f = Form::request(&pending_request());
        assert!(f.host_warning().is_some());
        assert!(Form::edit(&entries()[0]).can_save());
    }
}
