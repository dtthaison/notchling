use super::{EmbedError, Embedder, ModelId};
use kitoken::Kitoken;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;

pub const MAGIC: &[u8; 8] = b"NLST1\0\0\0";
pub const MODEL: &str = "static-mrl@b68f412/512i8/p1";

const HEADER: usize = 16;

pub struct StaticEmbedder {
    model: ModelId,
    kit: Kitoken,
    table: Mmap,
    rows: usize,
    dims: usize,
}

fn le_u32(bytes: &[u8]) -> usize {
    let mut b = [0u8; 4];
    b.copy_from_slice(&bytes[..4]);
    u32::from_le_bytes(b) as usize
}

fn shape(bytes: &[u8]) -> Result<(usize, usize), String> {
    if bytes.len() < HEADER {
        return Err("the static table is shorter than its header".to_string());
    }
    if &bytes[..8] != MAGIC {
        return Err("not a static table".to_string());
    }
    let rows = le_u32(&bytes[8..]);
    let dims = le_u32(&bytes[12..]);
    if rows == 0 || dims == 0 {
        return Err("the static table has no rows or no dimensions".to_string());
    }
    let body = rows
        .checked_mul(4)
        .and_then(|scales| rows.checked_mul(dims)?.checked_add(scales))
        .and_then(|n| n.checked_add(HEADER))
        .ok_or("the static table's header is out of range")?;
    if bytes.len() != body {
        return Err(format!(
            "the static table has {} bytes, its header says {body}",
            bytes.len()
        ));
    }
    Ok((rows, dims))
}

fn tokenizer(path: &Path) -> Result<Kitoken, String> {
    let json = path.extension().is_some_and(|e| e == "json");
    let kit = if json {
        Kitoken::from_tokenizers_file(path).map_err(|e| e.to_string())
    } else {
        Kitoken::from_file(path).map_err(|e| e.to_string())
    };
    kit.map_err(|e| format!("{}: {e}", path.display()))
}

impl StaticEmbedder {
    pub fn open(table: &Path, tokenizer_file: &Path) -> Result<StaticEmbedder, String> {
        let file = File::open(table).map_err(|e| format!("{}: {e}", table.display()))?;
        let map = unsafe { Mmap::map(&file) }.map_err(|e| format!("{}: {e}", table.display()))?;
        let (rows, dims) = shape(&map)?;
        let kit = tokenizer(tokenizer_file)?;
        Ok(StaticEmbedder {
            model: ModelId(MODEL.to_string()),
            kit,
            table: map,
            rows,
            dims,
        })
    }

    fn add_row(&self, id: usize, sum: &mut [f32]) -> bool {
        if id >= self.rows {
            return false;
        }
        let at = HEADER + id * 4;
        let mut b = [0u8; 4];
        b.copy_from_slice(&self.table[at..at + 4]);
        let scale = f32::from_le_bytes(b);
        let start = HEADER + self.rows * 4 + id * self.dims;
        for (s, &q) in sum.iter_mut().zip(&self.table[start..start + self.dims]) {
            *s += f32::from(q as i8) * scale;
        }
        true
    }

    fn vector(&self, text: &str) -> Result<Vec<f32>, EmbedError> {
        let ids = self
            .kit
            .encode(text, false)
            .map_err(|e| EmbedError::Failed(e.to_string()))?;
        let mut sum = vec![0f32; self.dims];
        let mut used = 0usize;
        for id in ids {
            if self.add_row(id as usize, &mut sum) {
                used += 1;
            }
        }
        if used == 0 {
            return Ok(sum);
        }
        let norm = sum.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 && norm.is_finite() {
            sum.iter_mut().for_each(|x| *x /= norm);
        }
        Ok(sum)
    }
}

impl Embedder for StaticEmbedder {
    fn model(&self) -> &ModelId {
        &self.model
    }

    fn dims(&self) -> usize {
        self.dims
    }

    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError> {
        texts.iter().map(|t| self.vector(t)).collect()
    }
}
