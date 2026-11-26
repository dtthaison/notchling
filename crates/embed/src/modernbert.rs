use candle_core::{
    CpuStorage, CustomOp1, CustomOp2, DType, Device, Layout, Result, Shape, Tensor, D,
};
use candle_nn::{
    embedding, layer_norm_no_bias, ops::softmax_last_dim, Embedding, LayerNorm, Module, VarBuilder,
};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Activation {
    #[default]
    #[serde(alias = "gelu_python", alias = "gelu_new")]
    Gelu,
    #[serde(alias = "swish")]
    Silu,
}

#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Config {
    pub vocab_size: usize,
    pub hidden_size: usize,
    pub num_hidden_layers: usize,
    pub num_attention_heads: usize,
    pub intermediate_size: usize,
    pub max_position_embeddings: usize,
    pub layer_norm_eps: f64,
    pub pad_token_id: u32,
    pub global_attn_every_n_layers: usize,
    pub global_rope_theta: f64,
    pub local_attention: usize,
    pub local_rope_theta: f64,
    #[serde(default)]
    pub hidden_activation: Activation,
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        if self.hidden_size == 0 || self.num_attention_heads == 0 {
            candle_core::bail!("config has no hidden size or no attention heads")
        }
        if !self.hidden_size.is_multiple_of(self.num_attention_heads) {
            candle_core::bail!(
                "hidden size {} does not split into {} heads",
                self.hidden_size,
                self.num_attention_heads
            )
        }
        if !(self.hidden_size / self.num_attention_heads).is_multiple_of(2) {
            candle_core::bail!(
                "a head of {} is not even",
                self.hidden_size / self.num_attention_heads
            )
        }
        if self.max_position_embeddings == 0 || self.num_hidden_layers == 0 {
            candle_core::bail!("config has no positions or no layers")
        }
        Ok(())
    }
}

pub fn matmul(a: &Tensor, b: &Tensor) -> Result<Tensor> {
    if cfg!(target_os = "macos") && a.dtype() != DType::F32 {
        let out = a.to_dtype(DType::F32)?.matmul(&b.to_dtype(DType::F32)?)?;
        return out.to_dtype(a.dtype());
    }
    a.matmul(b)
}

#[derive(Debug, Clone)]
struct Linear {
    weight: Tensor,
}

impl Linear {
    fn load(input: usize, output: usize, vb: VarBuilder) -> Result<Self> {
        Ok(Self {
            weight: vb.get((output, input), "weight")?,
        })
    }
}

impl Module for Linear {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let dims = xs.dims().to_vec();
        let input = dims[dims.len() - 1];
        let rows = xs.elem_count() / input.max(1);
        let out = matmul(&xs.reshape((rows, input))?, &self.weight.t()?)?;
        let mut shape = dims;
        let last = shape.len() - 1;
        shape[last] = self.weight.dim(0)?;
        out.reshape(shape)
    }
}

#[derive(Debug, Clone)]
struct Rotary {
    sin: Tensor,
    cos: Tensor,
}

impl Rotary {
    fn new(dtype: DType, config: &Config, theta: f64, dev: &Device) -> Result<Self> {
        let dim = config.hidden_size / config.num_attention_heads;
        let inv: Vec<f32> = (0..dim)
            .step_by(2)
            .map(|i| 1f32 / theta.powf(i as f64 / dim as f64) as f32)
            .collect();
        let half = inv.len();
        let inv = Tensor::from_vec(inv, (1, half), dev)?;
        let len = config.max_position_embeddings;
        let t = Tensor::arange(0u32, len as u32, dev)?
            .to_dtype(DType::F32)?
            .reshape((len, 1))?;
        let freqs = t.matmul(&inv)?;
        Ok(Self {
            sin: freqs.sin()?.to_dtype(dtype)?,
            cos: freqs.cos()?.to_dtype(dtype)?,
        })
    }

