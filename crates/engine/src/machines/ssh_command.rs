use super::forms::{is_host, is_user};
use super::ssh_key::shape_ok;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SshTarget {
    pub user: Option<String>,
    pub host: String,
    pub port: Option<u16>,
    pub key: Option<String>,
    pub jump: Option<String>,
}

impl SshTarget {
    pub fn canonical(&self) -> String {
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let body = match self.port {
            Some(port) => format!("{host}:{port}"),
            None => host,
        };
        match &self.user {
            Some(user) => format!("{user}@{body}"),
            None => body,
        }
    }
}

const EMPTY: &str = "Paste an ssh command or type user@host:port";
const SHAPE: &str = "Start with ssh, or type user@host:port";
const SHELL: &str = "Use plain words: no quotes, variables, pipes or redirects";
const NO_HOST: &str = "The command names no machine: add user@host";
const HOST: &str = "The host is a name or an IP address";
const USER: &str = "The user is lowercase letters, digits, hyphens and underscores";
const PORT: &str = "The port is a number from 1 to 65535";
const BRACKET: &str = "Close the IPv6 address's bracket";
const KEY: &str = "The key is the full path of a file, or one starting with ~/";
const ONE_JUMP: &str = "Only one jump host is supported";
const ONE_KEY: &str = "Only one key is supported";
const PATH: &str = "The address can't hold a path";

const PLAIN_FLAGS: [&str; 7] = ["-4", "-6", "-C", "-q", "-v", "-vv", "-vvv"];

#[derive(Default)]
struct Found {
    user: Option<String>,
    port: Option<u16>,
    key: Option<String>,
    jump: Option<String>,
}

struct Dest {
    user: Option<String>,
    host: String,
    port: Option<u16>,
}

pub fn parse_ssh_command(text: &str) -> Result<SshTarget, String> {
    if text.chars().any(char::is_control) {
        return Err("The address can't hold control characters".into());
    }
    if text.chars().any(|c| "\"'`$;|&<>(){}*?!\\#".contains(c)) {
        return Err(SHELL.into());
    }
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let Some(first) = tokens.first().copied() else {
        return Err(EMPTY.into());
    };
    if first == "ssh" {
        return command(&tokens[1..]);
    }
    if tokens.len() == 1 {
        let dest = if first.to_ascii_lowercase().starts_with("ssh://") {
            uri(first)?
        } else {
            destination(first)?
        };
        return Ok(SshTarget {
            user: dest.user,
            host: dest.host,
            port: dest.port,
            key: None,
            jump: None,
        });
    }
    Err(SHAPE.into())
}

fn command(args: &[&str]) -> Result<SshTarget, String> {
    let mut found = Found::default();
    let mut dest: Option<&str> = None;
    let mut options_done = false;
    let mut at = 0;
    while at < args.len() {
        let arg = args[at];
        at += 1;
        if !options_done && arg == "--" {
            options_done = true;
        } else if !options_done && arg.starts_with('-') && arg.len() > 1 {
            option(arg, args, &mut at, &mut found)?;
        } else if dest.is_none() {
            dest = Some(arg);
        } else {
            return Err(format!(
                "Remove \"{arg}\": Notchling needs the address, not a command to run"
            ));
        }
    }
    let Some(dest) = dest else {
        return Err(NO_HOST.into());
    };
    let dest = if dest.to_ascii_lowercase().starts_with("ssh://") {
        uri(dest)?
    } else {
        destination(dest)?
    };
    let user = merged(dest.user, found.user, "user")?;
    let port = merged(dest.port, found.port, "port")?;
    Ok(SshTarget {
        user,
        host: dest.host,
        port,
        key: found.key,
        jump: found.jump,
    })
}

fn merged<T: PartialEq>(a: Option<T>, b: Option<T>, what: &str) -> Result<Option<T>, String> {
    match (a, b) {
        (Some(a), Some(b)) if a != b => Err(format!("The {what} is given twice, differently")),
        (a, b) => Ok(a.or(b)),
    }
}

