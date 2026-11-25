use super::*;

const WORDS: &[&str] = &["alpha", "beta"];

#[test]
fn a_name_made_only_of_the_given_words_is_generic() {
    for name in ["alpha", "Alpha", "alpha-beta", "beta_alpha", "alpha.beta"] {
        assert!(generic_in(name, WORDS), "{name}");
    }
}

#[test]
fn a_name_with_one_other_word_is_not() {
    for name in ["alpha-gamma", "gamma", "m1-alpha"] {
        assert!(!generic_in(name, WORDS), "{name}");
    }
}

#[test]
fn an_empty_name_or_empty_list_is_not_generic() {
    assert!(!generic_in("", WORDS));
    assert!(!generic_in("-", WORDS));
    assert!(!generic_in("alpha", &[]));
}

#[test]
fn a_machine_named_after_the_machine_passes() {
    for name in ["m1", "vps-sing-1", "build-mac", "oracle-sg", "m1-admin"] {
        assert!(!machine_generic(name), "{name}");
    }
}

#[test]
fn a_generic_machine_name_is_caught_with_or_without_a_number() {
    for name in [
        "server",
        "Host",
        "new",
        "machine",
        "new-server",
        "server-2",
        "vps_1",
    ] {
        assert!(machine_generic(name), "{name}");
    }
}

#[test]
fn a_cluster_named_after_its_purpose_passes() {
    for name in ["fleet", "acme-builds", "fleet-workers", "m1-cluster"] {
        assert!(!cluster_generic(name), "{name}");
    }
}

#[test]
fn a_generic_cluster_name_is_caught() {
    for name in ["cluster", "my-cluster", "team", "new", "test-cluster-1"] {
        assert!(cluster_generic(name), "{name}");
    }
}

#[test]
fn a_refusal_names_the_name_and_carries_the_rule() {
    assert!(machine_refusal("server").contains(MACHINE_RULE));
    assert!(machine_refusal("server").contains("\"server\""));
    assert!(cluster_refusal("team").contains(CLUSTER_RULE));
}
