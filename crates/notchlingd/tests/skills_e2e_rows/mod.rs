use super::{OTHER, PAGE, WORDS};

pub(crate) const AWAITING: &[&str] = &[];

#[derive(Clone, Copy, Debug)]
pub(crate) enum Setup {
    SwitchOn(&'static str),
    Deny,
    Allow,
    ParseOnly(&'static str),
}

pub(crate) struct Example {
    pub(crate) skill: &'static str,
    pub(crate) line: &'static str,
    pub(crate) subst: Vec<(&'static str, &'static str)>,
    pub(crate) setup: Vec<Setup>,
    pub(crate) exit: i32,
    pub(crate) run: Option<&'static str>,
    pub(crate) extra: bool,
}

pub(crate) fn ex(skill: &'static str, line: &'static str) -> Example {
    Example {
        skill,
        line,
        subst: Vec::new(),
        setup: Vec::new(),
        exit: 0,
        run: None,
        extra: false,
    }
}

impl Example {
    pub(crate) fn sub(mut self, from: &'static str, to: &'static str) -> Example {
        self.subst.push((from, to));
        self
    }

    pub(crate) fn setup(mut self, setup: Setup) -> Example {
        self.setup.push(setup);
        self
    }

    pub(crate) fn exit(mut self, exit: i32) -> Example {
        self.exit = exit;
        self
    }

    pub(crate) fn run(mut self, run: &'static str) -> Example {
        self.run = Some(run);
        self
    }

    pub(crate) fn extra(mut self) -> Example {
        self.extra = true;
        self
    }

    pub(crate) fn fill(self) -> Example {
        self.sub("<id>", "{self}")
            .sub("<text>", "hello")
            .sub("<folder>", "{work}")
            .sub("<json>", "'[[\"a\"]]'")
            .sub("<command>", "/compact")
            .sub("<option>", "model")
            .sub("<value>", "opus")
            .sub("<1-9>", "1")
            .sub("<machine>", "m1")
            .sub("<path>", "{work}")
            .sub("<repo, branch, task>", "repo main fix")
            .sub("<task in a few words>", "fix the build")
            .sub("<words>", "{words}")
    }

    pub(crate) fn parse_only(self, why: &'static str) -> Example {
        self.setup(Setup::ParseOnly(why))
    }

    pub(crate) fn acting(self, reason: &'static str) -> Example {
        self.sub("[--reason <text>]", reason)
            .sub("[--no-wait]", "")
            .sub("[--timeout <seconds>]", "--timeout 10")
            .sub("[--expires-in <seconds>]", "")
            .sub("[--key <text>]", "")
            .sub("[--retry-of <id>]", "")
            .sub("[--dry-run]", "--dry-run")
    }

    pub(crate) fn command(&self, binary: &str, me: &str, ask: &str) -> String {
        let mut line = self.run.unwrap_or(self.line).to_string();
        line = line
            .replace(" [--socket <path>]", "")
            .replace("[--json]", "--json");
        for (from, to) in &self.subst {
            line = line.replace(from, to);
        }
        let line = line
            .replace("{self}", me)
            .replace("{ask}", ask)
            .replace("{page}", PAGE)
            .replace("{work}", "/tmp")
            .replace("{other}", OTHER)
            .replace("{words}", WORDS);
        format!("'{binary}' {line}")
    }
}

pub(crate) fn sessions_rows() -> Vec<Example> {
    vec![
        ex(SESSIONS, r#"consent <id> --wait --timeout 600 --json"#)
            .run(r#"consent consent-none --timeout 1 --json"#)
            .exit(1),
        ex(SESSIONS, r#"outbox [--json]"#),
        ex(SESSIONS, r#"prompts [--json]"#),
        ex(SESSIONS, r#"receipts [--json]"#),
        ex(SESSIONS, r#"search <words> --kind sessions --since 7d --json"#)
            .fill(),
        ex(SESSIONS, r#"session answer <id> --answers <json> --dry-run --json"#)
            .fill()
            .setup(Setup::SwitchOn("acting"))
            .parse_only("it needs a session with an open question"),
        ex(SESSIONS, r#"session answer <id> --answers <json> [--question <json>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session answer {self} --answers '[["a"]]' --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting"))
            .parse_only("it needs a session with an open question"),
        ex(SESSIONS, r#"session chrome <id> on --dry-run --json"#)
            .sub("<id>", "{other}")
            .fill()
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session chrome <id> on|off [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session chrome {other} on --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session command <id> <command> --dry-run --json"#)
            .sub("<id>", "{other}")
            .fill()
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session command <id> <command> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session command {other} /compact --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session dialog <id> --pick <1-9> --dry-run --json"#)
            .fill()
            .setup(Setup::SwitchOn("acting"))
            .parse_only("it needs a session in a terminal Notchling started"),
        ex(SESSIONS, r#"session dialog <id> --pick <1-9>|--dismiss [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session dialog {self} --pick 1 --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting"))
            .parse_only("it needs a session in a terminal Notchling started"),
        ex(SESSIONS, r#"session new <folder> --message "<task in a few words>" --dry-run --json"#)
            .fill()
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session new <folder> [--model <name>] [--message <text>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session new {work} --message hello --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session options <id> <option> <value> --dry-run --json"#)
            .fill()
            .setup(Setup::SwitchOn("acting"))
            .exit(4),
        ex(SESSIONS, r#"session options <id> <option> <value> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session options {self} model opus --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting"))
            .exit(4),
        ex(SESSIONS, r#"session reload <id|self> [--after-turn] [--json]"#)
            .fill()
            .run(r#"session reload self --json"#)
            .setup(Setup::SwitchOn("acting"))
            .parse_only("it reloads a session Notchling started; the fixture has none"),
        ex(SESSIONS, r#"session remove <id> --dry-run --json"#)
            .fill()
            .sub("{self}", "{other}")
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session remove <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session remove {other} --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session reply <id> "<text>" --dry-run --json"#)
            .fill()
            .sub("{self}", "{other}")
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session reply <id> <text> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session reply {other} hello --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session resume <id> --dry-run --json"#)
            .fill()
            .setup(Setup::SwitchOn("acting"))
            .parse_only("it resumes an ended Claude Code session; the fixture has none"),
        ex(SESSIONS, r#"session resume <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session resume {other} --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting"))
            .parse_only("it resumes an ended Claude Code session; the fixture has none"),
        ex(SESSIONS, r#"session stop <id> --dry-run --json"#)
            .sub("<id>", "{other}")
            .fill()
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"session stop|interrupt <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"session stop {other} --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting")),
        ex(SESSIONS, r#"sessions --json"#),
        ex(SESSIONS, r#"sessions history --limit 30 --json"#),
        ex(
            SESSIONS,
            r#"sessions history [--limit <n>] [--before <cursor>] [--json] [--socket <path>]"#,
        )
        .run(r#"sessions history --limit 5 --json"#),
        ex(SESSIONS, r#"sessions [--json] [--socket <path>]"#),
        ex(SESSIONS, r#"shells [--stuck] [--json] [--socket <path>]"#)
            .run(r#"shells --stuck --json"#),
        ex(SESSIONS, r#"shells stop <id> [--json] [--socket <path>]"#)
            .run(r#"shells stop sh-none --json"#)
            .exit(4),
        ex(SESSIONS, r#"watch --topic mine.consents --until applied --timeout 600 --json"#)
            .run(r#"watch --topic mine.consents --until applied --timeout 1 --json"#)
            .exit(124),
    ]
}

pub(crate) fn jobs_rows() -> Vec<Example> {
    vec![
        ex(JOBS, r#"dispatch <machine> --tool <claude|codex|agy|shell> [--folder <path>] [--model <name>] [--effort <level>] [--text <prompt>] [--command <line>] [--secret <name>]... [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .fill()
            .run(r#"dispatch m1 --tool claude --folder /tmp --text hello --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("acting"))
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a machine of the cluster; the fixture has none"),
        ex(JOBS, r#"dispatch <machine> --tool claude --folder <path> --text "<repo, branch, task>" --dry-run --json"#)
            .fill()
            .setup(Setup::SwitchOn("acting"))
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a machine of the cluster; the fixture has none"),
        ex(JOBS, r#"job <id> --wait --timeout 900 --json"#)
            .run(r#"job j-none --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"job <id> [--wait] [--timeout <seconds>] [--json] [--socket <path>]"#)
            .run(r#"job j-none --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"job return <id> --dry-run --json"#)
            .run(r#"job return j-none --dry-run --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"job return <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .run(r#"job return j-none --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"job stop <id> --dry-run --json"#)
            .run(r#"job stop j-none --dry-run --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"job stop <id> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .run(r#"job stop j-none --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"job tail <id> --dry-run --json"#)
            .run(r#"job tail j-none --dry-run --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"job tail <id> [--from <offset>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]"#)
            .run(r#"job tail j-none --reason ok --dry-run --json"#)
            .setup(Setup::SwitchOn("cluster"))
            .parse_only("it needs a job of the cluster; the fixture has none"),
        ex(JOBS, r#"jobs --json"#)
            .setup(Setup::SwitchOn("cluster")),
        ex(JOBS, r#"jobs [--json] [--socket <path>]"#)
            .setup(Setup::SwitchOn("cluster")),
    ]
}

pub(crate) fn cluster_rows() -> Vec<Example> {
    vec![
        ex(CLUSTER, r#"cluster allow <node> <secret>..."#)
            .run(r#"cluster allow node-1 github"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .parse_only("it needs a cluster; the fixture has none"),
        ex(CLUSTER, r#"cluster create <name>"#)
            .run(r#"cluster create acme-build"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .exit(5),
        ex(CLUSTER, r#"cluster create cluster"#)
            .run(r#"cluster create cluster"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .exit(2)
            .extra(),
        ex(CLUSTER, r#"cluster failover on|off|30m|1h"#)
            .run(r#"cluster failover on"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .parse_only("it needs a cluster; the fixture has none"),
        ex(
            CLUSTER,
            r#"cluster invite --node <id> --name <name> [--role master|worker] [--acting on|off]"#,
        )
        .run(r#"cluster invite --node node-1 --name build-mac"#)
        .setup(Setup::SwitchOn("cluster"))
        .setup(Setup::Deny)
        .exit(5)
        .parse_only("it needs a cluster; the fixture has none"),
        ex(CLUSTER, r#"cluster join --token-stdin [--json]"#)
            .parse_only("it reads a join token from stdin"),
        ex(CLUSTER, r#"cluster main <session> [on|off]"#)
            .run(r#"cluster main {self} on"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .parse_only("it needs a cluster; the fixture has none"),
        ex(
            CLUSTER,
            r#"cluster member <node> [--role master|worker] [--delegation on|off]"#,
        )
        .run(r#"cluster member node-1 --role worker"#)
        .setup(Setup::SwitchOn("cluster"))
        .setup(Setup::Deny)
        .exit(5)
        .parse_only("it needs a cluster; the fixture has none"),
        ex(CLUSTER, r#"cluster move-home <node>"#)
            .run(r#"cluster move-home node-1"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .parse_only("it needs a cluster; the fixture has none"),
        ex(CLUSTER, r#"cluster move-now"#)
            .run(r#"cluster move-now"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .parse_only("it needs a cluster; the fixture has none"),
        ex(CLUSTER, r#"cluster reach <addr> <port>"#)
            .parse_only("it connects to a network address"),
        ex(CLUSTER, r#"cluster remove <node>"#)
            .run(r#"cluster remove node-1"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .parse_only("it needs a cluster; the fixture has none"),
        ex(CLUSTER, r#"cluster status [--json]"#)
            .run(r#"cluster status --json"#)
            .setup(Setup::SwitchOn("cluster")),
        ex(CLUSTER, r#"cluster token"#)
            .run(r#"cluster token"#)
            .setup(Setup::SwitchOn("cluster"))
            .setup(Setup::Deny)
            .parse_only("it needs a cluster; the fixture has none"),
    ]
}

pub(crate) fn machines_rows() -> Vec<Example> {
    vec![
        ex(MACHINES, r#"machines add <user@host> [--name <text>]"#)
            .run(r#"machines add me@build-host --name build-mac"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .exit(5),
        ex(MACHINES, r#"machines add server"#)
            .run(r#"machines add me@build-host --name server"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .exit(2)
            .extra(),
        ex(MACHINES, r#"machines add me@host"#)
            .run(r#"machines add me@build-host"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .exit(5)
            .extra(),
        ex(MACHINES, r#"machines addresses <machine> <address>..."#)
            .run(r#"machines addresses m1 100.64.0.1"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .parse_only("it needs a registered machine; the fixture has none"),
        ex(
            MACHINES,
            r#"machines check [<machine>] [--json] [--socket <path>]"#,
        )
        .run(r#"machines check --json"#)
        .setup(Setup::SwitchOn("machines")),
        ex(MACHINES, r#"machines fix <machine> <step>"#)
            .run(r#"machines fix m1 trust"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .parse_only("it needs a registered machine; the fixture has none"),
        ex(MACHINES, r#"machines import <peer> --user <user>"#)
            .run(r#"machines import peer-1 --user me"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .parse_only("it reads the real Tailscale peers"),
        ex(MACHINES, r#"machines list [--json] [--socket <path>]"#)
            .run(r#"machines list --json"#)
            .setup(Setup::SwitchOn("machines")),
        ex(MACHINES, r#"machines peers [--json] [--socket <path>]"#)
            .parse_only("it reads the real Tailscale peers"),
        ex(
            MACHINES,
            r#"machines plan <machine> [<step>] [--json] [--socket <path>]"#,
        )
        .run(r#"machines plan m1 --json"#)
        .setup(Setup::SwitchOn("machines"))
        .parse_only("it needs a registered machine; the fixture has none"),
        ex(MACHINES, r#"machines remove <machine>"#)
            .run(r#"machines remove m1"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .parse_only("it needs a registered machine; the fixture has none"),
        ex(MACHINES, r#"machines running <machine> on|off"#)
            .run(r#"machines running m1 on"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .parse_only("it needs a registered machine; the fixture has none"),
        ex(MACHINES, r#"machines setup <machine>"#)
            .run(r#"machines setup m1"#)
            .setup(Setup::SwitchOn("machines"))
            .setup(Setup::Deny)
            .parse_only("it needs a registered machine; the fixture has none"),
        ex(MACHINES, r#"network list [--json] [--socket <path>]"#)
            .parse_only("it reads the real Tailscale networks"),
        ex(MACHINES, r#"network switch <tailnet>"#)
            .parse_only("it switches the real Tailscale network"),
    ]
}

pub(crate) fn accounts_rows() -> Vec<Example> {
    vec![
        ex(ACCOUNTS, r#"accounts --json"#).setup(Setup::SwitchOn("accounts")),
        ex(ACCOUNTS, r#"accounts [--json] [--socket <path>]"#)
            .run(r#"accounts --json"#)
            .setup(Setup::SwitchOn("accounts")),
        ex(ACCOUNTS, r#"accounts add claude --dry-run --json"#).setup(Setup::SwitchOn("accounts")),
        ex(ACCOUNTS, r#"accounts add claude|codex|agy"#)
            .run(r#"accounts add claude --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts")),
        ex(
            ACCOUNTS,
            r#"accounts move <session> <lane> --dry-run --json"#,
        )
        .run(r#"accounts move {self} lane-1 --dry-run --json"#)
        .setup(Setup::SwitchOn("accounts"))
        .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"accounts move <session> <lane>|main"#)
            .run(r#"accounts move {self} main --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"accounts move-all <account> --dry-run --json"#)
            .run(r#"accounts move-all acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"accounts move-all <account>|main"#)
            .run(r#"accounts move-all main --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"accounts remove <account>"#)
            .run(r#"accounts remove acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"accounts remove <account> --dry-run --json"#)
            .run(r#"accounts remove acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"accounts switch <account>"#)
            .run(r#"accounts switch acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"accounts switch <account> --dry-run --json"#)
            .run(r#"accounts switch acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"consent <id> --wait --timeout 600 --json"#)
            .run(r#"consent consent-none --timeout 1 --json"#)
            .exit(1),
        ex(ACCOUNTS, r#"lanes create <account>"#)
            .run(r#"lanes create acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"lanes create <account> --dry-run --json"#)
            .run(r#"lanes create acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"lanes list --json"#).setup(Setup::SwitchOn("accounts")),
        ex(ACCOUNTS, r#"lanes list [--json] [--socket <path>]"#)
            .run(r#"lanes list --json"#)
            .setup(Setup::SwitchOn("accounts")),
        ex(ACCOUNTS, r#"lanes remove <lane>"#)
            .run(r#"lanes remove lane-1 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"lanes remove <lane> --dry-run --json"#)
            .run(r#"lanes remove lane-1 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(ACCOUNTS, r#"lanes switch <lane> <account>"#)
            .run(r#"lanes switch lane-1 acct-1111111111111111 --dry-run --json"#)
            .setup(Setup::SwitchOn("accounts"))
            .parse_only("it needs a signed-in account or lane; the fixture has none"),
        ex(
            ACCOUNTS,
            r#"lanes switch <lane> <account> --dry-run --json"#,
        )
        .run(r#"lanes switch lane-1 acct-1111111111111111 --dry-run --json"#)
        .setup(Setup::SwitchOn("accounts"))
        .parse_only("it needs a signed-in account or lane; the fixture has none"),
    ]
}

pub(crate) const PAGES: &str = "notchling-pages";
pub(crate) const SETTINGS: &str = "notchling-settings";
pub(crate) const ASK: &str = "notchling-ask";
pub(crate) const VAULT: &str = "notchling-vault";
pub(crate) const DELEGATE: &str = "notchling-delegate";
pub(crate) const MODELS: &str = "notchling-models";
pub(crate) const GENERAL: &str = "notchling";
pub(crate) const SESSIONS: &str = "notchling-sessions";
pub(crate) const JOBS: &str = "notchling-jobs";
pub(crate) const ACCOUNTS: &str = "notchling-accounts";
pub(crate) const CLUSTER: &str = "notchling-cluster";
pub(crate) const MACHINES: &str = "notchling-machines";

pub(crate) fn table() -> Vec<Example> {
    let mut rows = vec![
        ex(PAGES, "page '<link | id | words>'").sub("<link | id | words>", "{page}"),
        ex(PAGES, "search <words> --kind pages").sub("<words>", "{words}"),
        ex(SETTINGS, "settings list"),
        ex(SETTINGS, "settings set <key> <value>")
            .sub("<key>", "chatHistory")
            .sub("<value>", "8mb"),
        ex(ASK, "asks --wait").parse_only("it blocks until another session asks a question"),
        ex(ASK, "answer <id> -- '<text>'")
            .sub("<id>", "{ask}")
            .sub("'<text>'", "'ok'")
            .setup(Setup::SwitchOn("acting"))
            .exit(4),
        ex(ASK, "answer <id> --choice <n>")
            .sub("<id>", "{ask}")
            .sub("<n>", "1")
            .setup(Setup::SwitchOn("acting"))
            .exit(4),
        ex(ASK, "ask [--choice '<option>']... [--main-may-answer] [--wait] [--timeout <seconds>] [--json] -- '<question>'")
            .sub("[--choice '<option>']...", "--choice yes --choice no")
            .sub("[--main-may-answer]", "")
            .sub("[--wait]", "--wait")
            .sub("[--timeout <seconds>]", "--timeout 10")
            .sub("'<question>'", "'Ship it?'")
            .setup(Setup::SwitchOn("acting"))
            .setup(Setup::Allow),
        ex(VAULT, "secret <name>[,<name>…] -- <command> [args…]").parse_only("the Keychain"),
        ex(VAULT, "secret github -- 'curl -sf -H \"Authorization: Bearer $GITHUB_TOKEN\" \"$GITHUB_URL/user\"'").parse_only("the Keychain"),
        ex(VAULT, "secret registry -- 'printf %s \"$REGISTRY_PASSWORD\" | docker login -u \"$REGISTRY_USERNAME\" --password-stdin \"$REGISTRY_URL\"'").parse_only("the Keychain"),
        ex(VAULT, "secret-request <name> --kind token|login --url '<url>' --note '<what it is for>' --how '<steps to get it>' --expires YYYY-MM-DD --reason '<why you need it now>'").parse_only("the Keychain"),
        ex(VAULT, "secrets '<URL, host or words from the task>'").parse_only("the Keychain"),
        ex(DELEGATE, "delegate <codex|agy> [--model <m>] [--effort <e>] [--folder <dir>] [--out <dir>] [--write] [--session <id>] [--timeout <s>] [--json] -- '<prompt>'").parse_only("it starts another AI tool"),
        ex(DELEGATE, "delegate agy --model <model> --effort high --folder . -- 'Review the uncommitted changes (git diff HEAD) for bugs. List each with file:line and why. Do not change files.'").parse_only("it starts another AI tool"),
        ex(DELEGATE, "delegate codex --model <model> --effort high --write --folder ./site -- 'Add a dark mode to styles.css.'").parse_only("it starts another AI tool"),
        ex(DELEGATE, "delegate codex --out ./assets -- 'Generate a 1024x1024 app icon: a paper plane, flat, blue on white.'").parse_only("it starts another AI tool"),
        ex(MODELS, "models            # check: read the lists, change nothing of Notchling's, report").parse_only("it reads real tool lists"),
        ex(MODELS, "models --json     # the same report as JSON (also: models update --json)").parse_only("it reads real tool lists"),
        ex(MODELS, "models update     # read the lists, update Notchling's copy, report (the app rewrites the skills)").parse_only("it reads real tool lists"),
    ];
    rows.extend(general_rows());
    rows.extend(sessions_rows());
    rows.extend(jobs_rows());
    rows.extend(cluster_rows());
    rows.extend(machines_rows());
    rows.extend(accounts_rows());
    rows
}

pub(crate) fn general_rows() -> Vec<Example> {
    vec![
        ex(GENERAL, "activity [--kind <event>]... [--since <ms>] [--limit <n>] [--json]")
            .sub("[--kind <event>]...", "")
            .sub("[--since <ms>]", "--since 0")
            .sub("[--limit <n>]", "--limit 5"),
        ex(GENERAL, "capabilities [--json] [--socket <path>]"),
        ex(GENERAL, "chat open-file <path> [--session <id>] [--json] [--socket <path>]")
            .sub("<path>", "/etc/hosts")
            .sub("[--session <id>]", "")
            .setup(Setup::SwitchOn("acting")),
        ex(GENERAL, "consent <id> [--wait] [--timeout <seconds>] [--json]")
            .sub("<id>", "consent-none")
            .sub("[--wait]", "")
            .sub("[--timeout <seconds>]", "")
            .exit(1),
        ex(GENERAL, "consent cancel <id> [--json]").sub("<id>", "consent-none").exit(1),
        ex(GENERAL, "consents [--json]"),
        ex(GENERAL, "cost check [--json]"),
        ex(GENERAL, "cost profile set|setup|undo [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]")
            .sub("set|setup|undo", "setup")
            .acting("--reason 'check the model table'")
            .setup(Setup::SwitchOn("install")),
        ex(GENERAL, "cost report [--days N] [--plan api|subscription|auto] [--json]")
            .sub("[--days N]", "--days 7")
            .sub("[--plan api|subscription|auto]", "--plan auto"),
        ex(GENERAL, "cost setup [--apply] [--consent a,b] [--json]")
            .sub("[--apply]", "")
            .sub("[--consent a,b]", ""),
        ex(GENERAL, "cost undo <claude|codex|agy> [--json]")
            .sub("<claude|codex|agy>", "claude")
            .exit(4),
        ex(GENERAL, "help [--json]"),
        ex(GENERAL, "inbox [--json] [--socket <path>]"),
        ex(GENERAL, "install plan [--what <a,b>] [--json] [--socket <path>]")
            .sub("[--what <a,b>]", "--what notchling-ask,notchling"),
        ex(GENERAL, "install request [--part <id>]... [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]")
            .sub("[--part <id>]...", "--part notchling-ask")
            .acting("--reason 'install the skills'")
            .setup(Setup::SwitchOn("install")),
        ex(GENERAL, "orgs [--json] [--socket <path>]"),
        ex(GENERAL, "providers [--json]"),
        ex(GENERAL, "roles [--json] [--socket <path>]"),
        ex(GENERAL, "status [--json] [--socket <path>]"),
        ex(GENERAL, "switch off <acting|bridge|accounts|vault|install|cluster|machines> [--json]")
            .sub("<acting|bridge|accounts|vault|install|cluster|machines>", "vault"),
        ex(GENERAL, "switch on <acting|bridge|accounts|vault|install|cluster|machines> [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]")
            .sub("<acting|bridge|accounts|vault|install|cluster|machines>", "vault")
            .sub("[--reason <text>]", "--reason 'the task needs the vault'")
            .sub("[--no-wait]", "")
            .sub("[--timeout <seconds>]", "--timeout 10")
            .sub("[--expires-in <seconds>]", "")
            .sub("[--key <text>]", "")
            .sub("[--retry-of <id>]", "")
            .sub("[--dry-run]", "")
            .setup(Setup::Allow),
        ex(GENERAL, "switches [--json] [--socket <path>]"),
        ex(GENERAL, "versions [--latest] [--workers] [--json] [--socket <path>]")
            .sub("[--latest]", "")
            .sub("[--workers]", ""),
        ex(GENERAL, "versions update <tool> [--to <version>] [--worker <machine>] [--reason <text>] [--no-wait] [--timeout <seconds>] [--expires-in <seconds>] [--key <text>] [--retry-of <id>] [--dry-run] [--json]")
            .parse_only("it needs an installed tool and the newest version from the network"),
        ex(GENERAL, "watch [--topic <topic>]... [--until <state>] [--count <n>] [--timeout <seconds>] [--json]")
            .sub("[--topic <topic>]...", "--topic mine.consents")
            .sub("[--until <state>]", "")
            .sub("[--count <n>]", "--count 1")
            .sub("[--timeout <seconds>]", "--timeout 1")
            .exit(124),
    ]
}
