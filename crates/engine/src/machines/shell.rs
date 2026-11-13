use super::registry::{AdminAccess, Machine, MachineOs};
use crate::lanes::quote;

pub const PATH_VALUE: &str = "$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

pub fn prelude() -> String {
    format!("export PATH=\"{PATH_VALUE}\"; ")
}

pub const ADMIN_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

pub const PRODUCTION_SERVICES: [&str; 7] = [
    "lingeva-app",
    "n8n",
    "inv3str",
    "postgres",
    "yuribot",
    "nginx",
    "caddy",
];

fn ssh_head(login: String) -> Vec<String> {
    let mut v: Vec<String> = [
        "/usr/bin/ssh",
        "-o",
        "BatchMode=yes",
        "-o",
        "ConnectTimeout=8",
        "--",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    v.push(login);
    v
}

pub fn ssh_argv(m: &Machine, remote: &[&str]) -> Vec<String> {
    let mut v = ssh_head(format!("{}@{}", m.user, m.host));
    v.push(
        remote
            .iter()
            .map(|w| quote(w))
            .collect::<Vec<_>>()
            .join(" "),
    );
    v
}

pub fn ssh_script(m: &Machine, script: &str) -> Vec<String> {
    let mut v = ssh_head(format!("{}@{}", m.user, m.host));
    v.push(format!(
        "/bin/sh -c {}",
        quote(&format!("{}{script}", prelude()))
    ));
    v
}

pub fn admin_home(m: &Machine) -> &'static str {
    if m.os == Some(MachineOs::MacOs) {
        "/var/root"
    } else {
        "/root"
    }
}

pub fn admin_refusal(m: &Machine) -> Option<&'static str> {
    (m.os == Some(MachineOs::Linux) && m.admin() == AdminAccess::Sudo).then_some(
        "On Linux the worker account can't be the admin: its own shell files could read the password. Use root over SSH or another admin account.",
    )
}

pub fn ssh_admin(m: &Machine, script: &str, as_root: bool) -> Vec<String> {
    let user = if as_root { "root" } else { m.user.as_str() };
    let body = format!("exec </dev/null; {script}");
    let fixed = format!(
        "/usr/bin/env -i PATH={ADMIN_PATH} HOME={} /bin/sh -c {}",
        admin_home(m),
        quote(&body)
    );
    let remote = if as_root {
        fixed
    } else {
        format!("/usr/bin/sudo -S -k -p '' {fixed}")
    };
    let mut v = ssh_head(format!("{user}@{}", m.host));
    v.push(format!("/bin/sh -c {}", quote(&remote)));
    v
}

pub const GIT_SSH_COMMAND: &str = "ssh -o BatchMode=yes -o ConnectTimeout=8";

pub fn not_absolute(path: &str) -> String {
    format!("the machine's worktree path \"{path}\" is not an absolute path")
}

pub fn git_url(m: &Machine, path: &str) -> Option<String> {
    path.starts_with('/')
        .then(|| format!("ssh://{}@{}{path}", m.user, m.host))
}

pub fn service_path(home: &str) -> String {
    PATH_VALUE.replace("$HOME", home)
}

pub fn path(p: &str) -> String {
    if p == "~" {
        return "\"$HOME\"".into();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return format!("\"$HOME\"/{}", quote(rest));
    }
    quote(p)
}

pub fn is_safe_machine_path(p: &str) -> bool {
    if !p.starts_with('/') || p.len() > 512 {
        return false;
    }
    let plain = p
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/+@".contains(c));
    plain && !p.split('/').any(|part| part == "..")
}

pub fn write_file(p: &str, mode: &str) -> String {
    let dir = match p.rfind('/') {
        Some(0) => "/",
        Some(i) => &p[..i],
        None => ".",
    };
    let temp = path(&format!("{p}.new"));
    format!(
        "umask 077; mkdir -p {} && cat > {temp} && chmod {mode} {temp} && mv -f {temp} {}",
        path(dir),
        path(p)
    )
}

pub fn stat(paths: &[&str], linux: bool) -> String {
    let format = if linux {
        "stat -c '%a %s'"
    } else {
        "stat -f '%Lp %z'"
    };
    paths
        .iter()
        .map(|p| {
            format!(
                "if [ -f {} ]; then {format} {}; else echo missing; fi",
                path(p),
                path(p)
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub fn sha256(p: &str, linux: bool) -> String {
    let tool = if linux { "sha256sum" } else { "shasum -a 256" };
    format!(
        "if [ -f {} ]; then {tool} {} 2>/dev/null | cut -d' ' -f1; fi",
        path(p),
        path(p)
    )
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFacts {
    pub mode: String,
    pub size: u64,
}

pub fn parse_stat(out: &str, count: usize) -> Vec<Option<FileFacts>> {
    let lines: Vec<&str> = out.split('\n').collect();
    (0..count)
        .map(|i| {
            let words: Vec<&str> = lines.get(i)?.split(' ').filter(|w| !w.is_empty()).collect();
            let [mode, size] = words.as_slice() else {
                return None;
            };
            let octal =
                (1..=4).contains(&mode.len()) && mode.chars().all(|c| ('0'..='7').contains(&c));
            let size: u64 = size.parse().ok()?;
            octal.then(|| FileFacts {
                mode: mode.to_string(),
                size,
            })
        })
        .collect()
}

pub const CLAUDE_ENV: &str = "~/.config/agents/claude.env";
pub const GITHUB_TOKEN: &str = "~/.config/agents/github-token";
pub const WORKER_JSON: &str = "~/.config/sd/worker.json";
pub const CLAUDE_KEY: &str = "CLAUDE_CODE_OAUTH_TOKEN";

pub fn sd_py(home: &str) -> String {
    format!("{home}/.local/share/agents/system-design/plugin/scripts/sd.py")
}

pub fn absolute(p: &str, home: &str) -> Option<String> {
    if !is_safe_machine_path(home) {
        return None;
    }
    if p == "~" {
        return Some(home.into());
    }
    if let Some(rest) = p.strip_prefix('~') {
        if rest.starts_with('/') {
            return Some(format!("{home}{rest}"));
        }
    }
    Some(p.into())
}

pub fn tool_path(name: &str) -> String {
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let candidate = format!("{dir}/{name}");
        if std::path::Path::new(&candidate).is_file() {
            return candidate;
        }
    }
    format!("/usr/bin/{name}")
}
