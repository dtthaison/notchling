use super::hybrid::*;
use super::store::DocHit;
use serde_json::Value;

fn hit(key: &str, chunk: u8, score: f32) -> DocHit {
    DocHit {
        corpus: "sessions".to_string(),
        key: key.to_string(),
        chunk: [chunk; 16],
        score,
    }
}

fn hits(keys: &[&str]) -> Vec<DocHit> {
    keys.iter()
        .enumerate()
        .map(|(i, k)| hit(k, i as u8, 1.0 - i as f32 * 0.01))
        .collect()
}

fn text(keys: &[&str]) -> Vec<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn no_age(_: &str) -> Option<f64> {
    None
}

fn golden_hits(value: &Value) -> Vec<DocHit> {
    value
        .as_array()
        .unwrap()
        .iter()
        .enumerate()
        .map(|(i, h)| {
            hit(
                h["key"].as_str().unwrap(),
                h["chunk"].as_u64().unwrap() as u8,
                1.0 - i as f32 * 0.01,
            )
        })
        .collect()
}

#[test]
fn rrf_golden() {
    let golden: Value = serde_json::from_str(include_str!("hybrid_golden.json")).unwrap();
    let lists = Lists {
        text: golden["text"]
            .as_array()
            .unwrap()
            .iter()
            .map(|k| k.as_str().unwrap().to_string())
            .collect(),
        text_or: golden["text_or"].as_bool().unwrap(),
        granite: Some(golden_hits(&golden["granite"])),
        statics: Some(golden_hits(&golden["statics"])),
        floors: Floors::default(),
    };
    let out = fuse(&lists, 10, &no_age);
    let expect = golden["expect"].as_array().unwrap();
    assert_eq!(out.len(), expect.len());
    for (got, want) in out.iter().zip(expect) {
        assert_eq!(got.key, want["key"].as_str().unwrap());
        assert!(
            (got.score - want["score"].as_f64().unwrap()).abs() < 1e-6,
            "{got:?}"
        );
        let matched = match want["match"].as_str().unwrap() {
            "text" => Match::Text,
            "semantic" => Match::Semantic,
            _ => Match::Both,
        };
        assert_eq!(got.matched, matched, "{}", got.key);
    }
    let b = out.iter().find(|r| r.key == "b").unwrap();
    assert_eq!(b.chunk, Some([1u8; 16]));
    let c = out.iter().find(|r| r.key == "c").unwrap();
    assert_eq!(c.chunk, None);
}

#[test]
fn exact_looking_queries_use_text_only() {
    for query in [
        "~/Developer/notchling",
        "https://claude.ai/artifact/x",
        "\"exact words\"",
        "a1b2c3d4e5",
    ] {
        let plan = plan(query, Semantic::Ready);
        assert!(plan.text && !plan.granite && !plan.statics, "{query}");
    }
    let plan = plan("how do I pause indexing", Semantic::Ready);
    assert!(plan.text && plan.granite && plan.statics);
}

#[test]
fn warming_fuses_text_and_static() {
    let warming = plan("search by meaning", Semantic::Warming);
    assert!(warming.text && warming.statics && !warming.granite);
    for state in [Semantic::Indexing, Semantic::Paused, Semantic::Ready] {
        let p = plan("search by meaning", state);
        assert!(p.text && p.statics && p.granite);
    }
}

#[test]
fn off_is_text_only() {
    for state in [Semantic::Off, Semantic::NoModel] {
        let p = plan("search by meaning", state);
        assert!(p.text && !p.statics && !p.granite);
    }
}

#[test]
fn meaning_only_fills_at_most_half() {
    let sem: Vec<String> = (0..20).map(|i| format!("m{i}")).collect();
    let sem_refs: Vec<&str> = sem.iter().map(String::as_str).collect();
    let lists = Lists {
        text: text(&["t0", "t1", "t2", "t3", "t4", "t5"]),
        text_or: false,
        granite: Some(hits(&sem_refs)),
        statics: None,
        floors: Floors::default(),
    };
    let out = fuse(&lists, 10, &no_age);
    assert_eq!(out.len(), 10);
    let only = out.iter().filter(|r| r.matched == Match::Semantic).count();
    assert!(only <= 5, "{only}");
    assert!(out.iter().any(|r| r.key == "t4"));
}

#[test]
fn a_short_text_list_leaves_meaning_all_the_room() {
    let lists = Lists {
        text: text(&["t0", "t1"]),
        text_or: false,
        granite: Some(hits(&["m0", "m1", "m2", "m3", "m4", "m5", "m6", "m7"])),
        statics: None,
        floors: Floors::default(),
    };
    let out = fuse(&lists, 10, &no_age);
    assert_eq!(out.len(), 10);
}

#[test]
fn an_or_text_list_weighs_half() {
    let mut lists = Lists {
        text: text(&["t"]),
        text_or: false,
        granite: Some(hits(&["m"])),
        statics: None,
        floors: Floors::default(),
    };
    let and = fuse(&lists, 10, &no_age);
    assert_eq!(and[0].key, "t");
    lists.text_or = true;
    let or = fuse(&lists, 10, &no_age);
    assert_eq!(or[0].key, "m");
}

#[test]
fn recency_lifts_a_fresh_hit() {
    let lists = Lists {
        text: text(&["old", "new"]),
        text_or: false,
        granite: None,
        statics: None,
        floors: Floors::default(),
    };
    let age = |key: &str| (key == "new").then_some(0.0);
    let out = fuse(&lists, 10, &age);
    assert_eq!(out[0].key, "new");
}

#[test]
fn floors_drop_low_scores() {
    let lists = Lists {
        text: Vec::new(),
        text_or: false,
        granite: Some(vec![hit("hi", 1, 0.8), hit("lo", 2, 0.2)]),
        statics: Some(vec![hit("shi", 3, 0.7), hit("slo", 4, 0.1)]),
        floors: Floors::default(),
    };
    let floors = Floors {
        granite: 0.5,
        statics: 0.4,
    };
    let out = fuse_with(&lists, 10, &no_age, floors);
    let keys: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
    assert_eq!(keys, vec!["hi", "shi"]);
    let none = Floors {
        granite: 0.0,
        statics: 0.0,
    };
    assert_eq!(fuse_with(&lists, 10, &no_age, none).len(), 4);
    let carried = Lists {
        floors: none,
        ..lists.clone()
    };
    assert_eq!(fuse(&carried, 10, &no_age).len(), 4);
}

#[test]
fn floors_are_the_p4b_calibration() {
    assert_eq!(FLOOR_GRANITE, 0.80);
    assert_eq!(FLOOR_STATIC, 0.0);
    assert_eq!(
        Floors::default(),
        Floors {
            granite: 0.80,
            statics: 0.0,
        }
    );
    let lists = Lists {
        text: Vec::new(),
        text_or: false,
        granite: Some(vec![hit("kept", 1, 0.80), hit("dropped", 2, 0.79)]),
        statics: Some(vec![hit("low", 3, 0.01)]),
        floors: Floors::default(),
    };
    assert_eq!(lists.floors, Floors::default());
    let out = fuse(&lists, 10, &no_age);
    let mut keys: Vec<&str> = out.iter().map(|r| r.key.as_str()).collect();
    keys.sort_unstable();
    assert_eq!(keys, vec!["kept", "low"]);
}
