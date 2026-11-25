use candle_core::{DType, Device, Result, Tensor, D};
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

    fn forward(&self, xs: &Tensor, mask: &Tensor) -> Result<Tensor> {
        let (b, seq, d) = xs.dims3()?;
        let qkv = xs
            .apply(&self.qkv)?
            .reshape((b, seq, 3, self.heads, self.head_size))?
            .permute((2, 0, 3, 1, 4))?;
        let q = qkv.get(0)?;
        let k = qkv.get(1)?;
        let v = qkv.get(2)?;
        let (q, k) = self.rotary.apply(&q, &k)?;
        let q = (q * (self.head_size as f64).powf(-0.5))?;
        let att = matmul(&q, &k.transpose(D::Minus2, D::Minus1)?.contiguous()?)?;
        let att = softmax_last_dim(&att.broadcast_add(mask)?)?;
        let xs = matmul(&att, &v.contiguous()?)?;
        xs.transpose(1, 2)?.reshape((b, seq, d))?.apply(&self.proj)
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

    fn forward(&self, xs: &Tensor, global: &Tensor, local: &Tensor) -> Result<Tensor> {
        let normed = match &self.attn_norm {
            Some(norm) => xs.apply(norm)?,
            None => xs.clone(),
        };
        let mask = if self.local { local } else { global };
        let xs = (self.attn.forward(&normed, mask)? + xs)?;
        let mlp = xs.apply(&self.mlp_norm)?.apply(&self.mlp)?;
        xs + mlp
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
            xs = layer.forward(&xs, &global, &local)?;
        }
        xs.apply(&self.final_norm)
    }
}
