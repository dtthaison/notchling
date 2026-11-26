use crate::modernbert::{Config, ModernBert};
use crate::tokens::Tokens;
use candle_core::Shape;
use candle_core::{DType, Device, Tensor};
use candle_nn::var_builder::SimpleBackend;
use candle_nn::{Init, VarBuilder};
use memmap2::Mmap;
use std::collections::HashMap;
use std::path::Path;

pub const MODEL: &str = "granite-97m-r2@835ad14/p1";
pub const MAX_TOKENS: usize = 512;
pub const TOKENS_FILE: &str = "tokenizer.notchling";
const WORDS_KEY: &str = "embeddings.tok_embeddings.weight";

enum Source {
    Bytes(Box<dyn AsRef<[u8]> + Send + Sync>),
    File(std::fs::File),
}

pub struct Words {
    source: Source,
    offset: usize,
    rows: usize,
    dims: usize,
    dtype: DType,
}

#[cfg(unix)]
fn read_at(file: &std::fs::File, buf: &mut [u8], offset: u64) -> std::io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

#[cfg(windows)]
fn read_at(file: &std::fs::File, mut buf: &mut [u8], mut offset: u64) -> std::io::Result<()> {
    while !buf.is_empty() {
        let n = std::os::windows::fs::FileExt::seek_read(file, buf, offset)?;
        if n == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        buf = &mut buf[n..];
        offset += n as u64;
    }
    Ok(())
}

impl Words {
    pub fn from_bytes(
        bytes: Box<dyn AsRef<[u8]> + Send + Sync>,
        offset: usize,
        rows: usize,
        dims: usize,
        dtype: DType,
    ) -> Result<Words, String> {
        let len = (*bytes).as_ref().len();
        Self::new(Source::Bytes(bytes), len, offset, rows, dims, dtype)
    }

    fn new(
        source: Source,
        len: usize,
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
        if len < need {
            return Err(format!("word table needs {need} bytes, the file has {len}"));
        }
        Ok(Words {
            source,
            offset,
            rows,
            dims,
            dtype,
        })
    }

    fn width(&self) -> usize {
        if self.dtype == DType::F32 {
            4
        } else {
            2
        }
    }

    fn push_row(&self, id: u32, buf: &mut Vec<u8>, out: &mut Vec<f32>) -> Result<(), String> {
        let id = (id as usize).min(self.rows.saturating_sub(1));
        let len = self.dims * self.width();
        let start = self.offset + id * len;
        let bytes: &[u8] = match &self.source {
            Source::Bytes(bytes) => &(**bytes).as_ref()[start..start + len],
            Source::File(file) => {
                buf.resize(len, 0);
                read_at(file, buf, start as u64).map_err(|e| format!("word table: {e}"))?;
                buf
            }
        };
        match self.dtype {
            DType::F32 => out.extend(
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| f32::from_le_bytes(*b)),
            ),
            DType::F16 => out.extend(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| half::f16::from_le_bytes(*b).to_f32()),
            ),
            _ => out.extend(
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| half::bf16::from_le_bytes(*b).to_f32()),
            ),
        }
        Ok(())
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

fn tensor_dtype(dtype: safetensors::Dtype) -> Result<DType, String> {
    match dtype {
        safetensors::Dtype::F32 => Ok(DType::F32),
        other => safetensors_dtype(other),
    }
}

struct Layout {
    file: std::fs::File,
    len: usize,
    words: (usize, usize, usize, DType),
    tensors: HashMap<String, (usize, usize, Vec<usize>, safetensors::Dtype)>,
}

fn read_layout(path: &Path) -> Result<Layout, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let map = unsafe { Mmap::map(&file) }.map_err(|e| format!("{}: {e}", path.display()))?;
    let (n, meta) = safetensors::SafeTensors::read_metadata(&map).map_err(|e| e.to_string())?;
    let base = n + 8;
    let mut words = None;
    let mut tensors = HashMap::new();
    for (name, info) in meta.tensors() {
        let (start, end) = info.data_offsets;
        if name == WORDS_KEY {
            if info.shape.len() != 2 {
                return Err(format!("word table has shape {:?}", info.shape));
            }
            let dtype = safetensors_dtype(info.dtype)?;
            words = Some((base + start, info.shape[0], info.shape[1], dtype));
        } else {
            tensors.insert(
                name,
                (base + start, end - start, info.shape.clone(), info.dtype),
            );
        }
    }
    let words = words.ok_or_else(|| format!("{} has no {WORDS_KEY}", path.display()))?;
    Ok(Layout {
        len: map.len(),
        file,
        words,
        tensors,
    })
}

