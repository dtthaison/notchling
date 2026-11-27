use super::{can_pin, Method, Tool, Version};

pub const CODEX_INSTALL_URL: &str = "https://chatgpt.com/codex/install.sh";

fn package(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Claude => Some("@anthropic-ai/claude-code"),
        Tool::Codex => Some("@openai/codex"),
        Tool::Agy | Tool::Notchling => None,
    }
}

fn cask(tool: Tool) -> Option<&'static str> {
    match tool {
        Tool::Claude => Some("claude-code"),
        Tool::Codex => Some("codex"),
        Tool::Agy | Tool::Notchling => None,
    }
}

fn args(parts: &[&str]) -> Vec<String> {
    parts.iter().map(|p| p.to_string()).collect()
}

pub fn update_argv(
    tool: Tool,
    method: Method,
    to: Option<&Version>,
) -> Result<Vec<String>, String> {
    if tool == Tool::Notchling {
        return Err("Notchling updates itself".to_string());
    }
    if to.is_some() && !can_pin(tool, method) {
        return Err(format!(
            "{} can't install an exact version",
            match method {
                Method::Brew => "Homebrew".to_string(),
                _ => tool.title().to_string(),
            }
        ));
    }
    let target = to.map_or("latest".to_string(), |v| v.as_str().to_string());
    match (tool, method) {
        (Tool::Claude, Method::Native) => Ok(match to {
            Some(v) => args(&["claude", "install", v.as_str()]),
            None => args(&["claude", "update"]),
        }),
        (Tool::Claude | Tool::Codex, Method::Npm) => {
            let name = package(tool).ok_or("no package")?;
            Ok(args(&["npm", "install", "-g", &format!("{name}@{target}")]))
        }
        (Tool::Claude | Tool::Codex, Method::Brew) => {
            let name = cask(tool).ok_or("no cask")?;
            Ok(args(&["brew", "upgrade", "--cask", name]))
        }
        (Tool::Codex, Method::Standalone) => {
            let tail = match to {
                Some(v) => format!(" --release {}", v.as_str()),
                None => String::new(),
            };
            Ok(args(&[
                "sh",
                "-c",
                &format!(
                    "s=$(curl -fsSL {CODEX_INSTALL_URL}) && printf '%s\\n' \"$s\" | sh -s --{tail}"
                ),
            ]))
        }
        (Tool::Agy, Method::Standalone) => Ok(args(&["agy", "update"])),
        _ => Err(format!(
            "{} was installed a way Notchling doesn't update",
            tool.title()
        )),
    }
}

pub fn go_back_target(seen: &[Version], current: &Version, failed: &[Version]) -> Option<Version> {
    seen.iter()
        .filter(|v| *v < current && !failed.contains(v))
        .max()
        .cloned()
}
