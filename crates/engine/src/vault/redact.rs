use std::collections::BTreeMap;

pub use super::forms::hide_forms;

pub const MIN_HIDDEN: usize = 4;
const FORCE_BYTES: usize = 64 * 1024;

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

    pub fn lookback(&self) -> usize {
        self.pairs
            .first()
            .map_or(0, |p| p.0.len().saturating_sub(1))
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(data);
        let force = self.pending.len() >= FORCE_BYTES;
        let (out, cut) = self.process(&self.pending, 0, false, force);
        self.pending.drain(..cut);
        out
    }

    pub fn finish(&mut self) -> Vec<u8> {
        let (out, _) = self.process(&self.pending, 0, true, false);
        self.pending.clear();
        out
    }

    pub fn finish_closed(&mut self) -> Vec<u8> {
        let mut buf = std::mem::take(&mut self.pending);
        let out = self.closed_mask(&buf, 0);
        wipe(&mut buf);
        out
    }

    pub fn mask_window(&self, lead: &[u8], bytes: &[u8], closed: bool) -> (Vec<u8>, usize) {
        let mut buf = Vec::with_capacity(lead.len() + bytes.len());
        buf.extend_from_slice(lead);
        buf.extend_from_slice(bytes);
        let result = if closed {
            (self.closed_mask(&buf, lead.len()), 0)
        } else {
            let (out, cut) = self.process(&buf, lead.len(), false, false);
            (out, buf.len() - cut.max(lead.len()))
        };
        wipe(&mut buf);
        result
    }

    fn closed_mask(&self, buf: &[u8], lead: usize) -> Vec<u8> {
        if buf.len() <= lead {
            return Vec::new();
        }
        let at = self.held_from(buf);
        if at == buf.len() {
            return self.process(buf, lead, true, false).0;
        }
        let runs = self.runs(buf);
        let straddle = runs
            .iter()
            .find(|run| run[0].start < at && end_of(run) > at);
        let split = straddle.map_or(at, |run| run[0].start);
        let mut out = self.process(&buf[..split], lead.min(split), true, false).0;
        let mut covered = split;
        if let Some(run) = straddle {
            self.mask_run(run, lead, &mut out);
            covered = end_of(run);
        }
        if covered < buf.len() {
            let tail = &buf[at..];
            if let Some((_, mask)) = self
                .pairs
                .iter()
                .find(|(v, _)| v.len() > tail.len() && v.starts_with(tail))
            {
                out.extend_from_slice(mask);
            }
        }
        out
    }

    fn process(&self, buf: &[u8], lead: usize, at_end: bool, force: bool) -> (Vec<u8>, usize) {
        let runs = self.runs(buf);
        let mut cut = if at_end {
            buf.len()
        } else {
            self.held_from(buf)
        };
        let mut partial: Option<(usize, usize)> = None;
        if let Some(i) = runs
            .iter()
            .position(|r| r.first().is_some_and(|h| h.start < cut) && end_of(r) > cut)
        {
            let chain = self.chain(&runs[i]);
            let done = chain.iter().take_while(|h| h.end <= cut).count();
            if force && runs[i][0].start == 0 && done > 0 {
                cut = chain[done].start;
                partial = Some((i, done));
            } else {
                cut = runs[i][0].start;
            }
        }
        let mut out = Vec::with_capacity(cut.saturating_sub(lead));
        let mut at = lead;
        for (i, run) in runs.iter().enumerate() {
            let part = partial.filter(|p| p.0 == i).map(|p| p.1);
            if part.is_none() && end_of(run) > cut {
                continue;
            }
            if run[0].start > at {
                out.extend_from_slice(&buf[at..run[0].start]);
            }
            let chain = self.chain(run);
            let take = part.unwrap_or(chain.len());
            for hit in chain.iter().take(take) {
                if hit.end > lead {
                    out.extend_from_slice(&self.pairs[hit.pair].1);
                }
            }
            at = at.max(chain[take - 1].end);
        }
        if cut > at {
            out.extend_from_slice(&buf[at..cut]);
        }
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
        let mut reach = 0;
        for hit in hits {
            match runs.last_mut() {
                Some(run) if hit.start < reach => {
                    run.push(hit);
                    reach = reach.max(hit.end);
                }
                _ => {
                    reach = hit.end;
                    runs.push(vec![hit]);
                }
            }
        }
        runs
    }

    fn chain(&self, run: &[Hit]) -> Vec<Hit> {
        let first = run[0];
        let mut chain = vec![first];
        let mut reach = first.end;
        let end = end_of(run);
        let mut next = 1;
        while reach < end {
            let mut best: Option<Hit> = None;
            while next < run.len() && run[next].start < reach {
                let hit = run[next];
                let better =
                    best.is_none_or(|b| hit.end > b.end || (hit.end == b.end && hit.pair < b.pair));
                if hit.end > reach && better {
                    best = Some(hit);
                }
                next += 1;
            }
            let Some(found) = best else {
                break;
            };
            chain.push(found);
            reach = found.end;
        }
        chain
    }

    fn mask_run(&self, run: &[Hit], floor: usize, out: &mut Vec<u8>) {
        for hit in self.chain(run) {
            if hit.end > floor {
                out.extend_from_slice(&self.pairs[hit.pair].1);
            }
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
