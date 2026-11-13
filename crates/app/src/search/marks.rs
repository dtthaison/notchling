use std::ops::Range;

pub fn clean(snippet: &str, raw: &[(usize, usize)]) -> Vec<Range<usize>> {
    let mut ranges: Vec<Range<usize>> = raw
        .iter()
        .filter_map(|&(start, end)| {
            let end = floor(snippet, end.min(snippet.len()));
            let start = ceil(snippet, start.min(snippet.len()));
            (start < end).then_some(start..end)
        })
        .collect();
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut out: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        match out.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => out.push(range),
        }
    }
    out
}

fn floor(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at -= 1;
    }
    at
}

fn ceil(text: &str, mut at: usize) -> usize {
    while !text.is_char_boundary(at) {
        at += 1;
    }
    at
}

#[cfg(test)]
#[allow(clippy::single_range_in_vec_init)]
mod tests {
    use super::*;

    fn slices<'a>(text: &'a str, ranges: &[Range<usize>]) -> Vec<&'a str> {
        ranges.iter().map(|r| &text[r.clone()]).collect()
    }

    #[test]
    fn overlapping_ranges_are_joined() {
        let text = "the bridge\u{2019}s state";
        let ranges = clean(text, &[(4, 5), (4, 10)]);
        assert_eq!(ranges, [4..10]);
        assert_eq!(slices(text, &ranges), ["bridge"]);
        let text = "cache\u{2014}stale";
        let ranges = clean(text, &[(0, 1), (0, 5), (2, 3)]);
        assert_eq!(ranges, [0..5]);
    }

    #[test]
    fn touching_ranges_join_and_apart_ones_stay_sorted() {
        let text = "abc def abc";
        assert_eq!(clean(text, &[(8, 11), (0, 2), (2, 3)]), [0..3, 8..11]);
        assert_eq!(clean(text, &[(4, 7), (0, 3)]), [0..3, 4..7]);
    }

    #[test]
    fn ranges_are_cut_to_the_text_and_to_whole_characters() {
        let text = "the bridge\u{2019}s state";
        assert_eq!(clean(text, &[(14, 999)]), [14..text.len()]);
        assert!(clean(text, &[(11, 12)]).is_empty());
        assert_eq!(clean(text, &[(11, 15)]), [13..15]);
        assert_eq!(clean(text, &[(4, 12)]), [4..10]);
        for r in clean(text, &[(1, 11), (12, 20), (9, 14)]) {
            assert!(text.is_char_boundary(r.start) && text.is_char_boundary(r.end));
        }
    }

    #[test]
    fn empty_backwards_and_stray_ranges_are_dropped() {
        let text = "abc";
        assert!(clean(text, &[(1, 1), (2, 1), (7, 9)]).is_empty());
        assert!(clean("", &[(0, 3)]).is_empty());
        assert!(clean(text, &[]).is_empty());
    }
}
