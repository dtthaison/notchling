use crate::modernbert::{Config, ModernBert};
use crate::tokens::Tokens;
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use memmap2::Mmap;
use std::path::Path;

pub const MODEL: &str = "granite-97m-r2@835ad14/p1";
pub const MAX_TOKENS: usize = 512;
pub const TOKENS_FILE: &str = "tokenizer.notchling";
const WORDS_KEY: &str = "embeddings.tok_embeddings.weight";

pub struct Words {
    bytes: Box<dyn AsRef<[u8]> + Send + Sync>,
    offset: usize,
    rows: usize,
    dims: usize,
    dtype: DType,
}

struct Mapped(Mmap);

impl AsRef<[u8]> for Mapped {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Words {
    pub fn from_bytes(
        bytes: Box<dyn AsRef<[u8]> + Send + Sync>,
        offset: usize,
        rows: usize,
        dims: usize,
        dtype: DType,
    ) -> Result<Words, String> {
        let width = match dtype {
            DType::BF16 | DType::F16 => 2,
            DType::F32 => 4,
            other => return Err(format!("word table in {other:?} is not supported")),
        };
        if rows == 0 || dims == 0 {
            return Err(format!("word table is {rows} by {dims}"));
        }
        let need = rows
            .checked_mul(dims)
            .and_then(|n| n.checked_mul(width))
            .and_then(|n| n.checked_add(offset))
            .ok_or_else(|| format!("word table of {rows} by {dims} is too large"))?;
        if (*bytes).as_ref().len() < need {
            return Err(format!(
                "word table needs {need} bytes, the file has {}",
                (*bytes).as_ref().len()
            ));
        }
        Ok(Words {
            bytes,
            offset,
            rows,
            dims,
            dtype,
        })
    }

    fn push_row(&self, id: u32, out: &mut Vec<f32>) {
        let id = (id as usize).min(self.rows.saturating_sub(1));
        let bytes = (*self.bytes).as_ref();
        match self.dtype {
            DType::F32 => {
                let start = self.offset + id * self.dims * 4;
                out.extend(
                    bytes[start..start + self.dims * 4]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b)),
                );
            }
            DType::F16 => {
                let start = self.offset + id * self.dims * 2;
                out.extend(
                    bytes[start..start + self.dims * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| half::f16::from_le_bytes(*b).to_f32()),
                );
            }
            _ => {
                let start = self.offset + id * self.dims * 2;
                out.extend(
                    bytes[start..start + self.dims * 2]
                        .as_chunks::<2>()
                        .0
                        .iter()
                        .map(|b| half::bf16::from_le_bytes(*b).to_f32()),
                );
            }
        }
    }
}

fn safetensors_dtype(dtype: safetensors::Dtype) -> Result<DType, String> {
    match dtype {
        safetensors::Dtype::BF16 => Ok(DType::BF16),
        safetensors::Dtype::F16 => Ok(DType::F16),
        safetensors::Dtype::F32 => Ok(DType::F32),
        other => Err(format!("word table in {other:?} is not supported")),
    }
}

fn map_words(file: &Path) -> Result<Words, String> {
    let f = std::fs::File::open(file).map_err(|e| format!("{}: {e}", file.display()))?;
    let map = unsafe { Mmap::map(&f) }.map_err(|e| format!("{}: {e}", file.display()))?;
    let (offset, rows, dims, dtype) = {
        let st = safetensors::SafeTensors::deserialize(&map).map_err(|e| e.to_string())?;
        let view = st.tensor(WORDS_KEY).map_err(|e| e.to_string())?;
        let shape = view.shape();
        if shape.len() != 2 {
            return Err(format!("word table has shape {shape:?}"));
        }
        let offset = view.data().as_ptr() as usize - map.as_ptr() as usize;
        (offset, shape[0], shape[1], safetensors_dtype(view.dtype())?)
    };
    Words::from_bytes(Box::new(Mapped(map)), offset, rows, dims, dtype)
}

pub struct Granite {
    model: ModernBert,
    words: Words,
    tokens: Tokens,
    max_tokens: usize,
}

impl Granite {
    pub fn load(dir: &Path) -> Result<Granite, String> {
        let text = std::fs::read_to_string(dir.join("config.json"))
            .map_err(|e| format!("config.json: {e}"))?;
        let mut config: Config =
            serde_json::from_str(&text).map_err(|e| format!("config.json: {e}"))?;
        config.max_position_embeddings = config.max_position_embeddings.min(MAX_TOKENS);
        let file = dir.join("model.safetensors");
        let words = map_words(&file)?;
        if words.rows != config.vocab_size || words.dims != config.hidden_size {
            return Err(format!(
                "word table is {} by {}, config.json says {} by {}",
                words.rows, words.dims, config.vocab_size, config.hidden_size
            ));
        }
        let model = {
            let vb =
                unsafe { VarBuilder::from_mmaped_safetensors(&[&file], DType::F32, &Device::Cpu) }
                    .map_err(|e| e.to_string())?;
            ModernBert::load_encoder(vb, &config).map_err(|e| e.to_string())?
        };
        let native = dir.join(TOKENS_FILE);
        let tokens = match native.is_file().then(|| Tokens::from_native_file(&native)) {
            Some(Ok(tokens)) => tokens,
            _ => Tokens::from_tokenizer_json(&dir.join("tokenizer.json"))?,
        };
        Ok(Self::from_parts(
            model,
            words,
            tokens,
            config.max_position_embeddings,
        ))
    }

    pub fn from_parts(
        model: ModernBert,
        words: Words,
        tokens: Tokens,
        max_tokens: usize,
    ) -> Granite {
        Granite {
            model,
            words,
            tokens,
            max_tokens,
        }
    }

    pub fn dims(&self) -> usize {
        self.words.dims
    }

    pub fn tokens(&self, text: &str) -> usize {
        self.tokens.encode(text, self.max_tokens).len()
    }

    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        texts.iter().map(|t| self.embed_one(t)).collect()
    }

    pub fn embed_padded(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, String> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let ids: Vec<Vec<u32>> = texts
            .iter()
            .map(|t| self.tokens.encode(t, self.max_tokens))
            .collect();
        self.forward(&ids)
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>, String> {
        let ids = vec![self.tokens.encode(text, self.max_tokens)];
        Ok(self.forward(&ids)?.pop().unwrap_or_default())
    }

    fn forward(&self, ids: &[Vec<u32>]) -> Result<Vec<Vec<f32>>, String> {
        let seq = ids.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let pad = self.tokens.pad_id();
        let dims = self.words.dims;
        let mut rows = Vec::with_capacity(ids.len() * seq * dims);
        let mut mask = Vec::with_capacity(ids.len() * seq);
        for text in ids {
            for i in 0..seq {
                let id = text.get(i).copied();
                self.words.push_row(id.unwrap_or(pad), &mut rows);
                mask.push(u32::from(id.is_some()));
            }
        }
        let run = || -> candle_core::Result<Vec<Vec<f32>>> {
            let embedded = Tensor::from_vec(rows, (ids.len(), seq, dims), &Device::Cpu)?;
            let mask = Tensor::from_vec(mask, (ids.len(), seq), &Device::Cpu)?;
            let out = self.model.forward_embedded(&embedded, &mask)?;
            out.narrow(1, 0, 1)?
                .squeeze(1)?
                .to_dtype(DType::F32)?
                .to_vec2()
        };
        let mut out = run().map_err(|e| e.to_string())?;
        for v in &mut out {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            v.iter_mut().for_each(|x| *x /= n);
        }
        Ok(out)
    }
}
