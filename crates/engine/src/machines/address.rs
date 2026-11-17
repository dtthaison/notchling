use super::forms::machine_problems;
use std::net::{Ipv4Addr, Ipv6Addr};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Address {
    pub name: String,
    pub host: String,
    pub user: String,
}

const ROOT: &str =
    "Notchling runs as its own account; give that account, for example agent@vps-sing-1";
const PORT: &str = "A port isn't supported; use the machine's default SSH port";
const SHAPE: &str = "Type the address as user@host";

pub fn parse(typed: &str, this_user: &str, existing: &[String]) -> Result<Address, String> {
    if typed.chars().any(char::is_control) {
        return Err("The address can't hold control characters".into());
    }
    let typed = typed.trim();
    if typed.chars().any(char::is_whitespace) {
        return Err("The address can't hold spaces".into());
    }
    if typed.is_empty() {
        return Err(SHAPE.into());
    }
    if typed.to_ascii_lowercase().starts_with("ssh://") {
        return Err("Type user@host, without ssh://".into());
    }
    if typed.contains('/') {
        return Err("The address can't hold a path".into());
    }
    let (user, host) = match typed.split_once('@') {
        Some((user, host)) => {
            if user.is_empty() || host.is_empty() || host.contains('@') {
                return Err(SHAPE.into());
            }
            (user, host)
        }
        None => (this_user, typed),
    };
    if user == "root" {
        return Err(ROOT.into());
    }
    let (host, v6) = unbracket(host)?;
    if !v6 && host.contains(':') {
        return Err(PORT.into());
    }
    let name = short_name(&host, v6);
    let user = user.to_string();
    if let Some(why) = machine_problems(&name, &host, &user, existing)
        .into_iter()
        .next()
    {
        return Err(why);
    }
    Ok(Address { name, host, user })
}

fn unbracket(host: &str) -> Result<(String, bool), String> {
    let Some(rest) = host.strip_prefix('[') else {
        return Ok((host.to_string(), false));
    };
    let Some((inner, after)) = rest.split_once(']') else {
        return Err("Close the IPv6 address's bracket".into());
    };
    if !after.is_empty() {
        return Err(if after.starts_with(':') {
            PORT.into()
        } else {
            SHAPE.into()
        });
    }
    if inner.parse::<Ipv6Addr>().is_err() {
        return Err("That isn't an IPv6 address".into());
    }
    Ok((inner.to_string(), true))
}

fn short_name(host: &str, v6: bool) -> String {
    if v6 {
        let last = host.rsplit(':').next().unwrap_or("").to_ascii_lowercase();
        let last = last.trim_start_matches('0');
        return if last.is_empty() {
            "machine".into()
        } else {
            format!("machine-{last}")
        };
    }
    if let Ok(v4) = host.parse::<Ipv4Addr>() {
        return format!("machine-{}", v4.octets()[3]);
    }
    let label = host.split('.').next().unwrap_or("").to_ascii_lowercase();
    let mapped: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_lowercase() || c.is_ascii_digit() {
                c
            } else {
                '-'
            }
        })
        .collect();
    mapped.trim_matches('-').chars().take(40).collect()
}
