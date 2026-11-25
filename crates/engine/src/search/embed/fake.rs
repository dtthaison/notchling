use super::{EmbedError, Embedder, ModelId};
use crate::search::query::fold;
use std::collections::HashMap;

const HASH_DIMS: usize = 64;

pub struct HashEmbedder {
    model: ModelId,
}

impl HashEmbedder {
    pub fn new(model: &str) -> Self {
        Self {
            model: ModelId(model.to_string()),
        }
    }

    fn vector(text: &str) -> Vec<f32> {
        let folded: Vec<char> = fold(text).chars().collect();
        let mut v = vec![0f32; HASH_DIMS];
        for w in folded.windows(3) {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for c in w {
                for b in c.to_string().bytes() {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0000_0100_0000_01b3);
                }
            }
            v[(h % HASH_DIMS as u64) as usize] += 1.0;
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        v
    }
}

impl Embedder for HashEmbedder {
    fn model(&self) -> &ModelId {
        &self.model
    }

    fn dims(&self) -> usize {
        HASH_DIMS
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        Ok(texts.iter().map(|t| Self::vector(t)).collect())
    }
}

pub struct TableEmbedder {
    model: ModelId,
    dims: usize,
    rows: HashMap<String, Vec<f32>>,
}

impl TableEmbedder {
    pub fn new(model: &str, dims: usize, rows: Vec<(String, Vec<f32>)>) -> Self {
        Self {
            model: ModelId(model.to_string()),
            dims,
            rows: rows.into_iter().collect(),
        }
    }
}

impl Embedder for TableEmbedder {
    fn model(&self) -> &ModelId {
        &self.model
    }

    fn dims(&self) -> usize {
        self.dims
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        texts
            .iter()
            .map(|t| match self.rows.get(*t) {
                Some(v) if v.len() == self.dims => Ok(v.clone()),
                Some(_) => Err(EmbedError::Failed("wrong dimensions".to_string())),
                None => Err(EmbedError::Failed("unknown text".to_string())),
            })
            .collect()
    }
}
