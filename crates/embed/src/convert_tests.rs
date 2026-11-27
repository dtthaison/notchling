use crate::convert::{convert, DIMS};
use crate::testing::{cosine, Lcg, TINY};
use candle_core::{Device, Tensor};
use notchling_engine::search::embed::{quantize, Embedder, StaticEmbedder};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const ROWS: usize = 10;
const WIDE: usize = 1024;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-convert-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("0_StaticEmbedding")).expect("scratch dir");
    dir
}

fn rows(width: usize) -> Vec<Vec<f32>> {
    let mut rng = Lcg::new(5);
    (0..ROWS)
        .map(|_| (0..width).map(|_| rng.next()).collect())
        .collect()
}

fn write_model(dir: &Path, key: &str, data: &[Vec<f32>]) {
    let flat: Vec<f32> = data.iter().flatten().copied().collect();
    let tensor = Tensor::from_vec(flat, (data.len(), data[0].len()), &Device::Cpu).expect("tensor");
    let map: HashMap<String, Tensor> = HashMap::from([(key.to_string(), tensor)]);
    candle_core::safetensors::save(&map, dir.join("0_StaticEmbedding/model.safetensors"))
        .expect("save");
}

fn le(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().expect("four bytes"))
}

fn dequantized(row: &[f32]) -> Vec<f32> {
    let q = quantize(&row[..DIMS]);
    q.vec.iter().map(|&v| f32::from(v) * q.scale).collect()
}

#[test]
fn round_trips_a_tiny_table() {
    let dir = scratch("round");
    let data = rows(WIDE);
    write_model(&dir, "embedding.weight", &data);
    let out = dir.join("table.bin");
    let hash = convert(&dir, &out).expect("convert");

    let bytes = std::fs::read(&out).expect("table");
    assert_eq!(&bytes[..8], b"NLST1\0\0\0");
    assert_eq!(le(&bytes[8..]) as usize, ROWS);
    assert_eq!(le(&bytes[12..]) as usize, DIMS);
    assert_eq!(bytes.len(), 16 + ROWS * 4 + ROWS * DIMS);
    assert_eq!(hash, format!("{:x}", Sha256::digest(&bytes)));
    assert!(!dir.join("table.bin.part").exists());

    let scale0 = f32::from_le_bytes(bytes[16..20].try_into().expect("scale"));
    assert_eq!(scale0, quantize(&data[0][..DIMS]).scale);

    let tokenizer = dir.join("tokenizer.json");
    std::fs::write(&tokenizer, TINY).expect("tokenizer");
    let e = StaticEmbedder::open(&out, &tokenizer).expect("open");
    assert_eq!(e.dims(), DIMS);
    let v = e.embed(&["ab c"]).expect("embed").remove(0);
    let a = dequantized(&data[7]);
    let b = dequantized(&data[6]);
    let mean: Vec<f32> = a.iter().zip(&b).map(|(x, y)| (x + y) / 2.0).collect();
    assert!(cosine(&v, &mean) > 0.9999);
}

#[test]
fn a_model_that_cannot_be_read_is_refused() {
    let dir = scratch("refused");
    let out = dir.join("table.bin");
    assert!(convert(&dir, &out).is_err());

    write_model(&dir, "embedding.weight", &rows(256));
    assert!(convert(&dir, &out).is_err());

    write_model(&dir, "other.weight", &rows(WIDE));
    assert!(convert(&dir, &out).is_err());
    assert!(!out.exists());
    assert!(!dir.join("table.bin.part").exists());
}

#[cfg(unix)]
#[test]
fn the_part_file_and_the_target_are_not_followed() {
    let dir = scratch("links");
    write_model(&dir, "embedding.weight", &rows(WIDE));
    let victim = dir.join("victim");
    std::fs::write(&victim, b"keep").expect("victim");
    let out = dir.join("table.bin");
    std::os::unix::fs::symlink(&victim, dir.join("table.bin.part")).expect("part link");
    convert(&dir, &out).expect("convert");
    assert_eq!(std::fs::read(&victim).expect("victim"), b"keep");
    assert!(std::fs::read(&out).expect("table").starts_with(b"NLST1"));

    std::fs::remove_file(&out).expect("remove");
    std::os::unix::fs::symlink(&victim, &out).expect("target link");
    convert(&dir, &out).expect("convert over a link");
    assert_eq!(std::fs::read(&victim).expect("victim"), b"keep");
    assert!(!std::fs::symlink_metadata(&out)
        .expect("target")
        .file_type()
        .is_symlink());
}

#[test]
fn the_downloaded_flat_table_is_read_when_it_is_there() {
    let dir = scratch("flat");
    write_model(&dir, "embedding.weight", &rows(WIDE));
    std::fs::rename(
        dir.join("0_StaticEmbedding/model.safetensors"),
        dir.join("table-f32.safetensors"),
    )
    .expect("move");
    std::fs::remove_dir(dir.join("0_StaticEmbedding")).expect("rmdir");
    let out = dir.join("table-512i8.bin");
    let hash = convert(&dir, &out).expect("convert");
    assert_eq!(
        hash,
        format!("{:x}", Sha256::digest(std::fs::read(&out).expect("table")))
    );
}