fn read_tensor(
    file: &std::fs::File,
    offset: usize,
    len: usize,
    shape: &[usize],
    dtype: DType,
) -> Result<Tensor, String> {
    let mut buf = vec![0u8; len];
    read_at(file, &mut buf, offset as u64).map_err(|e| format!("model.safetensors: {e}"))?;
    let values: Vec<f32> = match dtype {
        DType::F32 => buf
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect(),
        DType::F16 => buf
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| half::f16::from_le_bytes(*b).to_f32())
            .collect(),
        _ => buf
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| half::bf16::from_le_bytes(*b).to_f32())
            .collect(),
    };
    Tensor::from_vec(values, shape, &Device::Cpu).map_err(|e| e.to_string())
}

impl SimpleBackend for &Layout {
    fn get(
        &self,
        s: Shape,
        name: &str,
        _: Init,
        dtype: DType,
        dev: &Device,
    ) -> candle_core::Result<Tensor> {
        let tensor = self.get_unchecked(name, dtype, dev)?;
        if tensor.shape() != &s {
            candle_core::bail!("{name} is {:?}, expected {s:?}", tensor.shape())
        }
        Ok(tensor)
    }

    fn get_unchecked(&self, name: &str, dtype: DType, dev: &Device) -> candle_core::Result<Tensor> {
        let Some((offset, len, shape, stored)) = self.tensors.get(name) else {
            candle_core::bail!("model.safetensors has no {name}")
        };
        let Ok(stored) = tensor_dtype(*stored) else {
            candle_core::bail!("{name} in {stored:?} is not supported")
        };
        read_tensor(&self.file, *offset, *len, shape, stored)
            .map_err(candle_core::Error::msg)?
            .to_dtype(dtype)?
            .to_device(dev)
    }

    fn contains_tensor(&self, name: &str) -> bool {
        self.tensors.contains_key(name)
    }
}

#[cfg(target_os = "macos")]
fn give_back_freed_memory() {
    extern "C" {
        fn malloc_zone_pressure_relief(zone: *mut libc::c_void, goal: libc::size_t)
            -> libc::size_t;
    }
    unsafe {
        malloc_zone_pressure_relief(std::ptr::null_mut(), 0);
    }
}

#[cfg(not(target_os = "macos"))]
fn give_back_freed_memory() {}

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
        let native = dir.join(TOKENS_FILE);
        let tokens = match native.is_file().then(|| Tokens::from_native_file(&native)) {
            Some(Ok(tokens)) => tokens,
            _ => Tokens::from_tokenizer_json(&dir.join("tokenizer.json"))?,
        };
        give_back_freed_memory();
        let layout = read_layout(&dir.join("model.safetensors"))?;
        let (offset, rows, dims, dtype) = layout.words;
        if rows != config.vocab_size || dims != config.hidden_size {
            return Err(format!(
                "word table is {rows} by {dims}, config.json says {} by {}",
                config.vocab_size, config.hidden_size
            ));
        }
        let model = {
            let vb = VarBuilder::from_backend(Box::new(&layout), DType::F32, Device::Cpu);
            ModernBert::load_encoder(vb, &config).map_err(|e| e.to_string())?
        };
        let words = Words::new(
            Source::File(layout.file),
            layout.len,
            offset,
            rows,
            dims,
            dtype,
        )?;
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
        self.forward(&ids, true)
    }

    fn embed_one(&self, text: &str) -> Result<Vec<f32>, String> {
        let ids = vec![self.tokens.encode(text, self.max_tokens)];
        Ok(self.forward(&ids, false)?.pop().unwrap_or_default())
    }

    fn forward(&self, ids: &[Vec<u32>], full: bool) -> Result<Vec<Vec<f32>>, String> {
        let seq = ids.iter().map(Vec::len).max().unwrap_or(0).max(1);
        let pad = self.tokens.pad_id();
        let dims = self.words.dims;
        let mut rows = Vec::with_capacity(ids.len() * seq * dims);
        let mut mask = Vec::with_capacity(ids.len() * seq);
        let mut buf = Vec::new();
        for text in ids {
            for i in 0..seq {
                let id = text.get(i).copied();
                self.words
                    .push_row(id.unwrap_or(pad), &mut buf, &mut rows)?;
                mask.push(u32::from(id.is_some()));
            }
        }
        let run = || -> candle_core::Result<Vec<Vec<f32>>> {
            let embedded = Tensor::from_vec(rows, (ids.len(), seq, dims), &Device::Cpu)?;
            let mask = Tensor::from_vec(mask, (ids.len(), seq), &Device::Cpu)?;
            let first = if full {
                self.model
                    .forward_embedded(&embedded, &mask)?
                    .narrow(1, 0, 1)?
                    .squeeze(1)?
            } else {
                self.model.forward_first(&embedded, &mask)?
            };
            first.to_dtype(DType::F32)?.to_vec2()
        };
        let mut out = run().map_err(|e| e.to_string())?;
        for v in &mut out {
            let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
            v.iter_mut().for_each(|x| *x /= n);
        }
        Ok(out)
    }
}
