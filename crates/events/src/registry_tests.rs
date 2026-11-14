use crate::registry::{all, find, COMMIT_KINDS, OWNER_KINDS};
use crate::spec::{AgentsRead, DefaultState, Durability, Group, KindSpec, Lock, Proc, Sinks};
use crate::KindName;

const SMALL_WORDS: &[&str] = &[
    "a", "an", "and", "as", "at", "but", "by", "for", "from", "in", "of", "on", "or", "the", "to",
    "with",
];

fn title_case(title: &str) -> bool {
    title.split(' ').enumerate().all(|(i, word)| {
        let first = word.chars().next();
        match first {
            Some(c) if c.is_ascii_uppercase() => true,
            Some(_) => i > 0 && SMALL_WORDS.contains(&word),
            None => false,
        }
    })
}

fn sentences_ok(text: &str) -> bool {
    let periods = text.matches('.').count();
    text.len() <= 200 && text.ends_with('.') && (1..=2).contains(&periods)
}

fn snake(name: &str) -> bool {
    let mut chars = name.chars();
    matches!(chars.next(), Some('a'..='z'))
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn dotted(name: &str) -> bool {
    let mut parts = name.split('.');
    let first_ok = parts
        .next()
        .is_some_and(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase()));
    let rest: Vec<&str> = parts.collect();
    first_ok
        && !rest.is_empty()
        && rest
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_lowercase() || b == b'_'))
}

fn spec(name: &str) -> &'static KindSpec {
    find(name).unwrap_or_else(|| panic!("{name} is not registered"))
}

#[test]
fn every_kind_has_title_records_why_default_and_lock() {
    for k in all() {
        assert!(
            !k.title.is_empty() && k.title.len() <= 40 && title_case(k.title),
            "{}: title {:?}",
            k.name,
            k.title
        );
        assert!(
            sentences_ok(k.records),
            "{}: records {:?}",
            k.name,
            k.records
        );
        assert!(sentences_ok(k.why), "{}: why {:?}", k.name, k.why);
        assert_eq!(k.kv, 1, "{}", k.name);
        match k.lock {
            Lock::Always(reason) => {
                assert_eq!(k.default, DefaultState::On, "{}", k.name);
                assert!(!reason.is_empty(), "{}", k.name);
            }
            Lock::Toggle => {}
        }
        assert!(!k.sinks.names().is_empty(), "{}", k.name);
        assert!(!k.emitters.is_empty(), "{}", k.name);
    }
}

#[test]
fn names_are_unique_and_dotted() {
    let kinds = all();
    for k in &kinds {
        assert!(dotted(k.name), "{}", k.name);
    }
    let mut names: Vec<&str> = kinds.iter().map(|k| k.name).collect();
    names.sort_unstable();
    let before = names.len();
    names.dedup();
    assert_eq!(before, names.len());
}

#[test]
fn owner_kinds_are_always_on() {
    for name in OWNER_KINDS {
        let k = spec(name);
        assert!(matches!(k.lock, Lock::Always(_)), "{name}");
        assert_eq!(k.default, DefaultState::On, "{name}");
    }
}

#[test]
fn commit_kinds_are_audited_and_always() {
    let committing: Vec<&str> = all()
        .iter()
        .filter(|k| k.durability == Durability::CommitFromAgent)
        .map(|k| k.name)
        .collect();
    for k in all() {
        if k.durability == Durability::CommitFromAgent {
            assert!(k.sinks.has(Sinks::AUDIT), "{}", k.name);
        }
    }
    for name in [
        "vault.secret_used",
        "vault.secret_requested",
        "settings.agent",
    ] {
        let k = spec(name);
        assert!(matches!(k.lock, Lock::Always(_)), "{name}");
        assert_eq!(k.durability, Durability::CommitFromAgent, "{name}");
    }
    let mut want: Vec<&str> = COMMIT_KINDS.to_vec();
    want.sort_unstable();
    let mut got = committing;
    got.sort_unstable();
    assert_eq!(got, want);
}

#[test]
fn agents_cannot_emit_locked_kinds() {
    for k in all() {
        let agent_side = k.emitters.contains(&Proc::Hook) || k.emitters.contains(&Proc::Cli);
        if matches!(k.lock, Lock::Always(_)) {
            assert!(!agent_side, "{}", k.name);
        }
        if agent_side {
            assert!(!k.sinks.has(Sinks::AUDIT), "{}", k.name);
        }
    }
}

