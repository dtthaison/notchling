use crate::search::embed::{quantize, Q8};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VecFormat {
    F16,
    Q8,
}

impl VecFormat {
    pub(super) fn tag(self) -> &'static str {
        match self {
            VecFormat::F16 => "f16",
            VecFormat::Q8 => "q8",
        }
    }

    pub(super) fn from_tag(tag: &str) -> Option<VecFormat> {
        match tag {
            "f16" => Some(VecFormat::F16),
            "q8" => Some(VecFormat::Q8),
            _ => None,
        }
    }

    pub fn encode(self, v: &[f32]) -> Vec<u8> {
        match self {
            VecFormat::F16 => v
                .iter()
                .flat_map(|x| f32_to_f16(*x).to_le_bytes())
                .collect(),
            VecFormat::Q8 => {
                let q = quantize(v);
                let mut out = Vec::with_capacity(4 + q.vec.len());
                out.extend_from_slice(&q.scale.to_le_bytes());
                out.extend(q.vec.iter().map(|b| *b as u8));
                out
            }
        }
    }

    pub fn decode(self, bytes: &[u8]) -> Option<Q8> {
        match self {
            VecFormat::F16 => {
                if bytes.is_empty() || !bytes.len().is_multiple_of(2) {
                    return None;
                }
                let v: Vec<f32> = bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| f16_to_f32(u16::from_le_bytes(*b)))
                    .collect();
                Some(quantize(&v))
            }
            VecFormat::Q8 => {
                if bytes.len() <= 4 {
                    return None;
                }
                let scale = f32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                Some(Q8 {
                    vec: bytes[4..].iter().map(|b| *b as i8).collect(),
                    scale,
                })
            }
        }
    }
}

pub(super) fn f32_to_f16(x: f32) -> u16 {
    let bits = x.to_bits();
    let sign = ((bits >> 16) & 0x8000) as u16;
    let exp = ((bits >> 23) & 0xff) as i32;
    let man = bits & 0x007f_ffff;
    if exp == 0xff {
        return sign | 0x7c00 | if man != 0 { 0x0200 } else { 0 };
    }
    let e = exp - 127 + 15;
    if e >= 0x1f {
        return sign | 0x7c00;
    }
    if e <= 0 {
        if e < -10 {
            return sign;
        }
        let m = man | 0x0080_0000;
        let shift = (14 - e) as u32;
        let mut half = m >> shift;
        let rem = m & ((1 << shift) - 1);
        let halfway = 1 << (shift - 1);
        if rem > halfway || (rem == halfway && half & 1 == 1) {
            half += 1;
        }
        return sign | half as u16;
    }
    let mut half = ((e as u32) << 10) | (man >> 13);
    let rem = man & 0x1fff;
    if rem > 0x1000 || (rem == 0x1000 && half & 1 == 1) {
        half += 1;
    }
    sign | half as u16
}

pub(super) fn f16_to_f32(h: u16) -> f32 {
    let sign = u32::from(h & 0x8000) << 16;
    let exp = u32::from((h >> 10) & 0x1f);
    let man = u32::from(h & 0x03ff);
    let bits = match exp {
        0 if man == 0 => sign,
        0 => {
            let v = man as f32 / (1u32 << 24) as f32;
            return if sign == 0 { v } else { -v };
        }
        0x1f => sign | 0x7f80_0000 | (man << 13),
        _ => sign | ((exp + 112) << 23) | (man << 13),
    };
    f32::from_bits(bits)
}