    fn apply(&self, q: &Tensor, k: &Tensor) -> Result<(Tensor, Tensor)> {
        let q = candle_nn::rotary_emb::rope(&q.contiguous()?, &self.cos, &self.sin)?;
        let k = candle_nn::rotary_emb::rope(&k.contiguous()?, &self.cos, &self.sin)?;
        Ok((q, k))
    }

    fn apply_first(&self, q: &Tensor, k: &Tensor) -> Result<(Tensor, Tensor)> {
        let cos = self.cos.narrow(0, 0, 1)?;
        let sin = self.sin.narrow(0, 0, 1)?;
        let q = candle_nn::rotary_emb::rope(&q.narrow(2, 0, 1)?.contiguous()?, &cos, &sin)?;
        let k = candle_nn::rotary_emb::rope(&k.contiguous()?, &self.cos, &self.sin)?;
        Ok((q, k))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Path {
    Full,
    Fast,
    First,
}

#[derive(Clone)]
struct Attention {
    qkv: Linear,
    proj: Linear,
    heads: usize,
    head_size: usize,
    rotary: Arc<Rotary>,
}

impl Attention {
    fn load(vb: VarBuilder, config: &Config, rotary: Arc<Rotary>) -> Result<Self> {
        let heads = config.num_attention_heads;
        let head_size = config.hidden_size / heads;
        let qkv = Linear::load(config.hidden_size, config.hidden_size * 3, vb.pp("Wqkv"))?;
        let proj = Linear::load(config.hidden_size, config.hidden_size, vb.pp("Wo"))?;
        Ok(Self {
            qkv,
            proj,
            heads,
            head_size,
            rotary,
        })
    }

    fn forward(&self, xs: &Tensor, mask: Option<&Tensor>, path: Path) -> Result<Tensor> {
        let (b, seq, d) = xs.dims3()?;
        let qkv = xs
            .apply(&self.qkv)?
            .reshape((b, seq, 3, self.heads, self.head_size))?
            .permute((2, 0, 3, 1, 4))?;
        let q = qkv.get(0)?;
        let k = qkv.get(1)?;
        let v = qkv.get(2)?;
        let (q, k) = if path == Path::First {
            self.rotary.apply_first(&q, &k)?
        } else {
            self.rotary.apply(&q, &k)?
        };
        let rows = q.dim(2)?;
        let q = (q * (self.head_size as f64).powf(-0.5))?;
        let kt = k.transpose(D::Minus2, D::Minus1)?;
        let kt = if path == Path::Full {
            kt.contiguous()?
        } else {
            kt
        };
        let att = matmul(&q, &kt)?;
        let mask = match mask {
            Some(mask) if mask.dim(2)? > rows => Some(mask.narrow(2, 0, rows)?),
            Some(mask) => Some(mask.clone()),
            None => None,
        };
        let att = match (path, mask) {
            (Path::Full, Some(mask)) => softmax_last_dim(&att.broadcast_add(&mask)?)?,
            (Path::Full, None) => softmax_last_dim(&att)?,
            (_, mask) => masked_softmax(&att, mask.as_ref())?,
        };
        let xs = matmul(&att, &v.contiguous()?)?;
        xs.transpose(1, 2)?.reshape((b, rows, d))?.apply(&self.proj)
    }
}

#[derive(Clone)]
struct Mlp {
    wi: Linear,
    wo: Linear,
    act: Activation,
}

impl Mlp {
    fn load(vb: VarBuilder, config: &Config) -> Result<Self> {
        let wi = Linear::load(
            config.hidden_size,
            config.intermediate_size * 2,
            vb.pp("Wi"),
        )?;
        let wo = Linear::load(config.intermediate_size, config.hidden_size, vb.pp("Wo"))?;
        Ok(Self {
            wi,
            wo,
            act: config.hidden_activation,
        })
    }
}

impl Module for Mlp {
    fn forward(&self, xs: &Tensor) -> Result<Tensor> {
        let xs = xs.apply(&self.wi)?.chunk(2, D::Minus1)?;
        let gate = match self.act {
            Activation::Silu => xs[0].silu()?,
            Activation::Gelu => xs[0].gelu_erf()?,
        };
        (gate * &xs[1])?.apply(&self.wo)
    }
}

#[derive(Clone)]
struct Layer {
    attn: Attention,
    mlp: Mlp,
    attn_norm: Option<LayerNorm>,
    mlp_norm: LayerNorm,
    local: bool,
}

impl Layer {
    fn load(
        vb: VarBuilder,
        config: &Config,
        rotary: Arc<Rotary>,
        local: bool,
        first: bool,
    ) -> Result<Self> {
        let attn = Attention::load(vb.pp("attn"), config, rotary)?;
        let mlp = Mlp::load(vb.pp("mlp"), config)?;
        let attn_norm = if first && !vb.contains_tensor("attn_norm.weight") {
            None
        } else {
            Some(layer_norm_no_bias(
                config.hidden_size,
                config.layer_norm_eps,
                vb.pp("attn_norm"),
            )?)
        };
        let mlp_norm =
            layer_norm_no_bias(config.hidden_size, config.layer_norm_eps, vb.pp("mlp_norm"))?;
        Ok(Self {
            attn,
            mlp,
            attn_norm,
            mlp_norm,
            local,
        })
    }

    fn forward(
        &self,
        xs: &Tensor,
        global: Option<&Tensor>,
        local: Option<&Tensor>,
        path: Path,
    ) -> Result<Tensor> {
        let normed = match &self.attn_norm {
            Some(norm) => xs.apply(norm)?,
            None => xs.clone(),
        };
        let mask = if self.local { local } else { global };
        let rest = if path == Path::First {
            xs.narrow(1, 0, 1)?
        } else {
            xs.clone()
        };
        let xs = (self.attn.forward(&normed, mask, path)? + rest)?;
        let mlp = xs.apply(&self.mlp_norm)?.apply(&self.mlp)?;
        xs + mlp
    }
}

#[cfg(target_os = "macos")]
#[link(name = "Accelerate", kind = "framework")]
extern "C" {
    fn vvexpf(dst: *mut f32, src: *const f32, len: *const i32);
}

fn exp_in_place(values: &mut [f32]) {
    #[cfg(target_os = "macos")]
    for chunk in values.chunks_mut(1 << 24) {
        let len = chunk.len() as i32;
        let ptr = chunk.as_mut_ptr();
        unsafe { vvexpf(ptr, ptr, &len) };
    }
    #[cfg(not(target_os = "macos"))]
    values.iter_mut().for_each(|v| *v = v.exp());
}

struct Mask<'a> {
    values: &'a [f32],
    rows_per_text: usize,
    rows: usize,
    heads: usize,
}

fn softmax_rows(src: &[f32], cols: usize, mask: Option<Mask>) -> Vec<f32> {
    let mut dst = src.to_vec();
    if cols == 0 {
        return dst;
    }
    for (i, row) in dst.chunks_mut(cols).enumerate() {
        if let Some(m) = &mask {
            let r = if m.rows_per_text == 1 { 0 } else { i % m.rows };
            let start = ((i / (m.rows * m.heads)) * m.rows_per_text + r) * cols;
            row.iter_mut()
                .zip(&m.values[start..start + cols])
                .for_each(|(x, v)| *x += v);
        }
        let max = row.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        row.iter_mut().for_each(|x| *x -= max);
    }
    exp_in_place(&mut dst);
    for row in dst.chunks_mut(cols) {
        let scale = 1.0 / row.iter().sum::<f32>();
        row.iter_mut().for_each(|x| *x *= scale);
    }
    dst
}

fn f32_slice<'a>(storage: &'a CpuStorage, layout: &Layout) -> Result<&'a [f32]> {
    let CpuStorage::F32(values) = storage else {
        candle_core::bail!("softmax takes f32")
    };
    let Some((start, end)) = layout.contiguous_offsets() else {
        candle_core::bail!("softmax takes a contiguous tensor")
    };
    Ok(&values[start..end])
}

