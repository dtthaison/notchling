use super::*;

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn lcg(state: &mut u64) -> f32 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*state >> 33) as f32 / (1u64 << 31) as f32) * 2.0 - 1.0
}

fn unit(v: Vec<f32>) -> Vec<f32> {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    v.into_iter().map(|x| x / n).collect()
}

#[test]
fn hash_embedder_is_deterministic_and_normalized() {
    let e = HashEmbedder::new("fake/p1");
    assert_eq!(e.dims(), 64);
    assert_eq!(e.model(), &ModelId("fake/p1".to_string()));
    let a = e.embed(&["đăng nhập lỗi", "keep the mac awake"]).unwrap();
    let b = e.embed(&["đăng nhập lỗi", "keep the mac awake"]).unwrap();
    assert_eq!(a, b);
    for v in &a {
        assert_eq!(v.len(), 64);
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        assert!((norm - 1.0).abs() < 1e-5, "{norm}");
    }
    assert_eq!(e.embed(&[]).unwrap().len(), 0);
}

#[test]
fn similar_text_scores_higher_under_hash() {
    let e = HashEmbedder::new("fake/p1");
    let v = e
        .embed(&["đăng nhập lỗi", "dang nhap loi", "keep the mac awake"])
        .unwrap();
    assert!(cosine(&v[0], &v[1]) > cosine(&v[0], &v[2]) + 0.5);
    let near = e
        .embed(&["login fails after update", "login failed after updating"])
        .unwrap();
    let far = e.embed(&["login fails after update"]).unwrap();
    let other = e.embed(&["sleep schedule of the notch"]).unwrap();
    assert!(cosine(&near[0], &near[1]) > cosine(&far[0], &other[0]));
}

#[test]
fn table_embedder_refuses_unknown_text() {
    let e = TableEmbedder::new(
        "table/p1",
        3,
        vec![
            ("a".to_string(), vec![1.0, 0.0, 0.0]),
            ("b".to_string(), vec![0.0, 1.0, 0.0]),
            ("short".to_string(), vec![1.0]),
        ],
    );
    assert_eq!(e.dims(), 3);
    assert_eq!(
        e.embed(&["a", "b"]).unwrap(),
        vec![vec![1.0, 0.0, 0.0], vec![0.0, 1.0, 0.0]]
    );
    assert!(matches!(e.embed(&["a", "zzz"]), Err(EmbedError::Failed(_))));
    assert!(matches!(e.embed(&["short"]), Err(EmbedError::Failed(_))));
}

#[test]
fn int8_dot_is_within_1_percent_of_f32() {
    let mut s = 42u64;
    for _ in 0..50 {
        let a = unit((0..384).map(|_| lcg(&mut s)).collect());
        let noise: Vec<f32> = (0..384).map(|_| lcg(&mut s) * 0.5).collect();
        let b = unit(a.iter().zip(&noise).map(|(x, n)| x + n * 0.05).collect());
        let exact = cosine(&a, &b);
        assert!(exact > 0.5);
        let (qa, qb) = (quantize(&a), quantize(&b));
        assert!(((dot(&qa, &qb) - exact) / exact).abs() < 0.01);
        assert!(((dot_f32(&a, &qb) - exact) / exact).abs() < 0.01);
        let c = unit((0..384).map(|_| lcg(&mut s)).collect());
        let qc = quantize(&c);
        assert!((dot(&qa, &qc) - cosine(&a, &c)).abs() < 0.01);
    }
}

#[test]
fn quantizing_zeros_and_odd_lengths_is_safe() {
    let z = quantize(&[0.0; 5]);
    assert_eq!(z.vec, vec![0; 5]);
    assert_eq!(dot(&z, &z), 0.0);
    let v = [0.5f32, -0.25, 1.0];
    let q = quantize(&v);
    assert_eq!(q.vec[2], 127);
    assert!((dot_f32(&v, &q) - 1.3125).abs() < 0.02);
}
