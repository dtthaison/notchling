//! Strings for ssh and the shell on a machine (Swift `SSH`, `RemoteShell`, `MachinePaths`,
//! `MachineTools`). Everything a machine reports is untrusted: only a value that passed
//! [`is_safe_machine_path`] may enter a script or a file.

use super::registry::{AdminAccess, Machine, MachineOs};
use crate::lanes::quote;

/// The PATH of a remote script (a non-interactive ssh command gets a short one, without mise or
/// cargo).
pub const PATH_VALUE: &str = "$HOME/.local/bin:$HOME/.cargo/bin:$HOME/.local/share/mise/shims:/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin";

/// Starts every worker script.
pub fn prelude() -> String {
    format!("export PATH=\"{PATH_VALUE}\"; ")
}

/// The PATH an admin script runs with. Nothing the worker account owns is on it.
pub const ADMIN_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin";

/// Services on a worker machine that no step ever touches.
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

/// `ssh` with no prompts and a short connect timeout, running `remote` on the machine; each word
/// is quoted, so a path with a space stays one argument.
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

/// A script run by `/bin/sh` on the machine with the worker PATH. The script's own stdin is ssh's:
/// a value to store goes there, never into the script.
pub fn ssh_script(m: &Machine, script: &str) -> Vec<String> {
    let mut v = ssh_head(format!("{}@{}", m.user, m.host));
    v.push(format!(
        "/bin/sh -c {}",
        quote(&format!("{}{script}", prelude()))
    ));
    v
}

/// The home of the account an admin script runs as (root).
pub fn admin_home(m: &Machine) -> &'static str {
    if m.os == Some(MachineOs::MacOs) {
        "/var/root"
    } else {
        "/root"
    }
}

/// Why an admin step can't run on `m`, or `None`. On Linux the worker account must not be the
/// admin: it can plant a shell file, and a password typed into its ssh session passes through the
/// shell files it owns.
pub fn admin_refusal(m: &Machine) -> Option<&'static str> {
    (m.os == Some(MachineOs::Linux) && m.admin() == AdminAccess::Sudo).then_some(
        "On Linux the worker account can't be the admin: its own shell files could read the password. Use root over SSH or another admin account.",
    )
}

/// A script that needs admin rights, built so that nothing the worker account can change decides
/// what runs: `/usr/bin/sudo` by absolute path, and the script under `env -i` with a fixed PATH.
/// Through sudo the password is the first line of ssh's stdin (`sudo -S`); the script itself reads
/// no stdin. As root over ssh there is no sudo at all.
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

/// For a `git fetch` over ssh: no prompts and a short connect timeout.
pub const GIT_SSH_COMMAND: &str = "ssh -o BatchMode=yes -o ConnectTimeout=8";

pub fn not_absolute(path: &str) -> String {
    format!("the machine's worktree path \"{path}\" is not an absolute path")
}

/// Where `git` reaches a path on the machine, or `None` for a path that isn't absolute (`.evil.com/x`
/// would make the host `m.host.evil.com`).
pub fn git_url(m: &Machine, path: &str) -> Option<String> {
    path.starts_with('/')
        .then(|| format!("ssh://{}@{}{path}", m.user, m.host))
}

/// The PATH for a service unit, which has no `$HOME` to expand.
pub fn service_path(home: &str) -> String {
    PATH_VALUE.replace("$HOME", home)
}

/// `~` and `~/rest` expand on the machine (`$HOME` in double quotes, the rest single-quoted); any
/// other path is one single-quoted word, so `~other/x` stays literal.
pub fn path(p: &str) -> String {
    if p == "~" {
        return "\"$HOME\"".into();
    }
    if let Some(rest) = p.strip_prefix("~/") {
        return format!("\"$HOME\"/{}", quote(rest));
    }
    quote(p)
}

/// Whether a path a machine reported may go into a script or a file: absolute, plain characters
/// only, no `..`.
pub fn is_safe_machine_path(p: &str) -> bool {
    if !p.starts_with('/') || p.len() > 512 {
        return false;
    }
    let plain = p
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-/+@".contains(c));
    plain && !p.split('/').any(|part| part == "..")
}

/// Reads stdin into `p` with `mode`, through `p.new`, so a half-written file never sits at `p`.
/// The umask makes the new file private from its first byte.
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

/// One line per path: `mode size`, or `missing`.
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

/// The SHA-256 of a file as 64 hex characters on one line, nothing for a missing file. The content
/// itself is never printed.
pub fn sha256(p: &str, linux: bool) -> String {
    let tool = if linux { "sha256sum" } else { "shasum -a 256" };
    format!(
        "if [ -f {} ]; then {tool} {} 2>/dev/null | cut -d' ' -f1; fi",
        path(p),
        path(p)
    )
}

/// The mode and size of a file on a machine; its content is never read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFacts {
    pub mode: String,
    pub size: u64,
}

/// The facts for each of `count` paths, `None` for a missing file or a line that isn't `mode size`.
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

/// Files on the machine, home-relative: [`path`] expands the `~` for the worker. Under sudo or as
/// root `~` is someone else's home, so an admin step takes [`absolute`] with the worker's home.
pub const CLAUDE_ENV: &str = "~/.config/agents/claude.env";
pub const GITHUB_TOKEN: &str = "~/.config/agents/github-token";
pub const WORKER_JSON: &str = "~/.config/sd/worker.json";
pub const CLAUDE_KEY: &str = "CLAUDE_CODE_OAUTH_TOKEN";

pub fn sd_py(home: &str) -> String {
    format!("{home}/.local/share/agents/system-design/plugin/scripts/sd.py")
}

/// `path` with a leading `~` replaced by `home`; an absolute path as it is. `None` when `home`
/// isn't a safe path (it came from the machine).
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

/// Where a tool on this Mac lives (a menu bar app's PATH has no Homebrew).
pub fn tool_path(name: &str) -> String {
    for dir in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        let candidate = format!("{dir}/{name}");
        if std::path::Path::new(&candidate).is_file() {
            return candidate;
        }
    }
    format!("/usr/bin/{name}")
}
