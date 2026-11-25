mod qwen3;
mod modernbert;
use anyhow::{anyhow, Result};
use candle_core::{DType, Device, Tensor};
use candle_nn::VarBuilder;
use candle_transformers::models::bert::{BertModel, Config as BertConfig};
use memmap2::Mmap;
use std::time::Instant;
use tokenizers::{PaddingParams, Tokenizer, TruncationParams};

const DATA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../data");

fn rss_mb() -> f64 {
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().parse::<f64>().unwrap_or(0.0) / 1024.0
}

fn peak_mb() -> f64 {
    let mut ru: libc::rusage = unsafe { std::mem::zeroed() };
    unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut ru) };
    ru.ru_maxrss as f64 / 1e6
}

trait Embed {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn tokens(&self, text: &str) -> usize;
}

fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt().max(1e-12);
    v.iter_mut().for_each(|x| *x /= n);
}

struct Static {
    map: Mmap,
    off: usize,
    rows: usize,
    dims: usize,
    keep: usize,
    tok: Tokenizer,
    quant: Option<(Vec<i8>, Vec<f32>)>,
}

impl Static {
    fn load(file: &str, tensor: &str, tok: &str, keep: usize, int8: bool) -> Result<Self> {
        let f = std::fs::File::open(file)?;
        let map = unsafe { Mmap::map(&f)? };
        let (off, rows, dims) = {
            let st = safetensors::SafeTensors::deserialize(&map)?;
            let v = st.tensor(tensor)?;
            let shape = v.shape().to_vec();
            (v.data().as_ptr() as usize - map.as_ptr() as usize, shape[0], shape[1])
        };
        let tok = Tokenizer::from_file(tok).map_err(|e| anyhow!("{e}"))?;
        let keep = if keep == 0 { dims } else { keep };
        let mut s = Static { map, off, rows, dims, keep, tok, quant: None };
        if int8 {
            let mut q = vec![0i8; rows * keep];
            let mut scales = vec![0f32; rows];
            for r in 0..rows {
                let row: Vec<f32> = (0..keep).map(|c| s.at(r, c)).collect();
                let m = row.iter().fold(0f32, |a, x| a.max(x.abs())).max(1e-12);
                scales[r] = m / 127.0;
                for c in 0..keep {
                    q[r * keep + c] = (row[c] / scales[r]).round() as i8;
                }
            }
            s.quant = Some((q, scales));
        }
        Ok(s)
    }
    fn at(&self, r: usize, c: usize) -> f32 {
        let i = self.off + (r * self.dims + c) * 4;
        f32::from_le_bytes(self.map[i..i + 4].try_into().unwrap())
    }
}

impl Embed for Static {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::with_capacity(texts.len());
        for t in texts {
            let enc = self.tok.encode(t.as_str(), false).map_err(|e| anyhow!("{e}"))?;
            let mut v = vec![0f32; self.keep];
            let ids = enc.get_ids();
            let mut n = 0;
            for &id in ids.iter().take(512) {
                let r = id as usize;
                if r >= self.rows {
                    continue;
                }
                n += 1;
                match &self.quant {
                    Some((q, s)) => {
                        for c in 0..self.keep {
                            v[c] += q[r * self.keep + c] as f32 * s[r];
                        }
                    }
                    None => {
                        for c in 0..self.keep {
                            v[c] += self.at(r, c);
                        }
                    }
                }
            }
            if n > 0 {
                v.iter_mut().for_each(|x| *x /= n as f32);
            }
            normalize(&mut v);
            out.push(v);
        }
        Ok(out)
    }
    fn tokens(&self, text: &str) -> usize {
        self.tok.encode(text, false).map(|e| e.get_ids().len()).unwrap_or(0)
    }
}

struct Bert {
    model: BertModel,
    tok: Tokenizer,
    dev: Device,
}

impl Bert {
    fn load(dir: &str, dev: Device, dtype: DType) -> Result<Self> {
        let cfg: BertConfig = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/config.json"))?)?;
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[format!("{dir}/model.safetensors")], dtype, &dev)? };
        let model = BertModel::load(vb, &cfg)?;
        let mut tok = Tokenizer::from_file(format!("{dir}/tokenizer.json")).map_err(|e| anyhow!("{e}"))?;
        tok.with_padding(Some(PaddingParams::default()));
        tok.with_truncation(Some(TruncationParams { max_length: 512, ..Default::default() })).map_err(|e| anyhow!("{e}"))?;
        Ok(Bert { model, tok, dev })
    }
}

