const LANES: usize = 16;

#[derive(Clone, Debug, PartialEq)]
pub struct Q8 {
    pub vec: Vec<i8>,
    pub scale: f32,
}

pub fn quantize(v: &[f32]) -> Q8 {
    let max = v.iter().fold(0.0f32, |m, x| m.max(x.abs()));
    if max == 0.0 || !max.is_finite() {
        return Q8 {
            vec: vec![0; v.len()],
            scale: 0.0,
        };
    }
    let scale = max / 127.0;
    let vec = v
        .iter()
        .map(|x| (x / scale).round().clamp(-127.0, 127.0) as i8)
        .collect();
    Q8 { vec, scale }
}

fn dot_i8(a: &[i8], b: &[i8]) -> i64 {
    let n = a.len().min(b.len());
    let (ca, ra) = a[..n].as_chunks::<LANES>();
    let (cb, rb) = b[..n].as_chunks::<LANES>();
    let mut lanes = [0i32; LANES];
    for (x, y) in ca.iter().zip(cb) {
        for i in 0..LANES {
            lanes[i] += i32::from(x[i]) * i32::from(y[i]);
        }
    }
    let mut total: i64 = lanes.iter().map(|&l| i64::from(l)).sum();
    for (x, y) in ra.iter().zip(rb) {
        total += i64::from(*x) * i64::from(*y);
    }
    total
}

pub fn dot(a: &Q8, b: &Q8) -> f32 {
    dot_i8(&a.vec, &b.vec) as f32 * a.scale * b.scale
}

pub fn dot_f32(q: &[f32], b: &Q8) -> f32 {
    dot_f32_i8(q, &b.vec, b.scale)
}

pub fn dot_f32_i8(q: &[f32], v: &[i8], scale: f32) -> f32 {
    let n = q.len().min(v.len());
    let (cq, rq) = q[..n].as_chunks::<LANES>();
    let (cb, rb) = v[..n].as_chunks::<LANES>();
    let mut lanes = [0f32; LANES];
    for (x, y) in cq.iter().zip(cb) {
        for i in 0..LANES {
            lanes[i] += x[i] * f32::from(y[i]);
        }
    }
    let mut total: f32 = lanes.iter().sum();
    for (x, y) in rq.iter().zip(rb) {
        total += x * f32::from(*y);
    }
    total * scale
}
