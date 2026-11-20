use crate::catalog::Price;

#[derive(Clone, Debug, PartialEq)]
pub struct Estimate {
    pub tokens: u64,
    pub usd: Option<f64>,
}

pub fn estimate(tokens: u64, price: Option<&Price>) -> Estimate {
    let usd = price.map(|price| {
        let rate = match price.over {
            Some(over) if tokens as f64 > over.tokens => over.input,
            _ => price.input,
        };
        tokens as f64 / 1_000_000.0 * rate
    });
    Estimate { tokens, usd }
}
