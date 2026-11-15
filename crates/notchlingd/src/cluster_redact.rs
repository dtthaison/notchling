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
    keys: Vec<String>,
    released: bool,
}

struct Standing {
    generation: u64,
    values: Vec<(String, GrantValue)>,
    template: Redactor,
}

#[derive(Default)]
struct Table {
    next: u64,
    jobs: BTreeMap<String, Masked>,
    standing: Option<Standing>,
}

pub const MAX_STANDING: usize = 16;

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

fn combined(job: u64, standing: u64) -> u64 {
    job.wrapping_mul(1_000_003).wrapping_add(standing)
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
        let keys = old.as_ref().map(|m| m.keys.clone()).unwrap_or_default();
        let released = old.as_ref().is_some_and(|m| m.released);
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
                keys,
                released,
            },
        );
        Ok(())
    }

    pub fn add_standing(&self, values: Vec<(String, GrantValue)>) -> Result<(), String> {
        TailMasks::check(&values)?;
        let mut table = lock(&self.table);
        let held = table.standing.as_ref().map_or(0, |s| s.values.len());
        let mut all = table
            .standing
            .as_ref()
            .map(|s| s.values.clone())
            .unwrap_or_default();
        for (name, value) in values {
            let known = all
                .iter()
                .any(|(n, v)| n == &name && v.expose() == value.expose());
            if !known {
                all.push((name, value));
            }
        }
        if all.len() == held {
            return Ok(());
        }
        if all.len() > MAX_STANDING {
            return Err("too many different standing values since the last restart".to_string());
        }
        let template = build(&all);
        table.next += 1;
        let generation = table.next;
        table.standing = Some(Standing {
            generation,
            values: all,
            template,
        });
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
            .chain(table.standing.iter().flat_map(|s| s.values.iter().cloned()))
            .collect();
        (!all.is_empty()).then(|| build(&all))
    }

    pub fn mark_granted(&self, job: &str) {
        self.mark_granted_as(job, &[]);
    }

    pub fn mark_granted_as(&self, job: &str, keys: &[String]) {
        let mut table = lock(&self.table);
        let masked = table.jobs.entry(job.to_string()).or_insert(Masked {
            generation: 0,
            values: Vec::new(),
            template: None,
            ended: false,
            lost: true,
            keys: Vec::new(),
            released: false,
        });
        for key in keys {
            if !masked.keys.contains(key) {
                masked.keys.push(key.clone());
            }
        }
    }

    pub fn job_for_key(&self, key: &str) -> Option<String> {
        lock(&self.table)
            .jobs
            .iter()
            .find(|(_, m)| m.keys.iter().any(|k| k == key))
            .map(|(job, _)| job.clone())
    }

    pub fn unidentified_lost(&self) -> bool {
        lock(&self.table)
            .jobs
            .values()
            .any(|m| (m.lost || m.template.is_none()) && m.keys.is_empty() && !m.released)
    }

    pub fn end_job(&self, job: &str) {
        if let Some(masked) = lock(&self.table).jobs.get_mut(job) {
            masked.ended = true;
        }
    }

    pub fn release_blanket(&self, job: &str) {
        if let Some(masked) = lock(&self.table).jobs.get_mut(job) {
            masked.released = true;
        }
    }

    pub fn forget_job(&self, job: &str) {
        lock(&self.table).jobs.remove(job);
    }

    pub fn is_ended(&self, job: &str) -> bool {
        lock(&self.table).jobs.get(job).is_some_and(|m| m.ended)
    }

    pub fn state(&self, job: &str) -> JobMasks {
        let table = lock(&self.table);
        match table.jobs.get(job) {
            None if table.standing.is_some() => JobMasks::Masked,
            None => JobMasks::Never,
            Some(m) if m.template.is_none() => JobMasks::Gone,
            Some(_) => JobMasks::Masked,
        }
    }

    pub fn for_job(&self, job: &str) -> Option<Redactor> {
        self.fresh(job).map(|(_, redactor)| redactor)
    }

    pub fn generation(&self, job: &str) -> Option<u64> {
        let table = lock(&self.table);
        match (table.jobs.get(job), &table.standing) {
            (Some(m), standing) if m.template.is_some() => Some(
                standing
                    .as_ref()
                    .map_or(m.generation, |s| combined(m.generation, s.generation)),
            ),
            (Some(_), _) => None,
            (None, standing) => standing.as_ref().map(|s| s.generation),
        }
    }

    pub fn lookback(&self, job: &str) -> usize {
        self.fresh(job)
            .map_or(0, |(_, redactor)| redactor.lookback())
    }

    fn fresh(&self, job: &str) -> Option<(u64, Redactor)> {
        let table = lock(&self.table);
        match (table.jobs.get(job), &table.standing) {
            (Some(masked), None) => masked
                .template
                .as_ref()
                .map(|t| (masked.generation, t.clone())),
            (Some(masked), Some(standing)) => {
                masked.template.as_ref()?;
                let all: Vec<(String, GrantValue)> = masked
                    .values
                    .iter()
                    .chain(&standing.values)
                    .cloned()
                    .collect();
                Some((
                    combined(masked.generation, standing.generation),
                    build(&all),
                ))
            }
            (None, Some(standing)) => Some((standing.generation, standing.template.clone())),
            (None, None) => None,
        }
    }
}

impl fmt::Debug for TailMasks {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let table = lock(&self.table);
        let mut list = f.debug_map();
        for (job, masked) in &table.jobs {
            list.entry(job, &format_args!("{} values", masked.values.len()));
        }
        if let Some(standing) = &table.standing {
            list.entry(
                &"standing",
                &format_args!("{} values", standing.values.len()),
            );
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