fn option(arg: &str, args: &[&str], at: &mut usize, found: &mut Found) -> Result<(), String> {
    if PLAIN_FLAGS.contains(&arg) {
        return Ok(());
    }
    let flag: String = if arg.starts_with("--") {
        arg.to_owned()
    } else {
        arg.chars().take(2).collect()
    };
    if !matches!(flag.as_str(), "-p" | "-i" | "-l" | "-J" | "-o") {
        return Err(format!("Notchling doesn't use the ssh option {flag}"));
    }
    let attached = &arg[2..];
    let value = if attached.is_empty() {
        let Some(next) = args.get(*at) else {
            return Err(format!("The ssh option {flag} needs a value"));
        };
        *at += 1;
        (*next).to_owned()
    } else {
        attached.to_owned()
    };
    match flag.as_str() {
        "-p" => set_port(found, &value),
        "-i" => set_key(found, &value),
        "-l" => set_user(found, &value),
        "-J" => set_jump(found, &value),
        _ => named_option(found, &value),
    }
}

fn named_option(found: &mut Found, value: &str) -> Result<(), String> {
    let Some((name, value)) = value.split_once('=') else {
        return Err("The ssh option -o needs Name=Value".into());
    };
    match name.to_ascii_lowercase().as_str() {
        "port" => set_port(found, value),
        "user" => set_user(found, value),
        "identityfile" => set_key(found, value),
        "proxyjump" => set_jump(found, value),
        _ => Err(format!("Notchling doesn't use the ssh option -o {name}")),
    }
}

fn set_port(found: &mut Found, value: &str) -> Result<(), String> {
    if found.port.is_some() {
        return Err("The port is given twice".into());
    }
    found.port = Some(port_of(value)?);
    Ok(())
}

fn set_user(found: &mut Found, value: &str) -> Result<(), String> {
    if found.user.is_some() {
        return Err("The user is given twice".into());
    }
    if !is_user(value) {
        return Err(USER.into());
    }
    found.user = Some(value.to_owned());
    Ok(())
}

fn set_key(found: &mut Found, value: &str) -> Result<(), String> {
    if found.key.is_some() {
        return Err(ONE_KEY.into());
    }
    if !shape_ok(value) {
        return Err(KEY.into());
    }
    found.key = Some(value.to_owned());
    Ok(())
}

fn set_jump(found: &mut Found, value: &str) -> Result<(), String> {
    if found.jump.is_some() || value.contains(',') {
        return Err(ONE_JUMP.into());
    }
    found.jump = Some(destination(value)?.host);
    Ok(())
}

fn port_of(text: &str) -> Result<u16, String> {
    text.parse::<u16>()
        .ok()
        .filter(|p| *p > 0)
        .ok_or_else(|| PORT.into())
}

fn uri(text: &str) -> Result<Dest, String> {
    let rest = &text["ssh://".len()..];
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    if rest.contains('/') {
        return Err(PATH.into());
    }
    destination(rest)
}

fn destination(text: &str) -> Result<Dest, String> {
    let (user, rest) = match text.split_once('@') {
        Some((user, rest)) => {
            if rest.contains('@') {
                return Err(HOST.into());
            }
            if !is_user(user) {
                return Err(USER.into());
            }
            (Some(user.to_owned()), rest)
        }
        None => (None, text),
    };
    if rest.contains('/') {
        return Err(PATH.into());
    }
    let (host, port) = host_and_port(rest)?;
    if !is_host(&host) {
        return Err(HOST.into());
    }
    Ok(Dest { user, host, port })
}

fn host_and_port(rest: &str) -> Result<(String, Option<u16>), String> {
    if let Some(inner) = rest.strip_prefix('[') {
        let Some((host, after)) = inner.split_once(']') else {
            return Err(BRACKET.into());
        };
        return match after {
            "" => Ok((host.to_owned(), None)),
            _ => match after.strip_prefix(':') {
                Some(port) => Ok((host.to_owned(), Some(port_of(port)?))),
                None => Err(HOST.into()),
            },
        };
    }
    match rest.matches(':').count() {
        0 => Ok((rest.to_owned(), None)),
        1 => {
            let (host, port) = rest.split_once(':').unwrap_or((rest, ""));
            Ok((host.to_owned(), Some(port_of(port)?)))
        }
        _ => Ok((rest.to_owned(), None)),
    }
}
