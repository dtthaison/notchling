use super::shell;
use super::steps_sd::MIN_SD;
use crate::versions::knob::{AGY_KNOB_VERIFIED, AGY_VAR, CLAUDE_VAR};
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServiceFile {
    pub name: String,
    pub remote_path: String,
    pub content: String,
}

pub const LINUX_NAMES: [&str; 4] = [
    "sd-worker.slice",
    "sd-worker.service",
    "sd-testflight.service",
    "sd-testflight.timer",
];
pub const PLIST_NAME: &str = "com.sd.worker.plist";
pub const UNIT_DIRECTORY: &str = "/etc/systemd/system";
pub const PLIST_PATH: &str = "~/Library/LaunchAgents/com.sd.worker.plist";

pub fn service_vars(agy_verified: bool) -> Vec<(&'static str, &'static str)> {
    let mut vars = vec![(CLAUDE_VAR, "1")];
    if agy_verified {
        vars.push((AGY_VAR, "true"));
    }
    vars
}

pub fn templates(dir: &Path) -> Option<BTreeMap<String, String>> {
    let mut found = BTreeMap::new();
    for name in LINUX_NAMES.iter().chain(std::iter::once(&PLIST_NAME)) {
        let text = std::fs::read_to_string(dir.join(name)).ok()?;
        found.insert((*name).to_string(), text);
    }
    Some(found)
}

pub fn render(
    templates: &BTreeMap<String, String>,
    linux: bool,
    user: &str,
    home: &str,
    python: &str,
    sd_path: &str,
) -> Result<Vec<ServiceFile>, String> {
    if user == "root" {
        return Err("the worker must not run as root".into());
    }
    if !is_safe_user(user) {
        return Err("the worker user name isn't a plain account name".into());
    }
    if !shell::is_safe_machine_path(home) {
        return Err("the home folder isn't a safe path".into());
    }
    if !shell::is_safe_machine_path(python) || !shell::is_safe_machine_path(sd_path) {
        return Err("python or sd.py is not an absolute path yet".into());
    }
    let names: Vec<&str> = if linux {
        LINUX_NAMES.to_vec()
    } else {
        vec![PLIST_NAME]
    };
    let vars = service_vars(AGY_KNOB_VERIFIED);
    let mut files = Vec::new();
    for name in names {
        let template = templates
            .get(name)
            .ok_or_else(|| format!("sd on this Mac has no {name} template"))?;
        if template
            .split('\n')
            .any(|l| l.trim().starts_with("EnvironmentFile="))
        {
            return Err(format!(
                "sd on this Mac is older than {MIN_SD}; update it first"
            ));
        }
        let mut text = template
            .replace("@SD_PY@", sd_path)
            .replace("@PYTHON3@", python)
            .replace("@HOME@", home);
        if linux {
            text = set_lines(&text, user, &shell::service_path(home), &vars);
            if name.ends_with(".service") && !text.split('\n').any(|l| l == format!("User={user}"))
            {
                return Err(format!("{name} has no User= line to set"));
            }
        } else {
            text = plist(&text, &shell::service_path(home), &vars)
                .ok_or_else(|| format!("{name} is not a plist this Notchling can fill in"))?;
        }
        if let Some(left) = leftover_placeholder(&text) {
            return Err(format!("{name} still has the placeholder {left}"));
        }
        files.push(ServiceFile {
            name: name.into(),
            remote_path: if linux {
                format!("{UNIT_DIRECTORY}/{name}")
            } else {
                PLIST_PATH.into()
            },
            content: text,
        });
    }
    Ok(files)
}

fn is_safe_user(user: &str) -> bool {
    let mut c = user.chars();
    (1..=32).contains(&user.len())
        && c.next().is_some_and(|f| f == '_' || f.is_ascii_lowercase())
        && c.all(|c| c == '_' || c == '-' || c.is_ascii_lowercase() || c.is_ascii_digit())
}

fn leftover_placeholder(text: &str) -> Option<&str> {
    let bytes = text.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b != b'@' {
            continue;
        }
        let run = bytes[i + 1..]
            .iter()
            .take_while(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || **c == b'_')
            .count();
        if run > 0 && bytes.get(i + 1 + run) == Some(&b'@') {
            return Some(&text[i..i + run + 2]);
        }
    }
    None
}

fn set_lines(text: &str, user: &str, path: &str, vars: &[(&str, &str)]) -> String {
    let have: Vec<&str> = text.split('\n').collect();
    let mut out: Vec<String> = Vec::new();
    for line in &have {
        if *line == "User=agent" {
            out.push(format!("User={user}"));
        } else if line.starts_with("Environment=PATH=") {
            out.push(format!("Environment=PATH={path}"));
            for (name, value) in vars {
                let own = format!("Environment={name}=");
                if !have.iter().any(|l| l.starts_with(&own)) {
                    out.push(format!("{own}{value}"));
                }
            }
        } else {
            out.push((*line).to_string());
        }
    }
    out.join("\n")
}

fn without_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("<!--") {
        let Some(len) = rest[start + 4..].find("-->") else {
            break;
        };
        out.push_str(&rest[..start]);
        rest = rest[start + 4 + len + 3..].trim_start();
    }
    out.push_str(rest);
    out
}

fn plist(text: &str, path: &str, vars: &[(&str, &str)]) -> Option<String> {
    let mut text = without_comments(text);
    let key = text.find("<key>PATH</key>")? + "<key>PATH</key>".len();
    let open = key + text[key..].find("<string>")? + "<string>".len();
    let close = open + text[open..].find("</string>")?;
    text.replace_range(open..close, path);
    let mut at = open + path.len() + "</string>".len();
    for (name, value) in vars {
        if text.contains(&format!("<key>{name}</key>")) {
            continue;
        }
        let pair = format!("\n\t\t<key>{name}</key>\n\t\t<string>{value}</string>");
        text.insert_str(at, &pair);
        at += pair.len();
    }
    well_formed(&text).then_some(text)
}

fn well_formed(text: &str) -> bool {
    let mut stack: Vec<&str> = Vec::new();
    let mut rest = text;
    let mut root_seen = false;
    while let Some(lt) = rest.find('<') {
        rest = &rest[lt + 1..];
        let Some(gt) = rest.find('>') else {
            return false;
        };
        let tag = &rest[..gt];
        rest = &rest[gt + 1..];
        if tag.starts_with('?') || tag.starts_with('!') || tag.ends_with('/') {
            continue;
        }
        if let Some(close) = tag.strip_prefix('/') {
            if stack.pop() != Some(close.trim()) {
                return false;
            }
            continue;
        }
        let name = tag.split_whitespace().next().unwrap_or("");
        if stack.is_empty() {
            if root_seen || name != "plist" {
                return false;
            }
            root_seen = true;
        }
        stack.push(name);
    }
    root_seen && stack.is_empty()
}
