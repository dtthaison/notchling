use super::refs::Counts;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub rev: String,
    pub commit: String,
}

pub fn short_name<'a>(rev: &'a str, remotes: &[String]) -> &'a str {
    remotes
        .iter()
        .filter_map(|remote| {
            rev.strip_prefix(remote.as_str())
                .and_then(|rest| rest.strip_prefix('/'))
                .map(|rest| (remote.len(), rest))
        })
        .max_by_key(|(len, _)| *len)
        .map_or(rev, |(_, rest)| rest)
}

pub fn candidates(
    own: Option<&str>,
    upstream: Option<&Candidate>,
    default: Option<&Candidate>,
    integration: Option<&Candidate>,
    remotes: &[String],
) -> Vec<Candidate> {
    let mut list: Vec<Candidate> = Vec::new();
    for candidate in [default, integration, upstream].into_iter().flatten() {
        if own.is_some_and(|own| short_name(&candidate.rev, remotes) == own) {
            continue;
        }
        if list.iter().any(|seen| seen.rev == candidate.rev) {
            continue;
        }
        list.push(candidate.clone());
    }
    list
}

pub fn pick(scored: &[(Candidate, Option<Counts>)]) -> Option<(Candidate, Counts)> {
    let mut best: Option<(&Candidate, Counts)> = None;
    for (candidate, counts) in scored {
        let Some(counts) = counts else { continue };
        let better = match &best {
            None => true,
            Some((_, held)) => (counts.ahead, counts.behind) < (held.ahead, held.behind),
        };
        if better {
            best = Some((candidate, *counts));
        }
    }
    best.map(|(candidate, counts)| (candidate.clone(), counts))
}
