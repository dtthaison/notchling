//! Antigravity's model and effort (Swift `Delegate.agyPick`, `agyLevel`, `agyModels`).
//!
//! `agy models` ids carry the thinking level as a suffix (`gemini-4-argon-high`), while a session
//! sets the model (`gemini-4-argon`) and `thought_level` apart.

const LEVELS: [&str; 3] = ["low", "medium", "high"];

/// Accepts a model with or without the level suffix; an effort given on its own wins over the
/// suffix. Claude Code's levels map onto agy's three. Returns `(model, level)`.
pub fn pick(
    model: Option<&str>,
    effort: Option<&str>,
    models: &[String],
) -> (Option<String>, Option<String>) {
    let mut base = model.map(String::from);
    let mut level = effort.map(level_of);
    if let Some(m) = model.filter(|m| !models.iter().any(|x| x == m)) {
        for suffix in LEVELS {
            if let Some(stem) = m.strip_suffix(&format!("-{suffix}")) {
                if models.iter().any(|x| x == stem) {
                    base = Some(stem.to_string());
                    if level.is_none() {
                        level = Some(suffix.to_string());
                    }
                }
            }
        }
        // A model offered only with its level in the name (`gpt-oss-120b-medium`).
        if let Some(b) = base.clone().filter(|b| !models.iter().any(|x| x == b)) {
            if let Some(whole) = models.iter().find(|x| x.starts_with(&format!("{b}-"))) {
                base = Some(whole.clone());
            }
        }
    }
    (base, level)
}

/// Claude Code's effort words as agy's `low`, `medium` and `high`; anything else lower-cased.
pub fn level_of(effort: &str) -> String {
    match effort.to_lowercase().as_str() {
        "minimal" | "none" | "low" => "low".into(),
        "xhigh" | "max" | "high" => "high".into(),
        other => other.into(),
    }
}

/// `agy models` lines (`id<TAB>name`) grouped as model and its thinking levels, in the order
/// listed. A line whose first field has a space (a progress message) is skipped.
pub fn models(listing: &str) -> Vec<(String, Vec<String>)> {
    let mut order: Vec<String> = Vec::new();
    let mut levels: Vec<(String, Vec<String>)> = Vec::new();
    for line in listing.lines() {
        let Some(id) = line.split('\t').find(|f| !f.is_empty()) else {
            continue;
        };
        if id.contains(' ') {
            continue;
        }
        let mut model = id.to_string();
        let mut level = "";
        for suffix in LEVELS {
            if let Some(stem) = id.strip_suffix(&format!("-{suffix}")) {
                model = stem.to_string();
                level = suffix;
            }
        }
        if !order.contains(&model) {
            order.push(model.clone());
            levels.push((model.clone(), Vec::new()));
        }
        if !level.is_empty() {
            if let Some((_, list)) = levels.iter_mut().find(|(m, _)| *m == model) {
                list.push(level.to_string());
            }
        }
    }
    levels
}

#[cfg(test)]
mod tests {
    use super::*;

    fn offered() -> Vec<String> {
        ["gemini-4-argon", "gemini-3.8-flash", "gpt-oss-120b-medium"]
            .iter()
            .map(|s| s.to_string())
            .collect()
    }

    #[test]
    fn a_level_suffix_is_split_off_and_an_explicit_effort_wins() {
        let m = offered();
        let (model, level) = pick(Some("gemini-4-argon-high"), None, &m);
        assert_eq!(model.as_deref(), Some("gemini-4-argon"));
        assert_eq!(level.as_deref(), Some("high"));
        let (_, level) = pick(Some("gemini-4-argon-high"), Some("low"), &m);
        assert_eq!(level.as_deref(), Some("low"));
        assert_eq!(
            pick(Some("gpt-oss-120b-medium"), None, &m).0.as_deref(),
            Some("gpt-oss-120b-medium"),
            "a value Antigravity offers as is stays whole"
        );
        assert_eq!(
            pick(Some("gpt-oss-120b"), None, &m).0.as_deref(),
            Some("gpt-oss-120b-medium")
        );
        assert_eq!(pick(None, Some("xhigh"), &m).1.as_deref(), Some("high"));
        assert_eq!(pick(None, Some("minimal"), &m).1.as_deref(), Some("low"));
        assert_eq!(pick(None, None, &m), (None, None));
        assert_eq!(
            pick(Some("nonsense"), None, &m).0.as_deref(),
            Some("nonsense"),
            "an unknown model is left for the caller to refuse"
        );
    }

    #[test]
    fn levels_are_lower_cased_and_mapped() {
        assert_eq!(level_of("MAX"), "high");
        assert_eq!(level_of("None"), "low");
        assert_eq!(level_of("Medium"), "medium");
        assert_eq!(level_of("odd"), "odd");
    }

    #[test]
    fn the_listing_groups_levels_under_their_model() {
        let listing = "Fetching available models...\ngemini-4-argon-medium\tGemini 4 Argon (Medium)\ngemini-4-argon-high\tGemini 4 Argon (High)\ngpt-oss-120b\tGPT-OSS\n\n\t\n";
        let groups = models(listing);
        let names: Vec<_> = groups.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(names, ["gemini-4-argon", "gpt-oss-120b"]);
        assert_eq!(groups[0].1, ["medium", "high"]);
        assert!(groups[1].1.is_empty());
        assert!(models("").is_empty());
    }
}