#[test]
fn app_lifecycle_is_never_sealed() {
    for name in ["process.started", "process.stopped", "process.crashed"] {
        assert!(!spec(name).sinks.has(Sinks::AUDIT), "{name}");
    }
}

#[test]
fn field_names_and_choices_are_well_formed() {
    for k in all() {
        assert!(k.fields.len() <= 24, "{}", k.name);
        let mut seen = Vec::new();
        for f in k.fields {
            assert!(snake(f.name), "{}.{}", k.name, f.name);
            assert!(!seen.contains(&f.name), "{}.{} twice", k.name, f.name);
            seen.push(f.name);
            if f.desc.ty == "choice" {
                assert!(!f.desc.choices.is_empty(), "{}.{}", k.name, f.name);
                assert_eq!(
                    f.desc.choices.last(),
                    Some(&"other"),
                    "{}.{}",
                    k.name,
                    f.name
                );
                assert!(
                    f.desc.choices.iter().all(|c| !c.is_empty()),
                    "{}.{}",
                    k.name,
                    f.name
                );
            }
        }
    }
}

#[test]
fn fifty_five_kinds() {
    let kinds = all();
    assert_eq!(kinds.len(), 55);
    let off: Vec<&str> = kinds
        .iter()
        .filter(|k| k.default == DefaultState::Off)
        .map(|k| k.name)
        .collect();
    assert_eq!(off, vec!["phone.viewed"]);
}

#[test]
fn performance_kinds_have_only_metrics_and_diag() {
    for k in all().iter().filter(|k| k.group == Group::Performance) {
        assert!(k.name.starts_with("perf."), "{}", k.name);
        assert!(k.sinks.has(Sinks::METRICS), "{}", k.name);
        assert!(!k.sinks.has(Sinks::ACTIVITY), "{}", k.name);
        assert!(!k.sinks.has(Sinks::AUDIT), "{}", k.name);
    }
    assert!(all()
        .iter()
        .all(|k| (k.group == Group::Performance) == k.name.starts_with("perf.")));
}

#[test]
fn groups_come_in_order_and_each_has_kinds() {
    let kinds = all();
    let order: Vec<usize> = kinds
        .iter()
        .map(|k| {
            Group::ALL
                .iter()
                .position(|g| *g == k.group)
                .expect("group")
        })
        .collect();
    assert!(order.windows(2).all(|w| w[0] <= w[1]));
    for group in Group::ALL {
        assert!(kinds.iter().any(|k| k.group == group), "{}", group.name());
    }
}

#[test]
fn retention_follows_the_plan() {
    for k in all() {
        let want = match k.name {
            "account.switched" => 90,
            "usage.daily" => 365,
            n if n.starts_with("perf.") => 7,
            _ if matches!(k.lock, Lock::Always(_)) => 0,
            _ => 30,
        };
        assert_eq!(k.keep_days, want, "{}", k.name);
    }
}

#[test]
fn agents_read_their_own_only_where_the_plan_says() {
    for k in all() {
        let own = k.name.starts_with("session.")
            || matches!(
                k.name,
                "hook.decided"
                    | "tool.turn"
                    | "delegate.job"
                    | "vault.secret_used"
                    | "vault.secret_requested"
                    | "settings.agent"
            );
        assert_eq!(k.agents_read == AgentsRead::Own, own, "{}", k.name);
    }
}

#[test]
fn kind_name_parses_only_registered_kinds() {
    let parsed = KindName::parse("api.refused").expect("registered");
    assert_eq!(parsed.as_str(), "api.refused");
    assert_eq!(KindName::of(spec("api.refused")), parsed);
    assert!(KindName::parse("api.nope").is_none());
}

#[test]
fn every_kind_struct_writes_its_declared_fields() {
    use crate::event::Kind;
    use crate::kinds::choices::CallerTier;
    use crate::kinds::security::ApiRefused;
    use crate::{Exe, Op};
    let kind = ApiRefused {
        op: Op::named("vault.use"),
        exe: Some(Exe::named("gh")),
        tier: CallerTier::Agent,
        why: crate::kinds::security::RefusedWhy::Forbidden,
        switch: None,
    };
    let names = kind.fields().names();
    let declared: Vec<&str> = ApiRefused::SPEC.fields.iter().map(|f| f.name).collect();
    assert_eq!(names, declared);
}
