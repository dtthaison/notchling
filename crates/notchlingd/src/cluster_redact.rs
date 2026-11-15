use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::vault::{hide_forms, wipe, wipe_forms, Redactor};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};

const MAX_VALUE_BYTES: usize = 32 * 1024;

struct Masked {
    generation: u64,
    values: Vec<(String, GrantValue)>,
    template: Option<Redactor>,
    ended: bool,
    lost: bool,
}

#[derive(Default)]
struct Table {
    next: u64,
    jobs: BTreeMap<String, Masked>,
}

#[derive(Default)]
pub struct TailMasks {
    table: Mutex<Table>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobMasks {
    Never,
    Gone,
    Masked,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn mask_for(name: &str) -> String {
    format!("[secret {name}]")
}

pub fn gone_message(job: &str) -> String {
    format!("the masks for job {job} are gone; its output isn't served")
}

fn build(values: &[(String, GrantValue)]) -> Redactor {
    let mut forms: BTreeMap<String, String> = BTreeMap::new();
    for (name, value) in values {
        forms.extend(hide_forms(value.expose(), &mask_for(name)));
    }
    let redactor = Redactor::new(&forms);
    wipe_forms(forms);
    redactor
}

impl TailMasks {
    pub fn new() -> TailMasks {
        TailMasks::default()
    }

    pub fn add(&self, job: &str, values: Vec<(String, GrantValue)>) -> Result<(), String> {
        TailMasks::check(&values)?;
        if values.is_empty() {
            return Ok(());
        }
        let mut table = lock(&self.table);
        let old = table.jobs.remove(job);
        let ended = old.as_ref().is_some_and(|m| m.ended);
        let lost = old.as_ref().is_some_and(|m| m.lost);
        let mut all = old.map(|m| m.values).unwrap_or_default();
        for (name, value) in values {
            let known = all
                .iter()
                .any(|(n, v)| n == &name && v.expose() == value.expose());
            if !known {
                all.push((name, value));
            }
        }
        let template = build(&all);
        table.next += 1;
        let generation = table.next;
        table.jobs.insert(
            job.to_string(),
            Masked {
                generation,
                values: all,
                template: Some(template),
                ended,
                lost,
            },
        );
        Ok(())
    }

    pub fn check(values: &[(String, GrantValue)]) -> Result<(), String> {
        match values
            .iter()
            .find(|(_, value)| value.expose().len() > MAX_VALUE_BYTES)
        {
            Some((name, _)) => Err(format!(
                "the value of {name} is larger than {MAX_VALUE_BYTES} bytes and can't be masked"
            )),
            None => Ok(()),
        }
    }

    pub fn lost(&self, job: &str) -> bool {
        lock(&self.table)
            .jobs
            .get(job)
            .is_some_and(|m| m.lost || m.template.is_none())
    }

    pub fn any_lost(&self) -> bool {
        lock(&self.table)
            .jobs
            .values()
            .any(|m| m.lost || m.template.is_none())
    }

    pub fn union(&self) -> Option<Redactor> {
        let table = lock(&self.table);
        let all: Vec<(String, GrantValue)> = table
            .jobs
            .values()
            .flat_map(|m| m.values.iter().cloned())
            .collect();
        (!all.is_empty()).then(|| build(&all))
    }

    pub fn mark_granted(&self, job: &str) {
        lock(&self.table)
            .jobs
            .entry(job.to_string())
            .or_insert(Masked {
                generation: 0,
                values: Vec::new(),
                template: None,
                ended: false,
                lost: true,
            });
    }

    pub fn end_job(&self, job: &str) {
        if let Some(masked) = lock(&self.table).jobs.get_mut(job) {
            masked.ended = true;
        }
    }

    pub fn forget_job(&self, job: &str) {
        lock(&self.table).jobs.remove(job);
    }

    pub fn is_ended(&self, job: &str) -> bool {
        lock(&self.table).jobs.get(job).is_some_and(|m| m.ended)
    }

    pub fn state(&self, job: &str) -> JobMasks {
        match lock(&self.table).jobs.get(job) {
            None => JobMasks::Never,
            Some(m) if m.template.is_none() => JobMasks::Gone,
            Some(_) => JobMasks::Masked,
        }
    }

    pub fn for_job(&self, job: &str) -> Option<Redactor> {
        self.fresh(job).map(|(_, redactor)| redactor)
    }

    pub fn generation(&self, job: &str) -> Option<u64> {
        lock(&self.table)
            .jobs
            .get(job)
            .filter(|m| m.template.is_some())
            .map(|m| m.generation)
    }

    pub fn lookback(&self, job: &str) -> usize {
        lock(&self.table)
            .jobs
            .get(job)
            .and_then(|m| m.template.as_ref())
            .map_or(0, |t| t.lookback())
    }

    fn fresh(&self, job: &str) -> Option<(u64, Redactor)> {
        let table = lock(&self.table);
        let masked = table.jobs.get(job)?;
        masked
            .template
            .as_ref()
            .map(|t| (masked.generation, t.clone()))
    }
}

impl fmt::Debug for TailMasks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = lock(&self.table);
        let mut list = f.debug_map();
        for (job, masked) in &table.jobs {
            list.entry(job, &format_args!("{} values", masked.values.len()));
        }
        list.finish()
    }
}

