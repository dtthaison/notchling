use crate::part::{create_part, part_path};
use memmap2::Mmap;
use safetensors::{Dtype, SafeTensors};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;

pub const DIMS: usize = 512;
pub const MAGIC: &[u8; 8] = b"NLST1\0\0\0";

const KEY: &str = "embedding.weight";
const SUBDIR: &str = "0_StaticEmbedding";
const FLAT: &str = "table-f32.safetensors";

struct Hashed<W: Write> {
    inner: W,
    hash: Sha256,
}

impl<W: Write> Write for Hashed<W> {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.hash.update(&buf[..n]);
        Ok(n)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.inner.flush()
    }
}

fn row_scale(row: &[f32]) -> f32 {
    let max = row.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if max == 0.0 || !max.is_finite() {
        0.0
    } else {
        max / 127.0
    }
}

fn row_ints(row: &[f32], scale: f32, out: &mut Vec<u8>) {
    out.clear();
    out.extend(row.iter().map(|x| {
        if scale == 0.0 {
            0
        } else {
            (x / scale).round().clamp(-127.0, 127.0) as i8 as u8
        }
    }));
}

fn row(data: &[u8], index: usize, width: usize, into: &mut Vec<f32>) {
    let start = index * width * 4;
    into.clear();
    into.extend(
        data[start..start + DIMS * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b)),
    );
}

fn write_table(
    out: &mut Hashed<BufWriter<File>>,
    data: &[u8],
    rows: usize,
    width: usize,
) -> Result<(), String> {
    let io = |e: std::io::Error| e.to_string();
    out.write_all(MAGIC).map_err(io)?;
    out.write_all(&(rows as u32).to_le_bytes()).map_err(io)?;
    out.write_all(&(DIMS as u32).to_le_bytes()).map_err(io)?;
    let mut buf = Vec::with_capacity(DIMS);
    let mut scales = Vec::with_capacity(rows);
    for i in 0..rows {
        row(data, i, width, &mut buf);
        scales.push(row_scale(&buf));
    }
    for scale in &scales {
        out.write_all(&scale.to_le_bytes()).map_err(io)?;
    }
    let mut ints = Vec::with_capacity(DIMS);
    for (i, scale) in scales.iter().enumerate() {
        row(data, i, width, &mut buf);
        row_ints(&buf, *scale, &mut ints);
        out.write_all(&ints).map_err(io)?;
    }
    Ok(())
}

pub fn convert(model_dir: &Path, out: &Path) -> Result<String, String> {
    let flat = model_dir.join(FLAT);
    let source = if flat.is_file() {
        flat
    } else {
        model_dir.join(SUBDIR).join("model.safetensors")
    };
    let file = File::open(&source).map_err(|e| format!("{}: {e}", source.display()))?;
    let map = unsafe { Mmap::map(&file) }.map_err(|e| format!("{}: {e}", source.display()))?;
    let tensors =
        SafeTensors::deserialize(&map).map_err(|e| format!("{}: {e}", source.display()))?;
    let table = tensors
        .tensor(KEY)
        .map_err(|e| format!("{}: {KEY}: {e}", source.display()))?;
    if table.dtype() != Dtype::F32 {
        return Err(format!("{KEY} is {:?}, not F32", table.dtype()));
    }
    let [rows, width] = table.shape()[..] else {
        return Err(format!("{KEY} is not a table"));
    };
    if rows == 0 || width < DIMS {
        return Err(format!(
            "{KEY} is {rows} by {width}, need {DIMS} dimensions"
        ));
    }
    if u32::try_from(rows).is_err() {
        return Err(format!("{KEY} has too many rows"));
    }
    let part = part_path(out);
    let file = create_part(&part)?;
    let mut sink = Hashed {
        inner: BufWriter::new(file),
        hash: Sha256::new(),
    };
    let finished = write_table(&mut sink, table.data(), rows, width)
        .and_then(|()| sink.flush().map_err(|e| e.to_string()))
        .and_then(|()| sink.inner.get_ref().sync_all().map_err(|e| e.to_string()))
        .and_then(|()| std::fs::rename(&part, out).map_err(|e| format!("{}: {e}", out.display())));
    if let Err(e) = finished {
        let _ = std::fs::remove_file(&part);
        return Err(e);
    }
    Ok(format!("{:x}", sink.hash.finalize()))
}
