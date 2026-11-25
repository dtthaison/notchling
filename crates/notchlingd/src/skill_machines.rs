use crate::skills_family::{minimal, NAMING_RULE};

pub const NAME: &str = "notchling-machines";

const DESCRIPTION: &str = "Use to list, check, plan, add or change the machines of your organization, to see the Tailscale peers, or to list or switch the Tailscale network.";

const TITLE: &str = "Notchling machines";

const ABOUT: &str = "A machine is a Mac or Linux node of your organization. Reads show only your organization; a change asks the user on a card on the Mac.";

const STEPS: &str = "\nSteps:\n\n\
- Name a machine after the machine itself (`m1`, `vps-sing-1`, `build-mac`), never `server`, `host`, `new` or `machine`. A generic `--name` is refused with exit 2; the name taken from `user@host` is not checked.\n\
- Run `machines check` before `machines plan`, and `machines plan` before asking for a `fix` or `setup`: the plan shows what each step would change.\n\
- Trust Host (trusting a machine's host key) is the user's decision in the app: a request for it, for a login or for a token is refused, so do not ask.\n\
- Passwords, keys and tokens come from the vault and never go in arguments.\n\
- Reads and changes need the `machines` switch on; if it is off, the reply names the command to ask the user to run (`switch on machines`).\n\
- Run `network list` before asking `network switch`: the card lists the machines that lose reach, and the ids come from the list.\n";

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
    fn the_machines_skill_teaches_the_names_the_order_and_the_rules() {
        let text = content("notchling");
        for part in [
            NAMING_RULE,
            REASON_RULE,
            KEY_RULE,
            "`m1`, `vps-sing-1`, `build-mac`",
            "Run `machines check` before `machines plan`",
            "Trust Host",
            "`switch on machines`",
            "Run `network list` before asking `network switch`",
            "'notchling' help --json",
        ] {
            assert!(text.contains(part), "{part}");
        }
        assert!(text.find("Steps:") < text.find("A request for the user's yes takes:"));
    }

    #[test]
    fn the_machines_skill_puts_no_secret_in_an_argument() {
        let text = content("notchling");
        assert!(text.contains("never go in arguments"));
        assert!(!text.contains("--password"));
    }
}