pub fn mask_tail(
    masks: &TailMasks,
    job: &str,
    offset: u64,
    lead: &[u8],
    bytes: &[u8],
    at_end: bool,
) -> Result<(Vec<u8>, u64), String> {
    let length = bytes.len() as u64;
    if masks.lost(job) {
        return Err(gone_message(job));
    }
    match masks.state(job) {
        JobMasks::Never => return Ok((bytes.to_vec(), offset + length)),
        JobMasks::Gone => return Err(gone_message(job)),
        JobMasks::Masked => {}
    }
    let redactor = masks.for_job(job).ok_or_else(|| gone_message(job))?;
    mask_with(redactor, offset, lead, bytes, at_end)
}

pub fn mask_with(
    redactor: Redactor,
    offset: u64,
    lead: &[u8],
    bytes: &[u8],
    at_end: bool,
) -> Result<(Vec<u8>, u64), String> {
    let length = bytes.len() as u64;
    let lookback = redactor.lookback();
    let lead = &lead[lead.len().saturating_sub(lookback)..];
    let (out, held) = redactor.mask_window(lead, bytes, at_end);
    if !at_end && held > lookback {
        let (out, _) = redactor.mask_window(lead, bytes, true);
        return Ok((out, offset + length));
    }
    Ok((out, offset + length - held as u64))
}

#[derive(Default)]
struct Stream {
    generation: Option<u64>,
    redactor: Option<Redactor>,
}

impl Stream {
    fn feed(&mut self, masks: &TailMasks, job: &str, bytes: &[u8]) -> Result<Vec<u8>, String> {
        if masks.state(job) == JobMasks::Gone && self.redactor.is_none() {
            return Err(gone_message(job));
        }
        let current = masks.generation(job);
        if current.is_some() && (self.redactor.is_none() || current != self.generation) {
            if let Some((generation, fresh)) = masks.fresh(job) {
                let mut input = self
                    .redactor
                    .as_ref()
                    .map(|old| old.held().to_vec())
                    .unwrap_or_default();
                input.extend_from_slice(bytes);
                self.generation = Some(generation);
                self.redactor = Some(fresh);
                let out = self.feed_through(&input);
                wipe(&mut input);
                return Ok(out);
            }
        }
        Ok(self.feed_through(bytes))
    }

    fn feed_through(&mut self, bytes: &[u8]) -> Vec<u8> {
        match self.redactor.as_mut() {
            Some(redactor) => redactor.feed(bytes),
            None => bytes.to_vec(),
        }
    }

    fn finish(&mut self) -> Vec<u8> {
        self.redactor
            .take()
            .map(|mut redactor| redactor.finish_closed())
            .unwrap_or_default()
    }
}

pub struct MaskedLog<W: Write> {
    inner: W,
    masks: Arc<TailMasks>,
    job: String,
    stream: Stream,
    closed: bool,
}

impl<W: Write> MaskedLog<W> {
    pub fn new(inner: W, masks: Arc<TailMasks>, job: &str) -> MaskedLog<W> {
        MaskedLog {
            inner,
            masks,
            job: job.to_string(),
            stream: Stream::default(),
            closed: false,
        }
    }

    pub fn close(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        let tail = self.stream.finish();
        self.inner.write_all(&tail)?;
        self.inner.flush()
    }

    pub fn get_ref(&self) -> &W {
        &self.inner
    }
}

impl<W: Write> Write for MaskedLog<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.closed {
            return Err(io::Error::new(io::ErrorKind::BrokenPipe, "log is closed"));
        }
        let out = self
            .stream
            .feed(&self.masks, &self.job, buf)
            .map_err(|message| io::Error::new(io::ErrorKind::PermissionDenied, message))?;
        self.inner.write_all(&out)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<W: Write> Drop for MaskedLog<W> {
    fn drop(&mut self) {
        let _ = self.close();
    }
}
