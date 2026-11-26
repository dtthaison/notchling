use crate::modernbert::{Activation, Config, ModernBert};
use crate::testing::{cosine, shapes, tiny_config, tiny_weights};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;

fn cls(config: &Config, dtype: DType, ids: &[[u32; 6]; 2], mask: &[[u32; 6]; 2]) -> Vec<Vec<f32>> {
    let vb = VarBuilder::from_tensors(tiny_weights(config, 7), dtype, &Device::Cpu);
    let model = ModernBert::load(vb, config).expect("load");
    let ids = Tensor::new(ids, &Device::Cpu).expect("ids");
    let mask = Tensor::new(mask, &Device::Cpu).expect("mask");
    let out = model.forward(&ids, &mask).expect("forward");
    out.narrow(1, 0, 1)
        .and_then(|t| t.squeeze(1))
        .and_then(|t| t.to_dtype(DType::F32))
        .and_then(|t| t.to_vec2())
        .expect("cls")
}

const IDS: [[u32; 6]; 2] = [[1, 9, 17, 33, 2, 0], [1, 40, 5, 2, 0, 0]];
const MASK: [[u32; 6]; 2] = [[1, 1, 1, 1, 1, 0], [1, 1, 1, 1, 0, 0]];

#[test]
fn silu_config_is_honored() {
    let silu = tiny_config();
    let gelu = Config {
        hidden_activation: Activation::Gelu,
        ..tiny_config()
    };
    let a = cls(&silu, DType::F32, &IDS, &MASK);
    let b = cls(&gelu, DType::F32, &IDS, &MASK);
    assert!(
        cosine(&a[0], &b[0]) < 0.9999,
        "the activation changed nothing"
    );
}

#[test]
fn silu_is_read_from_config_json() {
    let json = r#"{"vocab_size":64,"hidden_size":32,"num_hidden_layers":2,"num_attention_heads":4,
        "intermediate_size":48,"max_position_embeddings":1024,"layer_norm_eps":1e-5,"pad_token_id":0,
        "global_attn_every_n_layers":2,"global_rope_theta":10000.0,"local_attention":8,
        "local_rope_theta":10000.0,"hidden_activation":"silu"}"#;
    let config: Config = serde_json::from_str(json).expect("config");
    assert_eq!(config.hidden_activation, Activation::Silu);
    let plain: Config =
        serde_json::from_str(&json.replace(r#","hidden_activation":"silu""#, "")).expect("config");
    assert_eq!(plain.hidden_activation, Activation::Gelu);
}

#[test]
fn mask_follows_the_weights_dtype() {
    let config = tiny_config();
    let full = cls(&config, DType::F32, &IDS, &MASK);
    for dtype in [DType::F16, DType::BF16] {
        let half = cls(&config, dtype, &IDS, &MASK);
        for (a, b) in full.iter().zip(&half) {
            let c = cosine(a, b);
            assert!(c >= 0.99, "{dtype:?} cosine {c}");
        }
    }
}

#[test]
fn keys_have_no_model_prefix() {
    let names: Vec<String> = shapes(&tiny_config()).into_iter().map(|(n, _)| n).collect();
    assert!(names.iter().all(|n| !n.starts_with("model.")), "{names:?}");
    assert!(names.contains(&"embeddings.tok_embeddings.weight".to_string()));
    assert!(names.contains(&"layers.0.attn.Wqkv.weight".to_string()));
    assert!(names.contains(&"final_norm.weight".to_string()));
    assert!(!names.contains(&"layers.0.attn_norm.weight".to_string()));
    let prefixed = tiny_weights(&tiny_config(), 7)
        .into_iter()
        .map(|(k, v)| (format!("model.{k}"), v))
        .collect();
    let vb = VarBuilder::from_tensors(prefixed, DType::F32, &Device::Cpu);
    assert!(ModernBert::load(vb, &tiny_config()).is_err());
}

#[test]
fn a_bad_config_is_refused_not_a_panic() {
    let bad = [
        Config {
            num_attention_heads: 0,
            ..tiny_config()
        },
        Config {
            hidden_size: 0,
            ..tiny_config()
        },
        Config {
            hidden_size: 36,
            ..tiny_config()
        },
        Config {
            hidden_size: 30,
            ..tiny_config()
        },
        Config {
            max_position_embeddings: 0,
            ..tiny_config()
        },
        Config {
            num_hidden_layers: 0,
            ..tiny_config()
        },
    ];
    for config in &bad {
        let vb = VarBuilder::zeros(DType::F32, &Device::Cpu);
        let loaded = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            ModernBert::load_encoder(vb, config)
        }));
        match loaded {
            Ok(result) => assert!(result.is_err(), "accepted {config:?}"),
            Err(_) => panic!("panicked on {config:?}"),
        }
    }
}

