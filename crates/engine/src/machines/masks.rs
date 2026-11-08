//! What a machine's output and results must not show (Swift `SetupMasks`, `SetupFixLog`): the vault
//! values its steps use, as `[secret:<name>]`, and the admin password of the Fix in flight, as
//! `[admin password]`. Built per call; the engine never keeps one.

use super::process::CUT_NOTICE;
use super::runner::Channel;
use crate::vault::redact::MIN_HIDDEN;
use crate::vault::Redactor;
use std::collections::BTreeMap;
use std::sync::Mutex;

pub const ADMIN_MASK: &str = "[admin password]";

#[derive(Clone, Default)]
pub struct Masks {
    /// Secret value → what shows instead.
    vault: BTreeMap<String, String>,
    admin_password: Option<String>,
}

impl std::fmt::Debug for Masks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Masks({} values)", self.vault.len())
    }
}

impl Masks {
    pub fn new() -> Masks {
        Masks::default()
    }

    /// Hides `value` as `[secret:<name>]`.
    pub fn hide(&mut self, name: &str, value: &str) {
        if !value.is_empty() {
            self.vault.insert(value.into(), format!("[secret:{name}]"));
        }
    }

    pub fn with_admin(mut self, password: Option<&str>) -> Masks {
        self.admin_password = password.map(str::to_string);
        self
    }

    pub fn apply(&self, text: &str) -> String {
        if text.is_empty() {
            return String::new();
        }
        let mut text = text.to_string();
        if !self.vault.is_empty() {
            let mut redactor = Redactor::new(&self.vault);
            let mut data = redactor.feed(text.as_bytes());
            data.extend(redactor.finish());
            text = String::from_utf8_lossy(&data).into_owned();
        }
        // from 4 bytes up like the redactor: a shorter one would turn ordinary words into the mask
        if let Some(pw) = &self.admin_password {
            if pw.len() >= MIN_HIDDEN {
                text = text.replace(pw.as_str(), ADMIN_MASK);
            }
        }
        text
    }

    /// The longest string that is masked: a line may be cut that far from its end without
    /// splitting a secret.
    pub fn longest(&self) -> usize {
        let vault = self.vault.keys().map(String::len).max().unwrap_or(0);
        vault.max(self.admin_password.as_ref().map_or(0, String::len))
    }

    /// Whether `text` holds any value this hides (4 bytes or more), or the admin password.
    pub fn leaks_into(&self, text: &str) -> bool {
        self.vault
            .keys()
            .chain(self.admin_password.iter())
            .any(|v| v.len() >= MIN_HIDDEN && text.contains(v.as_str()))
    }

    /// Output a cut has made: masked first, then the line the cut ran through is dropped (it can
    /// hold the start or the end of a value that no longer matches). A head cut (the runner's 2 MiB
    /// limit) loses the first line after the notice; a tail cut (`head -c` on the machine) the last.
    pub fn apply_to_cut(&self, text: &str, head_cut: bool, tail_cut: bool) -> String {
        if !head_cut && !tail_cut {
            return text.to_string();
        }
        let body = if head_cut {
            text.strip_prefix(CUT_NOTICE).unwrap_or(text)
        } else {
            text
        };
        let mut body = self.apply(body);
        if head_cut {
            body = match body.find('\n') {
                Some(nl) => body[nl + 1..].to_string(),
                None => String::new(),
            };
        }
        if tail_cut {
            body = match body.rfind('\n') {
                Some(nl) => body[..nl].to_string(),
                None => String::new(),
            };
        }
        if head_cut {
            format!("{CUT_NOTICE}{body}")
        } else {
            body
        }
    }
}

/// Collects one Fix's output into masked lines. Chunks of each pipe build that pipe's own line, so
/// a value cut between two chunks of one pipe is whole again in its line even when the other pipe
/// wrote in between. After [`FixLog::end`] nothing more gets in.
pub struct FixLog {
    masks: Masks,
    state: Mutex<LogState>,
}

#[derive(Default)]
struct LogState {
    partial: BTreeMap<Channel, String>,
    lines: Vec<String>,
    ended: bool,
}

/// A line this long without a newline is let out in pieces.
const LONG_LINE: usize = 4096;

impl FixLog {
    pub fn new(masks: Masks) -> FixLog {
        FixLog {
            masks,
            state: Mutex::new(LogState::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LogState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn add(&self, channel: Channel, chunk: &str) {
        let mut state = self.lock();
        if state.ended || chunk.is_empty() {
            return;
        }
        let joined = format!(
            "{}{chunk}",
            state.partial.get(&channel).cloned().unwrap_or_default()
        );
        let mut parts: Vec<&str> = joined.split('\n').collect();
        let mut rest = parts.pop().unwrap_or("").to_string();
        for line in parts {
            let shown = self.shown(line);
            state.lines.push(shown);
        }
        if rest.len() > LONG_LINE {
            // mask first, cut afterwards; the end stays back, so a secret still arriving is whole
            // in the next piece
            let masked = self.masks.apply(&rest);
            let keep = self.masks.longest().max(1).min(masked.len());
            let mut cut = masked.len() - keep;
            while !masked.is_char_boundary(cut) {
                cut -= 1;
            }
            state.lines.push(masked[..cut].to_string());
            rest = masked[cut..].to_string();
        }
        state.partial.insert(channel, rest);
    }

    fn shown(&self, line: &str) -> String {
        self.masks.apply(line.strip_suffix('\r').unwrap_or(line))
    }

    /// Lets out what each pipe still holds and closes the log.
    pub fn end(&self) {
        let mut state = self.lock();
        if state.ended {
            return;
        }
        state.ended = true;
        let partial = std::mem::take(&mut state.partial);
        for (_, rest) in partial {
            if !rest.is_empty() {
                let shown = self.shown(&rest);
                state.lines.push(shown);
            }
        }
    }

    /// The masked lines so far.
    pub fn lines(&self) -> Vec<String> {
        self.lock().lines.clone()
    }

    /// Takes the masked lines so far, leaving none.
    pub fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().lines)
    }
}
