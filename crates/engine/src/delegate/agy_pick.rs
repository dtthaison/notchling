const LEVELS: [&str; 4] = ["low", "medium", "high", "xhigh"];
const NOT_OFFERED: [&str; 3] = ["max", "ultra", "minimal"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Listed {
    pub model: String,
    pub levels: Vec<String>,
    pub name: Option<String>,
    pub whole: String,
}

pub fn accepted(model: &str, effort: Option<&str>, offered: &[String]) -> bool {
    pick(Some(model), effort, offered)
        .0
        .is_some_and(|m| offered.contains(&m))
}

pub fn offers(model: &str, effort: &str, listed: &[(String, Vec<String>)]) -> bool {
    let level = level_of(effort);
    match listed.iter().find(|(m, _)| m == model) {
        Some((_, levels)) => levels.is_empty() || levels.contains(&level),
        None => true,
    }
}

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
        if let Some(b) = base.clone().filter(|b| !models.iter().any(|x| x == b)) {
            if let Some(whole) = models.iter().find(|x| x.starts_with(&format!("{b}-"))) {
                base = Some(whole.clone());
            }
        }
    }
    (base, level)
}

pub fn level_of(effort: &str) -> String {
    match effort.to_lowercase().as_str() {
        "minimal" | "none" | "low" => "low".into(),
        "max" | "high" => "high".into(),
        other => other.into(),
    }
}

pub fn entries(listing: &str) -> Vec<Listed> {
    let mut out: Vec<Listed> = Vec::new();
    for line in listing.lines() {
        let mut fields = line.split('\t');
        let Some(id) = fields.by_ref().find(|f| !f.is_empty()) else {
            continue;
        };
        let label = fields.find(|f| !f.is_empty()).map(str::trim);
        if id.contains(' ') || NOT_OFFERED.iter().any(|w| id.ends_with(&format!("-{w}"))) {
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
        let at = match out.iter().position(|e| e.model == model) {
            Some(at) => at,
            None => {
                out.push(Listed {
                    model: model.clone(),
                    levels: Vec::new(),
                    name: None,
                    whole: id.to_string(),
                });
                out.len() - 1
            }
        };
        let entry = &mut out[at];
        if !level.is_empty() {
            entry.levels.push(level.to_string());
            if entry.name.is_none() {
                entry.name = label.map(|l| strip_level_word(l).to_string());
            }
        } else {
            entry.name = label.map(str::to_string);
        }
    }
    for entry in &mut out {
        entry
            .levels
            .sort_by_key(|l| LEVELS.iter().position(|x| x == l));
    }
    out
}

fn strip_level_word(label: &str) -> &str {
    match label.rsplit_once(" (") {
        Some((head, tail)) if tail.ends_with(')') && !head.is_empty() => head,
        _ => label,
    }
}

pub fn models(listing: &str) -> Vec<(String, Vec<String>)> {
    entries(listing)
        .into_iter()
        .map(|e| (e.model, e.levels))
        .collect()
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
        assert_eq!(pick(None, Some("xhigh"), &m).1.as_deref(), Some("xhigh"));
        assert_eq!(pick(None, Some("max"), &m).1.as_deref(), Some("high"));
        assert_eq!(pick(None, Some("minimal"), &m).1.as_deref(), Some("low"));
        assert_eq!(pick(None, None, &m), (None, None));
        assert_eq!(
            pick(Some("nonsense"), None, &m).0.as_deref(),
            Some("nonsense"),
            "an unknown model is left for the caller to refuse"
        );
    }

    #[test]
    fn accepted_is_what_a_run_would_take() {
        let m = offered();
        assert!(accepted("gemini-4-argon", None, &m));
        assert!(accepted("gemini-4-argon-high", None, &m));
        assert!(accepted("gpt-oss-120b", None, &m));
        assert!(accepted("gemini-4-argon-xhigh", None, &m));
        assert!(!accepted("nonsense", None, &m));
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
        let with_max =
            "gemini-4-hydrogen-high\tH\ngemini-4-hydrogen-max\tM\ngemini-4-hydrogen-ultra\tU\n";
        assert_eq!(
            models(with_max),
            [("gemini-4-hydrogen".to_string(), vec!["high".to_string()])]
        );
    }

    const LISTING: &str = "Fetching available models...\ngemini-4-neon-high\tGemini 4 Neon (High)\ngemini-4-neon-xhigh\tGemini 4 Neon (XHigh)\ngemini-4-neon-medium\tGemini 4 Neon (Medium)\ngemini-4-neon-low\tGemini 4 Neon (Low)\ngemini-4-argon-medium\tGemini 4 Argon (Medium)\ngemini-4-argon-high\tGemini 4 Argon (High)\ngemini-pro-orchestrator-neon\tOrchestrator (Neon)\ngpt-oss-120b-medium\tGPT-OSS 120B (Medium)\n";

    #[test]
    fn xhigh_is_a_level_where_the_listing_has_it_and_the_levels_come_out_in_order() {
        let groups = models(LISTING);
        assert_eq!(
            groups[0],
            (
                "gemini-4-neon".to_string(),
                ["low", "medium", "high", "xhigh"]
                    .map(String::from)
                    .to_vec()
            )
        );
        assert_eq!(groups[1].1, ["medium", "high"]);
        assert!(groups[2].1.is_empty());
        assert_eq!(groups[3].0, "gpt-oss-120b");
        let listed = entries(LISTING);
        assert_eq!(listed[0].name.as_deref(), Some("Gemini 4 Neon"));
        assert_eq!(listed[2].name.as_deref(), Some("Orchestrator (Neon)"));
        assert_eq!(listed[3].whole, "gpt-oss-120b-medium");
    }

    #[test]
    fn an_effort_is_offered_only_where_the_model_lists_it() {
        let groups = models(LISTING);
        assert!(offers("gemini-4-neon", "xhigh", &groups));
        assert!(offers("gemini-4-neon", "high", &groups));
        assert!(!offers("gemini-4-argon", "xhigh", &groups));
        assert!(!offers("gemini-4-argon", "low", &groups));
        assert!(
            offers("gemini-4-argon", "max", &groups),
            "max is read as high"
        );
        assert!(offers("gemini-pro-orchestrator-neon", "xhigh", &groups));
        assert!(offers("unknown", "xhigh", &groups));
    }
}
