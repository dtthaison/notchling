use super::format::VecFormat;
use super::vectors::VectorDb;
use crate::search::embed::{dot_f32_i8, ModelId, Q8};
use std::cmp::{Ordering, Reverse};
use std::collections::{BinaryHeap, HashMap};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, PartialEq)]
pub struct SetDoc {
    pub corpus: String,
    pub key: String,
    pub at: f64,
    pub facets: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DocHit {
    pub corpus: String,
    pub key: String,
    pub chunk: [u8; 16],
    pub score: f32,
}

pub type Keep<'a> = dyn Fn(&str, &[(&str, &str)], f64) -> bool + 'a;

pub type KeepKeyed<'a> = dyn Fn(&str, &str, &[(&str, &str)], f64) -> bool + 'a;

struct Interned {
    corpus: u32,
    key: String,
    at: f64,
    facets: Vec<(u32, u32)>,
}

#[derive(Default)]
pub struct VectorSet {
    dims: usize,
    names: Vec<String>,
    docs: Vec<Interned>,
    row_doc: Vec<u32>,
    row_hash: Vec<[u8; 16]>,
    row_scale: Vec<f32>,
    data: Vec<i8>,
}

struct Scored {
    score: f32,
    row: u32,
}

impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Scored {}

impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then(other.row.cmp(&self.row))
    }
}

fn intern(names: &mut Vec<String>, index: &mut HashMap<String, u32>, text: &str) -> u32 {
    if let Some(&i) = index.get(text) {
        return i;
    }
    let i = names.len() as u32;
    names.push(text.to_string());
    index.insert(text.to_string(), i);
    i
}

impl VectorSet {
    pub fn from_parts(
        dims: usize,
        docs: Vec<SetDoc>,
        rows: Vec<(usize, [u8; 16], Q8)>,
    ) -> VectorSet {
        let mut names = Vec::new();
        let mut index = HashMap::new();
        let docs: Vec<Interned> = docs
            .into_iter()
            .map(|d| Interned {
                corpus: intern(&mut names, &mut index, &d.corpus),
                key: d.key,
                at: d.at,
                facets: d
                    .facets
                    .iter()
                    .map(|(n, v)| {
                        (
                            intern(&mut names, &mut index, n),
                            intern(&mut names, &mut index, v),
                        )
                    })
                    .collect(),
            })
            .collect();
        let mut set = VectorSet {
            dims,
            names,
            docs,
            ..VectorSet::default()
        };
        set.data.reserve(rows.len() * dims);
        for (doc, hash, q) in rows {
            if doc >= set.docs.len() || q.vec.len() != dims {
                continue;
            }
            set.row_doc.push(doc as u32);
            set.row_hash.push(hash);
            set.row_scale.push(q.scale);
            set.data.extend_from_slice(&q.vec);
        }
        set
    }