struct Softmax;

impl CustomOp1 for Softmax {
    fn name(&self) -> &'static str {
        "notchling-softmax"
    }

    fn cpu_fwd(&self, storage: &CpuStorage, layout: &Layout) -> Result<(CpuStorage, Shape)> {
        let src = f32_slice(storage, layout)?;
        let cols = layout.dims().last().copied().unwrap_or(1).max(1);
        let out = softmax_rows(src, cols, None);
        Ok((CpuStorage::F32(out), layout.shape().clone()))
    }
}

struct MaskedSoftmax;

impl CustomOp2 for MaskedSoftmax {
    fn name(&self) -> &'static str {
        "notchling-masked-softmax"
    }

    fn cpu_fwd(
        &self,
        s1: &CpuStorage,
        l1: &Layout,
        s2: &CpuStorage,
        l2: &Layout,
    ) -> Result<(CpuStorage, Shape)> {
        let src = f32_slice(s1, l1)?;
        let values = f32_slice(s2, l2)?;
        let &[b, heads, rows, cols] = l1.dims() else {
            candle_core::bail!("softmax takes scores of rank 4")
        };
        let &[mb, 1, rows_per_text, mcols] = l2.dims() else {
            candle_core::bail!("softmax takes a mask of shape (b, 1, r, c)")
        };
        if mb != b || mcols != cols || (rows_per_text != 1 && rows_per_text != rows) {
            candle_core::bail!("mask {:?} does not fit scores {:?}", l2.dims(), l1.dims())
        }
        let mask = Mask {
            values,
            rows_per_text,
            rows,
            heads,
        };
        let out = softmax_rows(src, cols, Some(mask));
        Ok((CpuStorage::F32(out), l1.shape().clone()))
    }
}

