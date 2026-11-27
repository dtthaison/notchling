use std::path::{Path, PathBuf};

use super::channel::Opener;

pub const BASE: &str = "antigravity://open?prompt=";

const APP: &str = "Antigravity.app";

const HEX: &[u8; 16] = b"0123456789ABCDEF";

pub fn link(text: &str) -> String {
    format!("{BASE}{}", encode(text))
}

pub fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            _ => {
                out.push('%');
                out.push(char::from(HEX[usize::from(byte >> 4)]));
                out.push(char::from(HEX[usize::from(byte & 0x0f)]));
            }
        }
    }
    out
}

pub fn app_folders(home: Option<&Path>) -> Vec<PathBuf> {
    let mut folders = vec![PathBuf::from("/Applications")];
    if let Some(home) = home {
        folders.push(home.join("Applications"));
    }
    folders
}

pub fn installed_in(folders: &[PathBuf]) -> bool {
    folders.iter().any(|folder| folder.join(APP).is_dir())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotInstalled;

pub fn hand_over(opener: &mut dyn Opener, text: &str) -> Result<(), NotInstalled> {
    if !opener.installed() {
        return Err(NotInstalled);
    }
    opener.open(&link(text));
    Ok(())
}

#[cfg(test)]
#[path = "channel_draft_tests.rs"]
mod tests;
