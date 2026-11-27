use super::query::fold;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FacetValue {
    pub value: String,
    pub label: String,
    pub count: usize,
}

pub fn label(name: &str, value: &str) -> String {
    if name == "project" {
        let trimmed = value.trim_end_matches('/');
        return trimmed
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(trimmed)
            .to_owned();
    }
    value.to_owned()
}

pub fn tally<I: IntoIterator<Item = String>>(
    values: I,
    name: &str,
    prefix: &str,
    limit: usize,
) -> Vec<FacetValue> {
    let prefix = fold(prefix.trim());
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for value in values {
        *counts.entry(value).or_insert(0) += 1;
    }
    let mut out: Vec<FacetValue> = counts
        .into_iter()
        .map(|(value, count)| FacetValue {
            label: label(name, &value),
            value,
            count,
        })
        .filter(|v| {
            prefix.is_empty()
                || fold(&v.label).starts_with(&prefix)
                || fold(&v.value).starts_with(&prefix)
        })
        .collect();
    out.sort_by(|a, b| {
        b.count
            .cmp(&a.count)
            .then_with(|| a.label.cmp(&b.label))
            .then_with(|| a.value.cmp(&b.value))
    });
    out.truncate(limit);
    out
}
