use crate::modernbert::{Activation, Config, ModernBert};
use candle_core::{DType, Device, Tensor};
use candle_nn::{VarBuilder, VarMap};
use std::collections::HashMap;
use std::path::Path;

pub fn tiny_config() -> Config {
    Config {
        vocab_size: 64,
        hidden_size: 32,
        num_hidden_layers: 2,
        num_attention_heads: 4,
        intermediate_size: 48,
        max_position_embeddings: 1024,
        layer_norm_eps: 1e-5,
        pad_token_id: 0,
        global_attn_every_n_layers: 2,
        global_rope_theta: 10000.0,
        local_attention: 8,
        local_rope_theta: 10000.0,
        hidden_activation: Activation::Silu,
    }
}

pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(
            seed.wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407),
        )
    }

    pub fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        ((self.0 >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0
    }
}

pub fn shapes(config: &Config) -> Vec<(String, Vec<usize>)> {
    let map = VarMap::new();
    let vb = VarBuilder::from_varmap(&map, DType::F32, &Device::Cpu);
    ModernBert::load(vb, config).expect("load into a VarMap");
    let data = map.data().lock().expect("varmap lock");
    let mut out: Vec<(String, Vec<usize>)> = data
        .iter()
        .map(|(k, v)| (k.clone(), v.as_tensor().dims().to_vec()))
        .collect();
    out.sort();
    out
}

pub fn tiny_weights(config: &Config, seed: u64) -> HashMap<String, Tensor> {
    let mut rng = Lcg::new(seed);
    shapes(config)
        .into_iter()
        .map(|(name, dims)| {
            let n: usize = dims.iter().product();
            let norm = dims.len() == 1;
            let values: Vec<f32> = (0..n)
                .map(|_| {
                    if norm {
                        1.0 + 0.1 * rng.next()
                    } else {
                        0.2 * rng.next()
                    }
                })
                .collect();
            let t = Tensor::from_vec(values, dims, &Device::Cpu).expect("tensor");
            (name, t)
        })
        .collect()
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    dot / (na * nb).max(1e-12)
}

pub const TINY: &str = r#"{
  "version": "1.0",
  "truncation": null,
  "padding": {"strategy": "BatchLongest", "direction": "Right", "pad_to_multiple_of": null, "pad_id": 3, "pad_type_id": 0, "pad_token": "<pad>"},
  "added_tokens": [
    {"id": 0, "content": "<s>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 1, "content": "</s>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 2, "content": "<unk>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true},
    {"id": 3, "content": "<pad>", "single_word": false, "lstrip": false, "rstrip": false, "normalized": false, "special": true}
  ],
  "normalizer": null,
  "pre_tokenizer": {"type": "Whitespace"},
  "post_processor": {
    "type": "TemplateProcessing",
    "single": [{"SpecialToken": {"id": "<s>", "type_id": 0}}, {"Sequence": {"id": "A", "type_id": 0}}, {"SpecialToken": {"id": "</s>", "type_id": 0}}],
    "pair": [{"SpecialToken": {"id": "<s>", "type_id": 0}}, {"Sequence": {"id": "A", "type_id": 0}}, {"SpecialToken": {"id": "</s>", "type_id": 0}}, {"Sequence": {"id": "B", "type_id": 1}}, {"SpecialToken": {"id": "</s>", "type_id": 1}}],
    "special_tokens": {
      "<s>": {"id": "<s>", "ids": [0], "tokens": ["<s>"]},
      "</s>": {"id": "</s>", "ids": [1], "tokens": ["</s>"]}
    }
  },
  "decoder": null,
  "model": {
    "type": "BPE", "dropout": null, "unk_token": "<unk>", "continuing_subword_prefix": null,
    "end_of_word_suffix": null, "fuse_unk": false, "byte_fallback": false, "ignore_merges": false,
    "vocab": {"<s>": 0, "</s>": 1, "<unk>": 2, "<pad>": 3, "a": 4, "b": 5, "c": 6, "ab": 7, "abc": 8},
    "merges": ["a b", "ab c"]
  }
}"#;

pub fn write_tiny_model(dir: &Path, max_positions: usize) {
    let json = format!(
        r#"{{"vocab_size":64,"hidden_size":32,"num_hidden_layers":2,"num_attention_heads":4,
        "intermediate_size":48,"max_position_embeddings":{max_positions},"layer_norm_eps":1e-5,
        "pad_token_id":0,"global_attn_every_n_layers":2,"global_rope_theta":10000.0,
        "local_attention":8,"local_rope_theta":10000.0,"hidden_activation":"silu"}}"#
    );
    std::fs::write(dir.join("config.json"), json).expect("config.json");
    candle_core::safetensors::save(
        &tiny_weights(&tiny_config(), 11),
        dir.join("model.safetensors"),
    )
    .expect("model.safetensors");
    std::fs::write(dir.join("tokenizer.json"), TINY).expect("tokenizer.json");
}