impl Embed for Bert {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let enc = self.tok.encode_batch(texts.to_vec(), true).map_err(|e| anyhow!("{e}"))?;
        let ids: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_ids(), &self.dev)).collect::<candle_core::Result<_>>()?;
        let mask: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_attention_mask(), &self.dev)).collect::<candle_core::Result<_>>()?;
        let ids = Tensor::stack(&ids, 0)?;
        let mask = Tensor::stack(&mask, 0)?;
        let types = ids.zeros_like()?;
        let h = self.model.forward(&ids, &types, Some(&mask))?.to_dtype(DType::F32)?;
        let m = mask.to_dtype(DType::F32)?.unsqueeze(2)?;
        let sum = h.broadcast_mul(&m)?.sum(1)?;
        let cnt = m.sum(1)?;
        let mean = sum.broadcast_div(&cnt)?;
        let mut out: Vec<Vec<f32>> = mean.to_vec2()?;
        out.iter_mut().for_each(|v| normalize(v));
        Ok(out)
    }
    fn tokens(&self, text: &str) -> usize {
        self.tok.encode(text, true).map(|e| e.get_ids().len()).unwrap_or(0)
    }
}

struct Xlmr {
    model: candle_transformers::models::xlm_roberta::XLMRobertaModel,
    tok: Tokenizer,
    dev: Device,
}

impl Xlmr {
    fn load(dir: &str, dev: Device, dtype: DType) -> Result<Self> {
        let cfg: candle_transformers::models::xlm_roberta::Config = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/config.json"))?)?;
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[format!("{dir}/model.safetensors")], dtype, &dev)? };
        let model = candle_transformers::models::xlm_roberta::XLMRobertaModel::new(&cfg, vb)?;
        let mut tok = Tokenizer::from_file(format!("{dir}/tokenizer.json")).map_err(|e| anyhow!("{e}"))?;
        tok.with_padding(Some(PaddingParams { pad_id: 1, pad_token: "<pad>".into(), ..Default::default() }));
        tok.with_truncation(Some(TruncationParams { max_length: 512, ..Default::default() })).map_err(|e| anyhow!("{e}"))?;
        Ok(Xlmr { model, tok, dev })
    }
}

impl Embed for Xlmr {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let enc = self.tok.encode_batch(texts.to_vec(), true).map_err(|e| anyhow!("{e}"))?;
        let ids: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_ids(), &self.dev)).collect::<candle_core::Result<_>>()?;
        let mask: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_attention_mask(), &self.dev)).collect::<candle_core::Result<_>>()?;
        let ids = Tensor::stack(&ids, 0)?;
        let mask = Tensor::stack(&mask, 0)?;
        let types = ids.zeros_like()?;
        let h = self.model.forward(&ids, &mask, &types, None, None, None)?;
        let cls = h.narrow(1, 0, 1)?.squeeze(1)?.to_dtype(DType::F32)?;
        let mut out: Vec<Vec<f32>> = cls.to_vec2()?;
        out.iter_mut().for_each(|v| normalize(v));
        Ok(out)
    }
    fn tokens(&self, text: &str) -> usize {
        self.tok.encode(text, true).map(|e| e.get_ids().len()).unwrap_or(0)
    }
}

struct Modern {
    model: modernbert::ModernBert,
    tok: Tokenizer,
    dev: Device,
}

impl Modern {
    fn load(dir: &str, dev: Device, dtype: DType) -> Result<Self> {
        let cfg: modernbert::Config = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/config.json"))?)?;
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[format!("{dir}/model.safetensors")], dtype, &dev)? };
        let model = modernbert::ModernBert::load(vb, &cfg)?;
        let mut tok = Tokenizer::from_file(format!("{dir}/tokenizer.json")).map_err(|e| anyhow!("{e}"))?;
        let pad = tok.id_to_token(cfg.pad_token_id).unwrap_or_default();
        tok.with_padding(Some(PaddingParams { pad_id: cfg.pad_token_id, pad_token: pad, ..Default::default() }));
        tok.with_truncation(Some(TruncationParams { max_length: 1024, ..Default::default() })).map_err(|e| anyhow!("{e}"))?;
        Ok(Modern { model, tok, dev })
    }
}

