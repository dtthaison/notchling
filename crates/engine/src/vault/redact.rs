//! Hides secret strings in a byte stream (Swift `Redactor`). Values shorter than 4 bytes aren't
//! hidden: they'd blank out ordinary output.
//!
//! Every byte that belongs to an occurrence of any value is hidden, overlapping occurrences
//! included: a run of overlapping occurrences becomes the masks of a chain that covers it (the
//! longest occurrence at the run's start, then the one reaching furthest among those it overlaps).
//! For values that don't overlap in the text this is the Swift app's output exactly.
//!
//! A tail that could be the start of a value waits for the next read, and so does an occurrence
//! that reaches into that tail (a later one could overlap it), so a value split across reads is
//! still caught. Only that much is held: the rest goes out at once. The output never depends on how
//! the input was cut into reads, and the raw bytes are kept until they go out, so a mask is never
//! matched again.

use std::collections::BTreeMap;

/// Values under this many bytes are left as they are.
pub const MIN_HIDDEN: usize = 4;

/// One stream's redactor; stdout and stderr each get their own.
pub struct Redactor {
    /// (value, mask), longest value first.
    pairs: Vec<(Vec<u8>, Vec<u8>)>,
    /// Raw bytes not yet written out.
    pending: Vec<u8>,
}

/// One occurrence: start, end, index into `pairs`.
#[derive(Clone, Copy)]
struct Hit {
    start: usize,
    end: usize,
    pair: usize,
}

impl Redactor {
    /// `hide` maps each value to its mask (`[secret:<name>]`).
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

    /// Takes the next read; returns what can go out now.
    pub fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        self.pending.extend_from_slice(data);
        let (out, cut) = self.process(false);
        self.pending.drain(..cut);
        out
    }

    /// The stream ended: everything still held, hidden as it stands.
    pub fn finish(&mut self) -> Vec<u8> {
        let (out, _) = self.process(true);
        self.pending.clear();
        out
    }

    /// The output for `pending[..cut]` and `cut`, which is everything at the end.
    fn process(&self, at_end: bool) -> (Vec<u8>, usize) {
        let buf = &self.pending;
        let runs = self.runs(buf);
        let mut cut = if at_end {
            buf.len()
        } else {
            self.held_from(buf)
        };
        // An occurrence reaching into the held tail waits with it: one that starts in the tail
        // could overlap it once the rest arrives.
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

    /// Every occurrence in `buf`, grouped into runs of overlapping ones, in order.
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
        // By start, the longest first at each start (pairs are longest first).
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

    /// The masks of a chain of occurrences that covers the run.
    fn mask_run(&self, run: &[Hit], out: &mut Vec<u8>) {
        let first = run[0];
        out.extend_from_slice(&self.pairs[first.pair].1);
        let mut reach = first.end;
        let end = end_of(run);
        while reach < end {
            // Some occurrence overlaps `reach` and goes past it, or the run would have ended here.
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

    /// Where the longest tail that could be the start of a value begins (`buf.len()` when none).
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
