use std::collections::BTreeMap;

pub const MIN_HIDDEN: usize = 4;

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
