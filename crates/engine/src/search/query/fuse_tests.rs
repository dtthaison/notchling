use super::fuse::*;

fn ids(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

fn score_of(out: &[(String, f64)], id: &str) -> f64 {
    out.iter().find(|(i, _)| i == id).unwrap().1
}

#[test]
fn rrf_golden() {
    let (l1, l2, l3) = (ids(&["a", "b", "c"]), ids(&["b", "a"]), ids(&["c"]));
    let lists = [
        List {
            ids: &l1,
            weight: 1.0,
        },
        List {
            ids: &l2,
            weight: 1.0,
        },
        List {
            ids: &l3,
            weight: 1.0,
        },
    ];
    let out = rrf(&lists, K);
    let order: Vec<&str> = out.iter().map(|(i, _)| i.as_str()).collect();
    assert_eq!(order, ["a", "b", "c"]);
    assert!((score_of(&out, "a") - (1.0 / 61.0 + 1.0 / 62.0)).abs() < 1e-12);
    assert!((score_of(&out, "b") - (1.0 / 62.0 + 1.0 / 61.0)).abs() < 1e-12);
    assert!((score_of(&out, "c") - (1.0 / 63.0 + 1.0 / 61.0)).abs() < 1e-12);
}

#[test]
fn a_missing_rank_adds_nothing() {
    let (l1, l2) = (ids(&["a", "b"]), ids(&["c"]));
    let out = rrf(
        &[
            List {
                ids: &l1,
                weight: 1.0,
            },
            List {
                ids: &l2,
                weight: 1.0,
            },
        ],
        K,
    );
    assert!((score_of(&out, "b") - 1.0 / 62.0).abs() < 1e-12);
    assert!((score_of(&out, "c") - 1.0 / 61.0).abs() < 1e-12);
    assert_eq!(out[0].0, "a");
    assert_eq!(out[1].0, "c");
    assert!(rrf(&[], K).is_empty());
}

#[test]
fn duplicates_in_one_list_count_once() {
    let l = ids(&["a", "a", "b"]);
    let out = rrf(
        &[List {
            ids: &l,
            weight: 1.0,
        }],
        K,
    );
    assert_eq!(out.len(), 2);
    assert!((score_of(&out, "a") - 1.0 / 61.0).abs() < 1e-12);
    assert!((score_of(&out, "b") - 1.0 / 63.0).abs() < 1e-12);
}

#[test]
fn weights_scale_lists() {
    let (l1, l2) = (ids(&["a"]), ids(&["b"]));
    let out = rrf(
        &[
            List {
                ids: &l1,
                weight: 1.0,
            },
            List {
                ids: &l2,
                weight: 2.0,
            },
        ],
        K,
    );
    assert_eq!(out[0].0, "b");
    assert!((score_of(&out, "b") - 2.0 / 61.0).abs() < 1e-12);
    assert!((score_of(&out, "a") - 1.0 / 61.0).abs() < 1e-12);
}

#[test]
fn recency_lifts_a_fresh_tie() {
    let fused = vec![("old".to_string(), 0.02), ("new".to_string(), 0.02)];
    let age = |id: &str| match id {
        "old" => Some(30.0 * 86400.0),
        "new" => Some(0.0),
        _ => None,
    };
    let out = recency(fused, &age);
    assert_eq!(out[0].0, "new");
    assert!((out[0].1 - 0.03).abs() < 1e-12);
    assert!((out[1].1 - 0.02).abs() < 1e-12);
}

#[test]
fn recency_halves_toward_the_window_and_ignores_unknown_ages() {
    let fused = vec![("a".to_string(), 1.0), ("b".to_string(), 1.0)];
    let half = crate::search::RECENT_SECS / 2.0;
    let age = |id: &str| if id == "a" { Some(half) } else { None };
    let out = recency(fused, &age);
    assert_eq!(out[0].0, "a");
    assert!((out[0].1 - 1.25).abs() < 1e-12);
    assert!((out[1].1 - 1.0).abs() < 1e-12);
}
