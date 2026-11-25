use crate::skills_family::{minimal, NAMING_RULE};

pub const NAME: &str = "notchling-cluster";

const DESCRIPTION: &str = "Use to read or change the cluster of your organization: its status and members, create it, invite or remove a machine, join with a token, allow secrets, fail over, or check that an address answers.";

const TITLE: &str = "Notchling cluster";

const ABOUT: &str = "The cluster links the machines of one organization. A change asks the user on a card on the Mac.";

const STEPS: &str = "\nSteps:\n\n\
- Read first: `cluster status` shows the cluster and its members. A change needs the `cluster` switch on; if it is off, the reply names the command to ask the user to run (`switch on cluster`).\n\
- Name a cluster after its purpose or organization (`fleet-workers`, `acme-builds`), never `cluster`, `team` or `new`. `invite --name` takes the name of a machine already in Machines (`machines list`), and a generic one is refused with exit 2.\n\
- `cluster join` reads the join token on stdin (`--token-stdin`). A token comes from `cluster token` or an invite and goes to this session once: pipe it on, never put it in an argument, a file or the chat.\n\
- `cluster allow` takes the names of vault entries, never a secret value: the secret stays in the vault.\n\
- Promoting a machine, taking the primary back and continuing here are the user's, in the app: a request for them is refused.\n";

pub fn content(binary: &str) -> String {
    let mut text = minimal(NAME, DESCRIPTION, TITLE, ABOUT, binary);
    let at = text.find("\nA request for the user's yes takes:");
    let steps = format!("{STEPS}\n{NAMING_RULE}\n");
    match at {
        Some(at) => text.insert_str(at, &steps),
        None => text.push_str(&steps),
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skills_family::{KEY_RULE, NAMING_RULE, REASON_RULE};

    #[test]
    fn the_cluster_skill_teaches_the_names_the_token_and_the_rules() {
        let text = content("notchling");
        for part in [
            NAMING_RULE,
            REASON_RULE,
            KEY_RULE,
            "fleet-workers",
            "--token-stdin",
            "never put it in an argument",
            "`switch on cluster`",
            "'notchling' help --json",
        ] {
            assert!(text.contains(part), "{part}");
        }
        assert!(text.find("Steps:") < text.find("A request for the user's yes takes:"));
    }

    #[test]
    fn the_cluster_skill_names_no_secret_and_no_email() {
        let text = content("notchling");
        assert!(!text.contains('@'));
        assert!(!text.contains("nlj1."));
    }
}