impl Embed for Modern {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let enc = self.tok.encode_batch(texts.to_vec(), true).map_err(|e| anyhow!("{e}"))?;
        let ids: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_ids(), &self.dev)).collect::<candle_core::Result<_>>()?;
        let mask: Vec<Tensor> = enc.iter().map(|e| Tensor::new(e.get_attention_mask(), &self.dev)).collect::<candle_core::Result<_>>()?;
        let ids = Tensor::stack(&ids, 0)?;
        let mask = Tensor::stack(&mask, 0)?;
        let h = self.model.forward(&ids, &mask)?;
        let cls = h.narrow(1, 0, 1)?.squeeze(1)?.to_dtype(DType::F32)?;
        let mut out: Vec<Vec<f32>> = cls.to_vec2()?;
        out.iter_mut().for_each(|v| normalize(v));
        Ok(out)
    }
    fn tokens(&self, text: &str) -> usize {
        self.tok.encode(text, true).map(|e| e.get_ids().len()).unwrap_or(0)
    }
}

struct Qwen {
    model: qwen3::Model,
    tok: Tokenizer,
    dev: Device,
    dims: usize,
}

impl Qwen {
    fn load(dir: &str, dev: Device, dtype: DType, dims: usize) -> Result<Self> {
        let cfg: qwen3::Config = serde_json::from_str(&std::fs::read_to_string(format!("{dir}/config.json"))?)?;
        let vb = unsafe { VarBuilder::from_mmaped_safetensors(&[format!("{dir}/model.safetensors")], dtype, &dev)? };
        let model = qwen3::Model::new(&cfg, vb)?;
        let tok = Tokenizer::from_file(format!("{dir}/tokenizer.json")).map_err(|e| anyhow!("{e}"))?;
        Ok(Qwen { model, tok, dev, dims })
    }
}

impl Embed for Qwen {
    fn embed(&mut self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        let mut out = Vec::new();
        for t in texts {
            let enc = self.tok.encode(format!("{t}<|endoftext|>"), false).map_err(|e| anyhow!("{e}"))?;
            let ids: Vec<u32> = enc.get_ids().iter().copied().take(1024).collect();
            let n = ids.len();
            let x = Tensor::new(ids.as_slice(), &self.dev)?.unsqueeze(0)?;
            self.model.clear_kv_cache();
            let h = self.model.forward(&x, 0)?;
            let last = h.narrow(1, n - 1, 1)?.squeeze(1)?.squeeze(0)?.to_dtype(DType::F32)?;
            let mut v: Vec<f32> = last.to_vec1()?;
            v.truncate(self.dims);
            normalize(&mut v);
            out.push(v);
        }
        Ok(out)
    }
    fn tokens(&self, text: &str) -> usize {
        self.tok.encode(text, false).map(|e| e.get_ids().len() + 1).unwrap_or(0)
    }
}

fn eval(e: &mut dyn Embed, qp: &str, dp: &str) -> Result<()> {
    for set in ["settings", "chat"] {
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{DATA}/{set}.json"))?)?;
        let docs = j["docs"].as_object().unwrap();
        let ids: Vec<&String> = docs.keys().collect();
        let texts: Vec<String> = docs.values().map(|v| format!("{dp}{}", v.as_str().unwrap())).collect();
        let mut dv = Vec::new();
        for c in texts.chunks(8) {
            dv.extend(e.embed(c)?);
        }
        let qs = j["queries"].as_array().unwrap();
        let (mut r1, mut r5) = (0.0, 0.0);
        for q in qs {
            let text = format!("{qp}{}", q[0].as_str().unwrap());
            let qv = &e.embed(&[text])?[0];
            let gold: Vec<&str> = q[1].as_array().unwrap().iter().map(|x| x.as_str().unwrap()).collect();
            let mut sc: Vec<(f32, usize)> = dv.iter().enumerate().map(|(i, d)| (d.iter().zip(qv).map(|(a, b)| a * b).sum(), i)).collect();
            sc.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap());
            if gold.contains(&ids[sc[0].1].as_str()) {
                r1 += 1.0;
            }
            if sc.iter().take(5).any(|(_, i)| gold.contains(&ids[*i].as_str())) {
                r5 += 1.0;
            }
        }
        let n = qs.len() as f64;
        println!("quality {set}: R@1 {:.3} R@5 {:.3} (n={})", r1 / n, r5 / n, qs.len());
    }
    Ok(())
}

