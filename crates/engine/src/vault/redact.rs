use std::collections::BTreeMap;

pub const MIN_HIDDEN: usize = 4;
const CLOSED_PREFIX: usize = 8;

#[derive(Clone)]
pub struct Redactor {
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    pending: Vec<u8>,
}

#[derive(Clone, Copy)]
struct Hit {
    start: usize,
    end: usize,
    pair: usize,
}

impl Redactor {
    pub fn new(hide: &BTreeMap<String, String>) -> Redactor {
        let mut pairs: Vec<(Vec<u8>, Vec<u8>)> = hide
            .iter()
            .filter(|(value, _)| value.len() >= MIN_HIDDEN)
            .map(|(v, m)| (v.as_bytes().to_vec(), m.as_bytes().to_vec()))
            .collect();
        pairs.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then_with(|| a.0.cmp(&b.0)));
        Redactor {
            pairs,
            pending: Vec::new(),
        }
    }

    pub fn held(&self) -> &[u8] {
        &self.pending
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(data);
        let (out, cut) = self.process(false);
        self.pending.drain(..cut);
        out
    }

    pub fn finish(&mut self) -> Vec<u8> {
        let (out, _) = self.process(true);
        self.pending.clear();
        out
    }

    pub fn finish_closed(&mut self) -> Vec<u8> {
        let split = self.closed_split();
        let mut out;
        match split {
            Some(at) => {
                let mut tail = self.pending.split_off(at);
                out = self.process(true).0;
                match self.pairs.iter().find(|(v, _)| v.starts_with(&tail)) {
                    Some((_, mask)) => out.extend_from_slice(mask),
                    None => out.extend_from_slice(&tail),
                }
                wipe(&mut tail);
            }
            None => out = self.process(true).0,
        }
        self.pending.clear();
        out
    }

    fn closed_split(&self) -> Option<usize> {
        let buf = &self.pending;
        let last = buf.len().checked_sub(CLOSED_PREFIX)?;
        let at = (0..=last).find(|&start| {
            let tail = &buf[start..];
            self.pairs
                .iter()
                .any(|(value, _)| value.len() > tail.len() && value.starts_with(tail))
        })?;
        let straddles = self
            .runs(buf)
            .iter()
            .any(|run| run[0].start < at && end_of(run) > at);
        (!straddles).then_some(at)
    }

    fn process(&self, at_end: bool) -> (Vec<u8>, usize) {
        let buf = &self.pending;
        let runs = self.runs(buf);
        let mut cut = if at_end {
            buf.len()
        } else {
            self.held_from(buf)
        };
        if let Some(run) = runs
            .iter()
            .find(|r| r.first().is_some_and(|h| h.start < cut) && end_of(r) > cut)
        {
            cut = run[0].start;
        }
        let mut out = Vec::with_capacity(cut);
        let mut at = 0;
        for run in runs.iter().filter(|r| end_of(r) <= cut) {
            out.extend_from_slice(&buf[at..run[0].start]);
            self.mask_run(run, &mut out);
            at = end_of(run);
        }
        out.extend_from_slice(&buf[at..cut]);
        (out, cut)
    }

    fn runs(&self, buf: &[u8]) -> Vec<Vec<Hit>> {
        let mut hits: Vec<Hit> = Vec::new();
        for (pair, (value, _)) in self.pairs.iter().enumerate() {
            if value.len() > buf.len() {
                continue;
            }
            for start in 0..=buf.len() - value.len() {
                if buf[start..].starts_with(value) {
                    hits.push(Hit {
                        start,
                        end: start + value.len(),
                        pair,
                    });
                }
            }
        }
        hits.sort_by(|a, b| a.start.cmp(&b.start).then(a.pair.cmp(&b.pair)));
        let mut runs: Vec<Vec<Hit>> = Vec::new();
        for hit in hits {
            match runs.last_mut() {
                Some(run) if hit.start < end_of(run) => run.push(hit),
                _ => runs.push(vec![hit]),
            }
        }
        runs
    }

    fn mask_run(&self, run: &[Hit], out: &mut Vec<u8>) {
        let first = run[0];
        out.extend_from_slice(&self.pairs[first.pair].1);
        let mut reach = first.end;
        let end = end_of(run);
        while reach < end {
            let Some(next) = run
                .iter()
                .filter(|h| h.start < reach && h.end > reach)
                .max_by(|a, b| a.end.cmp(&b.end).then(b.pair.cmp(&a.pair)))
            else {
                break;
            };
            out.extend_from_slice(&self.pairs[next.pair].1);
            reach = next.end;
        }
    }

    fn held_from(&self, buf: &[u8]) -> usize {
        let longest = self.pairs.first().map_or(0, |p| p.0.len());
        let earliest = buf.len().saturating_sub(longest.saturating_sub(1));
        (earliest..buf.len())
            .find(|&start| {
                let tail = &buf[start..];
                self.pairs
                    .iter()
                    .any(|(value, _)| value.len() > tail.len() && value.starts_with(tail))
            })
            .unwrap_or(buf.len())
    }
}

fn end_of(run: &[Hit]) -> usize {
    run.iter().map(|h| h.end).max().unwrap_or(0)
}

impl Drop for Redactor {
    fn drop(&mut self) {
        for (value, mask) in self.pairs.iter_mut() {
            wipe(value);
            wipe(mask);
        }
        let capacity = self.pending.capacity();
        self.pending.resize(capacity, 0);
        wipe(&mut self.pending);
    }
}

pub fn wipe(bytes: &mut [u8]) {
    for byte in bytes.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

pub fn wipe_string(text: String) {
    wipe(&mut text.into_bytes());
}

pub fn wipe_forms(forms: BTreeMap<String, String>) {
    for (key, mask) in forms {
        wipe_string(key);
        wipe_string(mask);
    }
}

const MIN_LINE: usize = 8;

fn json_inner(text: &str) -> String {
    let quoted = serde_json::to_string(text).unwrap_or_default();
    let inner = quoted
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .unwrap_or_default()
        .to_string();
    wipe_string(quoted);
    inner
}

fn ascii_escaped(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch.is_ascii() {
            out.push(ch);
        } else {
            let mut units = [0u16; 2];
            for unit in ch.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

fn html_escaped(json: &str) -> String {
    json.replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

fn forms_of(text: &str, into: &mut Vec<String>) {
    into.push(text.to_string());
    let json = json_inner(text);
    let mut levels = vec![json.clone()];
    if !text.is_ascii() {
        levels.push(ascii_escaped(&json));
    }
    if text.contains(['<', '>', '&', '\u{2028}', '\u{2029}']) {
        levels.push(html_escaped(&json));
    }
    let slashed: Vec<String> = levels
        .iter()
        .filter(|level| level.contains('/'))
        .map(|level| level.replace('/', "\\/"))
        .collect();
    levels.extend(slashed);
    for level in levels {
        let doubled = json_inner(&level);
        if doubled != level {
            into.push(doubled);
        }
        into.push(level);
    }
    wipe_string(json);
}

pub fn hide_forms(value: &str, mask: &str) -> BTreeMap<String, String> {
    let mut forms = Vec::new();
    forms_of(value, &mut forms);
    if value.contains(['\n', '\r']) {
        for line in value.split(['\n', '\r']) {
            if line.chars().count() >= MIN_LINE {
                forms_of(line, &mut forms);
            }
        }
    }
    let mut hidden = BTreeMap::new();
    for form in forms {
        if form.is_empty() || hidden.contains_key(&form) {
            wipe_string(form);
        } else {
            hidden.insert(form, mask.to_string());
        }
    }
    hidden
}
