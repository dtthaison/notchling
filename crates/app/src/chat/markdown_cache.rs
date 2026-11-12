//! A message's Markdown ready to draw, kept between draws: its blocks and spans parsed once per
//! text, and the paths in it looked up on disk once per conversation load. Before, every frame
//! parsed every message again and asked the disk about each path in it (a spinner asked for 60 or
//! 120 frames a second). `ChatView` keeps one: [`Cache::reloaded`] when a load lands (the disk is
//! asked again, the parses stay), [`Cache::switched`] when another session is picked (all of it
//! goes). Pure over a [`Probe`], so it is tested on every system.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::chat::markdown::{blocks, image_links, spans, Block, Span};
use crate::chat::paths::{self, Disk, Probe};

/// Messages kept at most; past this the cache starts over (a session has a few hundred).
const MAX_MESSAGES: usize = 4096;

/// A paragraph's text as spans, with the image files it links that exist (drawn under it).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inline {
    pub spans: Vec<Span>,
    pub images: Vec<String>,
}

impl Inline {
    /// The words, the spans' text joined: what the paragraph's text element holds and selects.
    pub fn text(&self) -> String {
        spans_text(&self.spans)
    }
}

/// The spans' text joined.
pub fn spans_text(spans: &[Span]) -> String {
    spans.iter().map(|span| span.text.as_str()).collect()
}

/// A list item, ready to draw.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Item {
    pub marker: String,
    pub depth: usize,
    pub inline: Inline,
}

/// A block of `chat::markdown`, its texts as spans with their paths linked.
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

/// A message, ready to draw.
pub type Message = Rc<Vec<Part>>;

/// The disk as the path rules see it: each path asked once, until [`Probes::forget`].
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

/// A message as parsed, before its paths are looked up.
struct Entry {
    text: String,
    folder: String,
    parsed: Vec<Part>,
    /// Linked against the disk as it was in load `.0`.
    linked: Option<(u64, Message)>,
}

pub struct Cache<P: Probe = Disk> {
    probes: Probes<P>,
    /// Counts the loads, so a message linked in an older one is linked again.
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

    /// The message under `key` (unique among those drawn), `text` read against `folder`.
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

    /// Whether `path` is a file, as this load's disk said.
    pub fn is_file(&self, path: &Path) -> bool {
        let (exists, dir) = self.probes.look(path);
        exists && !dir
    }

    /// A load landed: what the disk said is asked again; the parses stay.
    pub fn reloaded(&mut self) {
        self.probes.forget();
        self.load += 1;
    }

    /// Another session was picked: nothing of the last one is kept.
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

/// The message's blocks with their texts as spans; the images are every one linked, kept or
/// dropped by [`link`].
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

/// The parsed message with the paths that exist made links (`chat::paths`) and only the linked
/// images that are files.
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

/// `paths::link_spans`, skipped for a text with no slash and nothing to read it against.
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