pub(crate) fn masked_softmax(att: &Tensor, mask: Option<&Tensor>) -> Result<Tensor> {
    if att.dtype() != DType::F32 || !att.device().is_cpu() {
        let att = match mask {
            Some(mask) => att.broadcast_add(mask)?,
            None => att.clone(),
        };
        return softmax_last_dim(&att);
    }
    let att = att.contiguous()?;
    match mask {
        Some(mask) => {
            att.apply_op2_no_bwd(&mask.to_dtype(DType::F32)?.contiguous()?, &MaskedSoftmax)
        }
        None => att.apply_op1_no_bwd(&Softmax),
    }
}

pub fn mask_floor(dtype: DType) -> f32 {
    match dtype {
        DType::F16 => half::f16::MIN.to_f32(),
        DType::BF16 => half::bf16::MIN.to_f32(),
        _ => f32::MIN,
    }
}

pub fn attention_masks(mask: &Tensor, window: usize, dtype: DType) -> Result<(Tensor, Tensor)> {
    let (b, seq) = mask.dims2()?;
    let floor = mask_floor(dtype);
    let keep: Vec<Vec<f32>> = mask.to_dtype(DType::F32)?.to_vec2()?;
    let mut global = Vec::with_capacity(b * seq);
    let mut local = Vec::with_capacity(b * seq * seq);
    for row in &keep {
        global.extend(row.iter().map(|&m| if m > 0.0 { 0.0 } else { floor }));
        for i in 0..seq {
            for (j, &m) in row.iter().enumerate() {
                let far = i.abs_diff(j) > window;
                local.push(if m > 0.0 && !far { 0.0 } else { floor });
            }
        }
    }
    let dev = mask.device();
    let global = Tensor::from_vec(global, (b, 1, 1, seq), dev)?.to_dtype(dtype)?;
    let local = Tensor::from_vec(local, (b, 1, seq, seq), dev)?.to_dtype(dtype)?;
    Ok((global, local))
}

