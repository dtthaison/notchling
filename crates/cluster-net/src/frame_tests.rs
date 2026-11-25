use super::*;

#[test]
fn lines_split_across_chunks() {
    let mut r = LineReader::new(MAX_LINE);
    assert_eq!(r.push(b"ab").unwrap(), Vec::<Vec<u8>>::new());
    assert_eq!(
        r.push(b"c\nde\n\nf").unwrap(),
        vec![b"abc".to_vec(), b"de".to_vec()]
    );
    assert_eq!(r.push(b"\n").unwrap(), vec![b"f".to_vec()]);
}

#[test]
fn line_over_cap_is_an_error() {
    let mut r = LineReader::new(8);
    assert!(r.push(b"12345678\n").is_ok());
    assert!(r.push(b"123456789").is_err());
    let mut r = LineReader::new(8);
    assert!(r.push(b"1234").is_ok());
    assert!(r.push(b"56789\n").is_err());
}

#[test]
fn outbox_is_bounded() {
    let (outbox, rx) = Outbox::new(2);
    assert!(outbox.push(b"a".to_vec()));
    assert!(outbox.push(b"b".to_vec()));
    assert!(!outbox.push(b"c".to_vec()), "full");
    assert_eq!(rx.try_recv().unwrap(), b"a".to_vec());
}
