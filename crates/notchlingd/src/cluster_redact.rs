use notchling_engine::cluster::grant_files::GrantValue;
use notchling_engine::vault::{hide_forms, wipe_forms, Redactor};
use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::io::{self, Write};
use std::sync::{Arc, Mutex, MutexGuard};

struct Masked {
    generation: u64,
    names: usize,
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

impl TailMasks {
    pub fn new() -> TailMasks {
        TailMasks::default()
    }

    pub fn set(&self, job: &str, values: Vec<(String, GrantValue)>) {
        if values.is_empty() {
            self.drop_job(job);
            return;
        }
        let names = values.len();
        let mut forms: BTreeMap<String, String> = BTreeMap::new();
        for (name, value) in &values {
            forms.extend(hide_forms(value.expose(), &mask_for(name)));
        }
        drop(values);
        let template = Redactor::new(&forms);
        wipe_forms(forms);
        let mut table = lock(&self.table);
        table.next += 1;
        let generation = table.next;
        table.jobs.insert(
            job.to_string(),
            Masked {
                generation,
                names,
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
            list.entry(job, &format_args!("{} values", masked.names));
        }
        list.finish()
    }
}

#[derive(Default)]
struct Stream {
    generation: Option<u64>,
    redactor: Option<Redactor>,
}

impl Stream {
    fn feed(&mut self, masks: &TailMasks, job: &str, bytes: &[u8]) -> Vec<u8> {
        let current = masks.generation(job);
        if current.is_some() && current != self.generation {
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
                wipe_vec(input);
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
            .map(|mut redactor| redactor.finish())
            .unwrap_or_default()
    }
}

fn wipe_vec(mut bytes: Vec<u8>) {
    for byte in bytes.iter_mut() {
        unsafe { std::ptr::write_volatile(byte, 0) };
    }
    std::sync::atomic::compiler_fence(std::sync::atomic::Ordering::SeqCst);
}

type TailKey = (String, String, String);

#[derive(Default)]
pub struct TailFilters {
    streams: Mutex<HashMap<TailKey, Arc<Mutex<Stream>>>>,
}

impl TailFilters {
    pub fn new() -> TailFilters {
        TailFilters::default()
    }

    pub fn filter(
        &self,
        masks: &TailMasks,
        follower: &str,
        file: &str,
        job: &str,
        bytes: &[u8],
    ) -> Vec<u8> {
        let key = (follower.to_string(), file.to_string(), job.to_string());
        let stream = {
            let mut streams = lock(&self.streams);
            match streams.get(&key) {
                Some(stream) => stream.clone(),
                None if masks.generation(job).is_none() => return bytes.to_vec(),
                None => streams.entry(key).or_default().clone(),
            }
        };
        let mut stream = lock(&stream);
        stream.feed(masks, job, bytes)
    }

    pub fn finish(&self, follower: &str, file: &str, job: &str) -> Vec<u8> {
        let key = (follower.to_string(), file.to_string(), job.to_string());
        let stream = lock(&self.streams).remove(&key);
        match stream {
            Some(stream) => lock(&stream).finish(),
            None => Vec::new(),
        }
    }

    pub fn drop_job(&self, job: &str) {
        lock(&self.streams).retain(|(_, _, owner), _| owner != job);
    }

    pub fn count(&self) -> usize {
        lock(&self.streams).len()
    }
}

impl fmt::Debug for TailFilters {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "TailFilters({} streams)", self.count())
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
