use super::estimate;
use crate::catalog::{Over, Price};

fn price() -> Price {
    Price {
        input: 8.0,
        output: 40.0,
        cache_read: 0.2,
        over: None,
    }
}

#[test]
fn a_price_turns_tokens_into_dollars() {
    let found = estimate(250_000, Some(&price()));
    assert_eq!(found.tokens, 250_000);
    assert!((found.usd.unwrap() - 2.0).abs() < 1e-9);
}

#[test]
fn no_price_leaves_the_tokens_alone() {
    let found = estimate(42_000, None);
    assert_eq!(found.tokens, 42_000);
    assert_eq!(found.usd, None);
}

#[test]
fn the_long_context_rate_applies_past_its_line() {
    let long = Price {
        over: Some(Over {
            tokens: 200_000.0,
            input: 16.0,
            output: 60.0,
            cache_read: 0.4,
        }),
        ..price()
    };
    assert!((estimate(100_000, Some(&long)).usd.unwrap() - 0.8).abs() < 1e-9);
    assert!((estimate(300_000, Some(&long)).usd.unwrap() - 4.8).abs() < 1e-9);
}
