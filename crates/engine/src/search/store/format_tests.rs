use super::format::{f16_to_f32, f32_to_f16};
use super::*;

#[test]
fn f16_known_values() {
    assert_eq!(f32_to_f16(1.0), 0x3c00);
    assert_eq!(f32_to_f16(-2.0), 0xc000);
    assert_eq!(f32_to_f16(0.5), 0x3800);
    assert_eq!(f32_to_f16(65504.0), 0x7bff);
    assert_eq!(f32_to_f16(1e9), 0x7c00);
    assert_eq!(f32_to_f16(0.0), 0);
    assert_eq!(f16_to_f32(0x3c00), 1.0);
    assert_eq!(f16_to_f32(0xc000), -2.0);
    assert_eq!(f16_to_f32(0x0001), 2.0f32.powi(-24));
    assert!(f16_to_f32(0x7c00).is_infinite());
}

#[test]
fn f16_round_trip_is_close() {
    let mut state = 7u64;
    for _ in 0..2000 {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let x = ((state >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0;
        let back = f16_to_f32(f32_to_f16(x));
        assert!((back - x).abs() <= x.abs() * 1e-3 + 1e-7, "{x} {back}");
    }
}

#[test]
fn q8_bytes_are_a_scale_then_the_values() {
    let v = [0.5f32, -1.0, 0.25, 0.0];
    let bytes = VecFormat::Q8.encode(&v);
    assert_eq!(bytes.len(), 4 + v.len());
    let q = VecFormat::Q8.decode(&bytes).unwrap();
    assert_eq!(q.vec.len(), 4);
    for (x, b) in v.iter().zip(&q.vec) {
        assert!((f32::from(*b) * q.scale - x).abs() < 0.01);
    }
}

#[test]
fn f16_bytes_decode_to_a_close_int8() {
    let v = [0.5f32, -1.0, 0.25, 0.0, 0.75, 0.1, -0.3, 0.9];
    let bytes = VecFormat::F16.encode(&v);
    assert_eq!(bytes.len(), 2 * v.len());
    let q = VecFormat::F16.decode(&bytes).unwrap();
    for (x, b) in v.iter().zip(&q.vec) {
        assert!((f32::from(*b) * q.scale - x).abs() < 0.01);
    }
    assert!(VecFormat::F16.decode(&bytes[..3]).is_none());
    assert!(VecFormat::Q8.decode(&[1, 2]).is_none());
}
