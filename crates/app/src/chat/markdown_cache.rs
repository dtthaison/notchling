use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::chat::markdown::{blocks, image_links, spans, Block, Span};
use crate::chat::paths::{self, Disk, Probe};

const MAX_MESSAGES: usize = 4096;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    pub spans: Vec<Span>,
    pub images: Vec<String>,
}

impl Inline {
    pub fn text(&self) -> String {
        spans_text(&self.spans)
    }
}

pub fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub marker: String,
    pub depth: usize,
    pub inline: Inline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Part {
    Heading {
        level: usize,
        spans: Vec<Span>,
    },
    Paragraph(Inline),
    List(Vec<Item>),
    Quote(Inline),
    Code {
        language: String,
        text: String,
    },
    Table {
        header: Vec<Vec<Span>>,
        rows: Vec<Vec<Vec<Span>>>,
    },
    Rule,
}

pub type Message = Rc<Vec<Part>>;

pub struct Probes<P: Probe> {
    inner: P,
    seen: RefCell<HashMap<PathBuf, (bool, bool)>>,
}

impl<P: Probe> Probes<P> {
    fn new(inner: P) -> Probes<P> {
        Probes {
            inner,
            seen: RefCell::default(),
        }
    }

    fn look(&self, path: &Path) -> (bool, bool) {
        if let Some(known) = self.seen.borrow().get(path) {
            return *known;
        }
        let exists = self.inner.exists(path);
        let found = (exists, exists && self.inner.is_dir(path));
        self.seen.borrow_mut().insert(path.to_path_buf(), found);
        found
    }

    fn forget(&self) {
        self.seen.borrow_mut().clear();
    }
}

impl<P: Probe> Probe for Probes<P> {
    fn exists(&self, path: &Path) -> bool {
        self.look(path).0
    }
    fn is_dir(&self, path: &Path) -> bool {
        self.look(path).1
    }
    fn home(&self) -> Option<&Path> {
        self.inner.home()
    }
}

struct Entry {
    text: String,
    folder: String,
    parsed: Vec<Part>,
    linked: Option<(u64, Message)>,
}

pub struct Cache<P: Probe = Disk> {
    probes: Probes<P>,
    load: u64,
    messages: HashMap<String, Entry>,
}

impl Default for Cache<Disk> {
    fn default() -> Self {
        Cache::with_probe(Disk::new())
    }
}

impl<P: Probe> Cache<P> {
    pub fn with_probe(probe: P) -> Cache<P> {
        Cache {
            probes: Probes::new(probe),
            load: 0,
            messages: HashMap::new(),
        }
    }

    pub fn message(&mut self, key: &str, text: &str, folder: &str) -> Message {
        let fresh = self
            .messages
            .get(key)
            .is_some_and(|e| e.text == text && e.folder == folder);
        if !fresh {
            if self.messages.len() >= MAX_MESSAGES {
                self.messages.clear();
            }
            self.messages.insert(
                key.to_owned(),
                Entry {
                    text: text.to_owned(),
                    folder: folder.to_owned(),
                    parsed: parse(text),
                    linked: None,
                },
            );
        }
        let (load, probes) = (self.load, &self.probes);
        let Some(entry) = self.messages.get_mut(key) else {
            return Rc::new(Vec::new());
        };
        match &entry.linked {
            Some((at, message)) if *at == load => message.clone(),
            _ => {
                let message = Rc::new(link(&entry.parsed, text, folder, probes));
                entry.linked = Some((load, message.clone()));
                message
            }
        }
    }

    pub fn is_file(&self, path: &Path) -> bool {
        let (exists, dir) = self.probes.look(path);
        exists && !dir
    }

    pub fn reloaded(&mut self) {
        self.probes.forget();
        self.load += 1;
    }

    pub fn switched(&mut self) {
        self.reloaded();
        self.messages.clear();
    }
}

fn inline(text: &str) -> Inline {
    Inline {
        spans: spans(text),
        images: image_links(text),
    }
}

fn parse(text: &str) -> Vec<Part> {
    blocks(text)
        .into_iter()
        .map(|block| match block {
            Block::Heading { level, text } => Part::Heading {
                level,
                spans: spans(&text),
            },
            Block::Paragraph(text) => Part::Paragraph(inline(&text)),
            Block::List(items) => Part::List(
                items
                    .into_iter()
                    .map(|item| Item {
                        marker: item.marker,
                        depth: item.depth,
                        inline: inline(&item.text),
                    })
                    .collect(),
            ),
            Block::Quote(text) => Part::Quote(inline(&text)),
            Block::Code { language, text } => Part::Code { language, text },
            Block::Table { header, rows } => Part::Table {
                header: header.iter().map(|c| spans(c)).collect(),
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|c| spans(c)).collect())
                    .collect(),
            },
            Block::Rule => Part::Rule,
        })
        .collect()
}

fn link(parsed: &[Part], text: &str, folder: &str, probe: &dyn Probe) -> Vec<Part> {
    let bases = paths::bases(text, folder, probe);
    let spans = |spans: &[Span]| link_spans(spans, &bases, probe);
    let inline = |i: &Inline| Inline {
        spans: spans(&i.spans),
        images: i
            .images
            .iter()
            .filter(|p| {
                let p = Path::new(p);
                probe.exists(p) && !probe.is_dir(p)
            })
            .cloned()
            .collect(),
    };
    parsed
        .iter()
        .map(|part| match part {
            Part::Heading { level, spans: s } => Part::Heading {
                level: *level,
                spans: spans(s),
            },
            Part::Paragraph(i) => Part::Paragraph(inline(i)),
            Part::List(items) => Part::List(
                items
                    .iter()
                    .map(|item| Item {
                        marker: item.marker.clone(),
                        depth: item.depth,
                        inline: inline(&item.inline),
                    })
                    .collect(),
            ),
            Part::Quote(i) => Part::Quote(inline(i)),
            Part::Table { header, rows } => Part::Table {
                header: header.iter().map(|c| spans(c)).collect(),
                rows: rows
                    .iter()
                    .map(|row| row.iter().map(|c| spans(c)).collect())
                    .collect(),
            },
            other => other.clone(),
        })
        .collect()
}

fn link_spans(spans: &[Span], bases: &[PathBuf], probe: &dyn Probe) -> Vec<Span> {
    if bases.is_empty()
        && !spans
            .iter()
            .any(|s| s.text.contains('/') || s.link.is_some())
    {
        return spans.to_vec();
    }
    paths::link_spans(spans.to_vec(), bases, probe)
}

#[cfg(test)]
#[path = "markdown_cache_tests.rs"]
mod tests;
