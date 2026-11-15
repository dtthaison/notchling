use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::vault::{hide_forms, wipe, wipe_forms, Redactor};
use std::collections::BTreeMap;
use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};

struct Masked {
    generation: u64,
    values: Vec<(String, GrantValue)>,
    template: Redactor,
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub fn mask_for(name: &str) -> String {
    format!("[secret {name}]")
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

    pub fn add(&self, job: &str, values: Vec<(String, GrantValue)>) {
        if values.is_empty() {
            return;
        }
        let mut table = lock(&self.table);
        let mut all = table
            .jobs
            .remove(job)
            .map(|old| old.values)
            .unwrap_or_default();
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
                template,
            },
        );
    }

    pub fn drop_job(&self, job: &str) {
        lock(&self.table).jobs.remove(job);
    }

    pub fn for_job(&self, job: &str) -> Option<Redactor> {
        self.fresh(job).map(|(_, redactor)| redactor)
    }

    pub fn generation(&self, job: &str) -> Option<u64> {
        lock(&self.table).jobs.get(job).map(|m| m.generation)
    }

    fn fresh(&self, job: &str) -> Option<(u64, Redactor)> {
        lock(&self.table)
            .jobs
            .get(job)
            .map(|m| (m.generation, m.template.clone()))
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
    bytes: &[u8],
    at_end: bool,
) -> (Vec<u8>, u64) {
    let length = bytes.len() as u64;
    let Some(mut redactor) = masks.for_job(job) else {
        return (bytes.to_vec(), offset + length);
    };
    let mut out = redactor.feed(bytes);
    if at_end {
        out.extend(redactor.finish_closed());
        return (out, offset + length);
    }
    let held = redactor.held().len() as u64;
    (out, offset + length - held)
}

#[derive(Default)]
struct Stream {
    generation: Option<u64>,
    redactor: Option<Redactor>,
}

impl Stream {
    fn feed(&mut self, masks: &TailMasks, job: &str, bytes: &[u8]) -> Vec<u8> {
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
                return out;
            }
        }
        self.feed_through(bytes)
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
        let out = self.stream.feed(&self.masks, &self.job, buf);
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