fn queries() -> Result<Vec<String>> {
    let mut out = Vec::new();
    for set in ["settings", "chat"] {
        let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{DATA}/{set}.json"))?)?;
        for q in j["queries"].as_array().unwrap() {
            out.push(q[0].as_str().unwrap().to_string());
        }
    }
    Ok(out)
}

fn chunks(n: usize, chars: usize) -> Result<Vec<String>> {
    let j: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(format!("{DATA}/chat.json"))?)?;
    let all: Vec<String> = j["docs"].as_object().unwrap().values().map(|v| v.as_str().unwrap().to_string()).collect();
    let mut out = Vec::new();
    let mut i = 0;
    while out.len() < n {
        let mut s = String::new();
        while s.chars().count() < chars {
            s.push_str(&all[i % all.len()]);
            s.push('\n');
            i += 1;
        }
        out.push(s.chars().take(chars).collect());
    }
    Ok(out)
}

fn pct(v: &mut [f64], p: f64) -> f64 {
    v.sort_by(|a, b| a.partial_cmp(b).unwrap());
    v[((v.len() - 1) as f64 * p).round() as usize]
}

fn bench(e: &mut dyn Embed, qp: &str, dp: &str, nchunks: usize) -> Result<()> {
    let qs = queries()?;
    for q in qs.iter().take(3) {
        e.embed(&[format!("{qp}{q}")])?;
    }
    let mut lat = Vec::new();
    for _ in 0..3 {
        for q in &qs {
            let t = Instant::now();
            e.embed(&[format!("{qp}{q}")])?;
            lat.push(t.elapsed().as_secs_f64() * 1000.0);
        }
    }
    println!("query ms: p50 {:.2} p95 {:.2} (n={})", pct(&mut lat.clone(), 0.5), pct(&mut lat, 0.95), qs.len() * 3);
    let cs: Vec<String> = chunks(nchunks, 1200)?.into_iter().map(|c| format!("{dp}{c}")).collect();
    let toks: usize = cs.iter().map(|c| e.tokens(c)).sum();
    let t = Instant::now();
    for c in cs.chunks(8) {
        e.embed(c)?;
    }
    let s = t.elapsed().as_secs_f64();
    println!("index: {} chunks of 1200 chars ({:.0} tokens avg) in {:.2}s = {:.1} chunks/s", cs.len(), toks as f64 / cs.len() as f64, s, cs.len() as f64 / s);
    Ok(())
}

fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let kind = a[1].as_str();
    if kind == "scan" {
        let n: usize = a[2].parse()?;
        let d: usize = a[3].parse()?;
        let mut seed = 12345u64;
        let mut rnd = || { seed ^= seed << 13; seed ^= seed >> 7; seed ^= seed << 17; seed };
        let q8: Vec<i8> = (0..n * d).map(|_| (rnd() % 255) as i8).collect();
        let f: Vec<f32> = q8.iter().map(|&x| x as f32 / 127.0).collect();
        let qq: Vec<i8> = (0..d).map(|_| (rnd() % 255) as i8).collect();
        let qf: Vec<f32> = qq.iter().map(|&x| x as f32 / 127.0).collect();
        for _ in 0..3 {
            let t = Instant::now();
            let mut best = (i32::MIN, 0);
            for (i, row) in q8.chunks_exact(d).enumerate() {
                let s: i32 = row.iter().zip(&qq).map(|(&x, &y)| x as i32 * y as i32).sum();
                if s > best.0 { best = (s, i); }
            }
            let t8 = t.elapsed().as_secs_f64() * 1000.0;
            let t = Instant::now();
            let mut bf = (f32::MIN, 0);
            for (i, row) in f.chunks_exact(d).enumerate() {
                let s: f32 = row.iter().zip(&qf).map(|(x, y)| x * y).sum();
                if s > bf.0 { bf = (s, i); }
            }
            let tf = t.elapsed().as_secs_f64() * 1000.0;
            println!("scan {n} x {d}: int8 {t8:.2} ms, f32 {tf:.2} ms ({} {})", best.1, bf.1);
        }
        return Ok(());
    }
    if kind == "tok" {
        let r0 = rss_mb();
        let t = Instant::now();
        let tok = Tokenizer::from_file(&a[2]).map_err(|e| anyhow!("{e}"))?;
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        let n = tok.encode("Đăng nhập không được", false).map_err(|e| anyhow!("{e}"))?.get_ids().len();
        println!("tokenizer load {:.0} ms, rss before {:.0} MB after {:.0} MB, peak {:.0} MB ({} ids)", ms, r0, rss_mb(), peak_mb(), n);
        return Ok(());
    }
    let t = Instant::now();
    let (mut e, qp, dp): (Box<dyn Embed>, String, String) = match kind {
        "static" => {
            let keep = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(0);
            let int8 = a.get(6).map(|s| s == "int8").unwrap_or(false);
            (Box::new(Static::load(&a[2], &a[3], &a[4], keep, int8)?), String::new(), String::new())
        }
        "bert" => {
            let dev = match a.get(3).map(String::as_str) {
                Some("metal") => Device::new_metal(0)?,
                _ => Device::Cpu,
            };
            let dt = match a.get(4).map(String::as_str) {
                Some("f16") => DType::F16,
                Some("bf16") => DType::BF16,
                _ => DType::F32,
            };
            (Box::new(Bert::load(&a[2], dev, dt)?), "query: ".into(), "passage: ".into())
        }
        "xlmr" => {
            let dev = match a.get(3).map(String::as_str) {
                Some("metal") => Device::new_metal(0)?,
                _ => Device::Cpu,
            };
            let dt = match a.get(4).map(String::as_str) {
                Some("f16") => DType::F16,
                Some("bf16") => DType::BF16,
                _ => DType::F32,
            };
            (Box::new(Xlmr::load(&a[2], dev, dt)?), String::new(), String::new())
        }
        "modern" => {
            let dev = match a.get(3).map(String::as_str) {
                Some("metal") => Device::new_metal(0)?,
                _ => Device::Cpu,
            };
            let dt = match a.get(4).map(String::as_str) {
                Some("f16") => DType::F16,
                Some("bf16") => DType::BF16,
                _ => DType::F32,
            };
            (Box::new(Modern::load(&a[2], dev, dt)?), String::new(), String::new())
        }
        "qwen" => {
            let dev = match a.get(3).map(String::as_str) {
                Some("metal") => Device::new_metal(0)?,
                _ => Device::Cpu,
            };
            let dt = match a.get(4).map(String::as_str) {
                Some("f16") => DType::F16,
                Some("bf16") => DType::BF16,
                _ => DType::F32,
            };
            let dims = a.get(5).map(|s| s.parse().unwrap()).unwrap_or(1024);
            let q = "Instruct: Given a web search query, retrieve relevant passages that answer the query\nQuery:".to_string();
            (Box::new(Qwen::load(&a[2], dev, dt, dims)?), q, String::new())
        }
        _ => return Err(anyhow!("kind")),
    };
    let load = t.elapsed().as_secs_f64() * 1000.0;
    let r0 = rss_mb();
    e.embed(&[format!("{qp}hello")])?;
    println!("load ms {:.0}; rss after load {:.0} MB, after first embed {:.0} MB", load, r0, rss_mb());
    eval(e.as_mut(), &qp, &dp)?;
    println!("rss after eval {:.0} MB", rss_mb());
    let n = std::env::var("NCHUNKS").ok().and_then(|s| s.parse().ok()).unwrap_or(64);
    bench(e.as_mut(), &qp, &dp, n)?;
    println!("rss end {:.0} MB, peak {:.0} MB", rss_mb(), peak_mb());
    Ok(())
}
