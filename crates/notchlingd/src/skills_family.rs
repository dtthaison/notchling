use crate::commands::{rows, Row};

pub struct Family {
    pub skill: &'static str,
    pub about: &'static str,
}

pub const FAMILIES: &[Family] = &[
    Family {
        skill: "notchling",
        about: "everything no family covers: status, the inbox, consent requests, watching events, switches, activity, organizations, versions, costs and install",
    },
    Family {
        skill: "notchling-sessions",
        about: "follow, answer and control agent sessions, shells, prompts and queued messages",
    },
    Family {
        skill: "notchling-jobs",
        about: "send work to a machine of your organization and follow it",
    },
    Family {
        skill: "notchling-cluster",
        about: "the cluster of your organization: members, invites and tokens",
    },
    Family {
        skill: "notchling-machines",
        about: "the machines of your organization and the Tailscale networks",
    },
    Family {
        skill: "notchling-accounts",
        about: "accounts and lanes of your organization",
    },
    Family {
        skill: "notchling-delegate",
        about: "hand a job to another AI tool and read its limits",
    },
    Family {
        skill: "notchling-models",
        about: "models, efforts and prices of each tool",
    },
    Family {
        skill: "notchling-pages",
        about: "find and read saved Artifact pages, and search",
    },
    Family {
        skill: "notchling-settings",
        about: "read and change Notchling's settings",
    },
    Family {
        skill: "notchling-ask",
        about: "ask the user a question on its own card",
    },
    Family {
        skill: "notchling-vault",
        about: "use a saved credential, or ask for one",
    },
];

pub const NAMING_RULE: &str = "Name what you create (a vault entry, a machine, a cluster) after the service or host it is for, never a generic word: emqx-dashboard-dev-admin, not dashboards-basic-auth.";

pub const REASON_RULE: &str = "Give `--reason` a few plain words that say what you do and why: the user reads them on the card before saying yes.";

pub const KEY_RULE: &str = "Give a request `--key <text>` so a retry is safe: the same key with the same request finds the same card instead of making a second one, and the same key with a different request is refused (`key_reused`), so use a new key then.";

pub fn alternatives(usage: &str) -> (Vec<&str>, &str) {
    let (head, tail) = usage.split_once("; ").unwrap_or((usage, ""));
    (head.split(" | ").collect(), tail)
}

pub fn rows_of(skill: &str) -> impl Iterator<Item = &'static Row> + '_ {
    rows().filter(move |r| r.skill == skill)
}

pub fn usage_lines(skill: &str) -> Vec<String> {
    rows_of(skill)
        .flat_map(|r| alternatives(r.usage).0)
        .map(str::to_string)
        .collect()
}

fn tails(skill: &str) -> Vec<String> {
    let mut all: Vec<String> = Vec::new();
    for row in rows_of(skill) {
        let tail = alternatives(row.usage).1;
        let tail = tail.strip_prefix("each takes ").unwrap_or(tail);
        if !tail.is_empty() && !all.iter().any(|t| t == tail) {
            all.push(tail.to_string());
        }
    }
    all
}

pub fn minimal(name: &str, description: &str, title: &str, about: &str, binary: &str) -> String {
    let quoted = format!("'{binary}'");
    let lines: Vec<String> = usage_lines(name)
        .iter()
        .map(|line| match line.strip_prefix("notchling") {
            Some(rest) => format!("{quoted}{rest}"),
            None => line.clone(),
        })
        .collect();
    let mut text = format!(
        "---\nname: {name}\ndescription: {description}\n---\n\n# {title}\n\n{about}\n\n```sh\n{}\n```\n",
        lines.join("\n")
    );
    let tails = tails(name);
    if !tails.is_empty() {
        text.push_str("\nA request for the user's yes takes:\n\n");
        for tail in &tails {
            text.push_str(&format!("- {tail}\n"));
        }
        text.push_str(&format!("\n{REASON_RULE} {KEY_RULE}\n"));
    }
    text.push_str(&format!(
        "\nRun `{quoted} help --json` for every flag, exit code and consent rule of these commands.\n"
    ));
    text
}