    pub fn load(db: &VectorDb, model: &ModelId) -> VectorSet {
        db.with(|conn| {
            let format = conn
                .query_row(
                    "SELECT v FROM meta WHERE k = ?1",
                    [format!("format:{}", model.0)],
                    |r| r.get::<_, String>(0),
                )
                .ok()
                .and_then(|t| VecFormat::from_tag(&t));
            let Some(format) = format else {
                return VectorSet::default();
            };
            let mut docs: Vec<SetDoc> = Vec::new();
            let mut slot: HashMap<i64, usize> = HashMap::new();
            if let Ok(mut query) = conn.prepare("SELECT id, corpus, key, at FROM docs ORDER BY id")
            {
                if let Ok(found) = query.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        SetDoc {
                            corpus: r.get(1)?,
                            key: r.get(2)?,
                            at: r.get(3)?,
                            facets: Vec::new(),
                        },
                    ))
                }) {
                    for (id, doc) in found.flatten() {
                        slot.insert(id, docs.len());
                        docs.push(doc);
                    }
                }
            }
            if let Ok(mut query) = conn.prepare("SELECT doc, name, value FROM facets") {
                if let Ok(found) = query.query_map([], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                }) {
                    for (id, name, value) in found.flatten() {
                        if let Some(&i) = slot.get(&id) {
                            docs[i].facets.push((name, value));
                        }
                    }
                }
            }
            let mut rows: Vec<(usize, [u8; 16], Q8)> = Vec::new();
            if let Ok(mut query) = conn.prepare(
                "SELECT c.doc, c.hash, v.vec FROM chunks c
                 JOIN vectors v ON v.hash = c.hash AND v.model = ?1 ORDER BY c.id",
            ) {
                if let Ok(found) = query.query_map([&model.0], |r| {
                    Ok((
                        r.get::<_, i64>(0)?,
                        r.get::<_, Vec<u8>>(1)?,
                        r.get::<_, Vec<u8>>(2)?,
                    ))
                }) {
                    for (id, hash, bytes) in found.flatten() {
                        let (Some(&i), Ok(hash), Some(q)) = (
                            slot.get(&id),
                            <[u8; 16]>::try_from(hash),
                            format.decode(&bytes),
                        ) else {
                            continue;
                        };
                        rows.push((i, hash, q));
                    }
                }
            }
            let dims = rows.first().map_or(0, |(_, _, q)| q.vec.len());
            VectorSet::from_parts(dims, docs, rows)
        })
    }

    pub fn len(&self) -> usize {
        self.row_doc.len()
    }

    pub fn is_empty(&self) -> bool {
        self.row_doc.is_empty()
    }

    pub fn mask(&self, keep: &Keep) -> Vec<bool> {
        self.mask_keyed(&|corpus, _, facets, at| keep(corpus, facets, at))
    }

    pub fn mask_keyed(&self, keep: &KeepKeyed) -> Vec<bool> {
        self.docs
            .iter()
            .map(|d| {
                let facets: Vec<(&str, &str)> = d
                    .facets
                    .iter()
                    .map(|&(n, v)| {
                        (
                            self.names[n as usize].as_str(),
                            self.names[v as usize].as_str(),
                        )
                    })
                    .collect();
                keep(&self.names[d.corpus as usize], &d.key, &facets, d.at)
            })
            .collect()
    }

    pub fn scan(
        &self,
        query: &[f32],
        top_chunks: usize,
        mask: Option<&[bool]>,
        floor: f32,
    ) -> Vec<DocHit> {
        if top_chunks == 0 || self.dims == 0 || query.len() != self.dims {
            return Vec::new();
        }
        let mut heap: BinaryHeap<Reverse<Scored>> = BinaryHeap::with_capacity(top_chunks + 1);
        for row in 0..self.row_doc.len() {
            if let Some(mask) = mask {
                if !mask
                    .get(self.row_doc[row] as usize)
                    .copied()
                    .unwrap_or(false)
                {
                    continue;
                }
            }
            let at = row * self.dims;
            let score = dot_f32_i8(query, &self.data[at..at + self.dims], self.row_scale[row]);
            if !score.is_finite() || score < floor {
                continue;
            }
            if heap.len() < top_chunks {
                heap.push(Reverse(Scored {
                    score,
                    row: row as u32,
                }));
            } else if heap.peek().is_some_and(|Reverse(low)| score > low.score) {
                heap.pop();
                heap.push(Reverse(Scored {
                    score,
                    row: row as u32,
                }));
            }
        }
        let mut best: Vec<Scored> = heap.into_iter().map(|Reverse(s)| s).collect();
        best.sort_by(|a, b| b.cmp(a));
        let mut seen = vec![false; self.docs.len()];
        let mut out = Vec::new();
        for s in best {
            let doc = self.row_doc[s.row as usize] as usize;
            if std::mem::replace(&mut seen[doc], true) {
                continue;
            }
            let d = &self.docs[doc];
            out.push(DocHit {
                corpus: self.names[d.corpus as usize].clone(),
                key: d.key.clone(),
                chunk: self.row_hash[s.row as usize],
                score: s.score,
            });
        }
        out
    }
}

#[derive(Default)]
pub struct Sets {
    sets: Mutex<HashMap<ModelId, Arc<VectorSet>>>,
}

impl Sets {
    pub fn get(&self, model: &ModelId) -> Option<Arc<VectorSet>> {
        self.sets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(model)
            .cloned()
    }

    pub fn swap(&self, model: &ModelId, set: VectorSet) -> Option<Arc<VectorSet>> {
        self.sets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(model.clone(), Arc::new(set))
    }

    pub fn remove(&self, model: &ModelId) -> Option<Arc<VectorSet>> {
        self.sets
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(model)
    }
}