#[derive(Clone)]
pub struct ModernBert {
    words: Option<Embedding>,
    norm: LayerNorm,
    layers: Vec<Layer>,
    final_norm: LayerNorm,
    window: usize,
    dtype: DType,
}

impl ModernBert {
    pub fn load(vb: VarBuilder, config: &Config) -> Result<Self> {
        let words = embedding(
            config.vocab_size,
            config.hidden_size,
            vb.pp("embeddings.tok_embeddings"),
        )?;
        let mut model = Self::load_encoder(vb, config)?;
        model.words = Some(words);
        Ok(model)
    }

    pub fn load_encoder(vb: VarBuilder, config: &Config) -> Result<Self> {
        config.validate()?;
        let norm = layer_norm_no_bias(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("embeddings.norm"),
        )?;
        let global_rope = Arc::new(Rotary::new(
            vb.dtype(),
            config,
            config.global_rope_theta,
            vb.device(),
        )?);
        let local_rope = Arc::new(Rotary::new(
            vb.dtype(),
            config,
            config.local_rope_theta,
            vb.device(),
        )?);
        let every = config.global_attn_every_n_layers.max(1);
        let mut layers = Vec::with_capacity(config.num_hidden_layers);
        for id in 0..config.num_hidden_layers {
            let local = id % every != 0;
            let rope = if local {
                local_rope.clone()
            } else {
                global_rope.clone()
            };
            layers.push(Layer::load(
                vb.pp(format!("layers.{id}")),
                config,
                rope,
                local,
                id == 0,
            )?);
        }
        let final_norm = layer_norm_no_bias(
            config.hidden_size,
            config.layer_norm_eps,
            vb.pp("final_norm"),
        )?;
        Ok(Self {
            words: None,
            norm,
            layers,
            final_norm,
            window: config.local_attention / 2,
            dtype: vb.dtype(),
        })
    }

    pub fn dtype(&self) -> DType {
        self.dtype
    }

    pub fn forward(&self, ids: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let Some(words) = &self.words else {
            candle_core::bail!("this model was loaded without its word table")
        };
        self.forward_embedded(&ids.apply(words)?, mask)
    }

    pub fn forward_embedded(&self, embedded: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let (global, local) = attention_masks(mask, self.window, self.dtype)?;
        let mut xs = embedded.to_dtype(self.dtype)?.apply(&self.norm)?;
        for layer in &self.layers {
            xs = layer.forward(&xs, Some(&global), Some(&local), Path::Full)?;
        }
        xs.apply(&self.final_norm)
    }

    pub fn forward_first(&self, embedded: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let (b, seq) = mask.dims2()?;
        let (eb, eseq, _) = embedded.dims3()?;
        if (b, seq) != (eb, eseq) {
            candle_core::bail!("mask is {b} by {seq}, the embedded texts are {eb} by {eseq}")
        }
        let padded = mask
            .to_dtype(DType::F32)?
            .flatten_all()?
            .to_vec1::<f32>()?
            .iter()
            .any(|&m| m <= 0.0);
        let (global, local) = match (padded, seq > self.window + 1) {
            (false, false) => (None, None),
            (false, true) => (
                None,
                Some(attention_masks(mask, self.window, self.dtype)?.1),
            ),
            (true, _) => {
                let (global, local) = attention_masks(mask, self.window, self.dtype)?;
                (Some(global), Some(local))
            }
        };
        let mut xs = embedded.to_dtype(self.dtype)?.apply(&self.norm)?;
        let last = self.layers.len().saturating_sub(1);
        for (i, layer) in self.layers.iter().enumerate() {
            let path = if i == last { Path::First } else { Path::Fast };
            xs = layer.forward(&xs, global.as_ref(), local.as_ref(), path)?;
        }
        xs.apply(&self.final_norm)?.squeeze(1)
    }
}
