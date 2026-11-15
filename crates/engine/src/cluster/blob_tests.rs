use super::blob::{chunks, safe_name, sha256_of, Digest, Incoming, CHUNK, MAX_BLOB};

#[test]
fn blob_must_arrive_in_order() {
    let mut blob = Incoming::default();
    blob.put(0, b"abc").unwrap();
    assert!(blob.put(0, b"abc").unwrap_err().contains("out of order"));
    assert!(blob.put(5, b"x").unwrap_err().contains("expected offset 3"));
    blob.put(3, b"def").unwrap();
    assert_eq!(blob.next(), 6);
    blob.finish(&sha256_of(b"abcdef")).unwrap();
}

#[test]
fn blob_hash_mismatch_is_rejected() {
    let mut blob = Incoming::default();
    blob.put(0, b"abc").unwrap();
    assert!(blob
        .finish(&sha256_of(b"abd"))
        .unwrap_err()
        .contains("hash doesn't match"));
    let mut empty = Incoming::default();
    empty.put(0, b"").unwrap();
    assert!(empty.finish("not a hash").is_err());
    Incoming::default().finish(&sha256_of(b"")).unwrap();
}

#[test]
fn blob_over_cap_is_refused() {
    let mut small = Incoming::with_cap(10);
    small.put(0, &[0; 6]).unwrap();
    assert!(small.put(6, &[0; 5]).unwrap_err().contains("over"));
    small.put(6, &[0; 4]).unwrap();
    let mut blob = Incoming::default();
    assert!(blob
        .put(0, &vec![0; CHUNK + 1])
        .unwrap_err()
        .contains("256 KB"));
    assert_eq!(MAX_BLOB, 256 * 1024 * 1024);
}

#[test]
fn chunks_cover_the_length_once() {
    assert!(chunks(0).is_empty());
    assert_eq!(chunks(5), vec![(0, 5)]);
    let step = CHUNK as u64;
    assert_eq!(
        chunks(step * 2 + 7),
        vec![(0, CHUNK), (step, CHUNK), (step * 2, 7)]
    );
    assert_eq!(chunks(step), vec![(0, CHUNK)]);
}

#[test]
fn safe_names_are_plain() {
    for good in ["j-0123456789ab", "a_b-C9", "x"] {
        assert!(safe_name(good), "{good}");
    }
    for bad in [
        "",
        "-x",
        "a/b",
        "..",
        "a.b",
        "a b",
        "a\nb",
        "é",
        &"x".repeat(65),
    ] {
        assert!(!safe_name(bad), "{bad:?}");
    }
}

#[test]
fn a_digest_in_pieces_matches_one_hash() {
    let mut digest = Digest::default();
    digest.update(b"hello ");
    digest.update(b"world");
    assert_eq!(digest.finish(), sha256_of(b"hello world"));
}