#[test]
fn the_first_row_path_matches_the_full_forward_on_a_padded_batch() {
    for layers in [2, 3] {
        let mut config = tiny_config();
        config.num_hidden_layers = layers;
        let vb = VarBuilder::from_tensors(tiny_weights(&config, 5), DType::F32, &Device::Cpu);
        let model = ModernBert::load_encoder(vb, &config).expect("encoder");
        let mut rng = crate::testing::Lcg::new(9);
        let (b, seq) = (3, 20);
        let values: Vec<f32> = (0..b * seq * config.hidden_size)
            .map(|_| rng.next())
            .collect();
        let embedded =
            Tensor::from_vec(values, (b, seq, config.hidden_size), &Device::Cpu).expect("embedded");
        let lens = [20u32, 7, 13];
        let mask: Vec<u32> = lens
            .iter()
            .flat_map(|&n| (0..seq as u32).map(move |i| u32::from(i < n)))
            .collect();
        let mask = Tensor::from_vec(mask, (b, seq), &Device::Cpu).expect("mask");
        let full: Vec<Vec<f32>> = model
            .forward_embedded(&embedded, &mask)
            .expect("full")
            .narrow(1, 0, 1)
            .expect("narrow")
            .squeeze(1)
            .expect("squeeze")
            .to_vec2()
            .expect("rows");
        let first: Vec<Vec<f32>> = model
            .forward_first(&embedded, &mask)
            .expect("first")
            .to_vec2()
            .expect("rows");
        for (a, c) in full.iter().zip(&first) {
            let cos = cosine(a, c);
            assert!(cos >= 0.9999, "{layers} layers: cosine {cos}");
        }
    }
}

fn same_or_both_nan(a: &[f32], b: &[f32]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x.is_nan() && y.is_nan()) || (x - y).abs() < 1e-6)
}

#[test]
fn the_fast_softmax_matches_candle_including_an_all_masked_row() {
    let mut rng = crate::testing::Lcg::new(3);
    let mut values: Vec<f32> = (0..2 * 3 * 4).map(|_| rng.next()).collect();
    values[4..8].fill(f32::NEG_INFINITY);
    let att = Tensor::from_vec(values, (1, 2, 3, 4), &Device::Cpu).expect("att");
    let floor = crate::modernbert::mask_floor(DType::F32);
    let row = Tensor::from_vec(vec![0f32, 0.0, floor, 0.0], (1, 1, 1, 4), &Device::Cpu)
        .expect("row mask");
    let square: Vec<f32> = (0..12)
        .map(|i| if i % 5 == 0 { floor } else { 0.0 })
        .collect();
    let square = Tensor::from_vec(square, (1, 1, 3, 4), &Device::Cpu).expect("square mask");
    let flat = |t: Tensor| {
        t.flatten_all()
            .and_then(|t| t.to_vec1::<f32>())
            .expect("flat")
    };
    let ours = flat(crate::modernbert::masked_softmax(&att, None).expect("ours"));
    let theirs = flat(candle_nn::ops::softmax_last_dim(&att).expect("candle"));
    assert!(same_or_both_nan(&ours, &theirs), "{ours:?} {theirs:?}");
    for mask in [row, square] {
        let ours = flat(crate::modernbert::masked_softmax(&att, Some(&mask)).expect("ours"));
        let added = att.broadcast_add(&mask).expect("add");
        let theirs = flat(candle_nn::ops::softmax_last_dim(&added).expect("candle"));
        assert!(same_or_both_nan(&ours, &theirs), "{ours:?} {theirs:?}");
    }
}

#[test]
fn a_softmax_over_no_columns_is_empty_not_a_panic() {
    let att = Tensor::zeros((1, 1, 1, 0), DType::F32, &Device::Cpu).expect("att");
    let mask = Tensor::zeros((1, 1, 1, 0), DType::F32, &Device::Cpu).expect("mask");
    let out = crate::modernbert::masked_softmax(&att, Some(&mask)).expect("masked");
    assert_eq!(out.elem_count(), 0);
    let out = crate::modernbert::masked_softmax(&att, None).expect("plain");
    assert_eq!(out.elem_count(), 0);
}

#[test]
fn the_first_row_path_refuses_a_mask_that_does_not_fit() {
    let config = tiny_config();
    let vb = VarBuilder::from_tensors(tiny_weights(&config, 5), DType::F32, &Device::Cpu);
    let model = ModernBert::load_encoder(vb, &config).expect("encoder");
    let embedded =
        Tensor::zeros((1, 5, config.hidden_size), DType::F32, &Device::Cpu).expect("embedded");
    let short = Tensor::ones((1, 3), DType::U32, &Device::Cpu).expect("mask");
    assert!(model.forward_first(&embedded, &short).is_err());
    let two = Tensor::ones((2, 5), DType::U32, &Device::Cpu).expect("mask");
    assert!(model.forward_first(&embedded, &two).is_err());
    let fits = Tensor::ones((1, 5), DType::U32, &Device::Cpu).expect("mask");
    assert!(model.forward_first(&embedded, &fits).is_ok());
}
