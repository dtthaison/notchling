use super::*;
use crate::contract::channels::{ACP, APP, REMOTE, SIDECAR};
use crate::contract::{Registry, Support};
use crate::settings;

#[test]
fn every_tool_has_a_first_channel_that_sends_for_itself() {
    for tool in Tool::ALL {
        let all = of(tool);
        assert!(!all.is_empty(), "{tool:?}");
        assert!(all.iter().all(|c| c.tool() == tool));
        assert_eq!(all[0].mode(), Mode::Sends, "{tool:?}");
        for ability in [
            Ability::Reply,
            Ability::Start,
            Ability::Phone,
            Ability::Stop,
        ] {
            assert!(all[0].abilities().has(ability), "{tool:?} {ability:?}");
        }
    }
}

#[test]
fn only_antigravity_has_a_choice_and_agy_acp_is_the_default() {
    let choosing: Vec<Tool> = Tool::ALL.into_iter().filter(|t| chooses(*t)).collect();
    assert_eq!(choosing, [Tool::Antigravity]);
    assert_eq!(chosen(Tool::Antigravity, None).id(), ACP);
    assert_eq!(chosen(Tool::Antigravity, Some("nonsense")).id(), ACP);
    assert_eq!(chosen(Tool::Antigravity, Some(APP)).id(), APP);
    assert_eq!(chosen(Tool::Claude, Some(APP)).tool(), Tool::Claude);
    assert_eq!(chosen(Tool::Codex, None).tool(), Tool::Codex);
}

#[test]
fn the_setting_takes_exactly_the_channel_ids_and_defaults_to_the_first() {
    for tool in Tool::ALL {
        let key = setting_key(tool);
        assert_eq!(key.is_some(), chooses(tool), "{tool:?}");
        let Some(key) = key else { continue };
        let spec = settings::spec(key).expect("a setting for the choice");
        let ids: Vec<&str> = of(tool).iter().map(|c| c.id()).collect();
        assert_eq!(spec.allowed, ids.as_slice());
        assert_eq!(spec.default_value(), Some(serde_json::json!(ids[0])));
    }
}

#[test]
fn the_app_draft_hands_the_text_over_and_cannot_be_used_from_the_phone() {
    let app = chosen(Tool::Antigravity, Some(APP));
    assert_eq!(app.mode(), Mode::NewDraft);
    assert!(app.mode().user_sends());
    assert!(refusal(app, Ability::Reply).is_none());
    assert!(refusal(app, Ability::Start).is_none());
    assert_eq!(
        refusal(app, Ability::Phone).unwrap(),
        "Antigravity can't be used from the phone while Notchling talks to it through the Antigravity app (a draft you send there)."
    );
    assert!(refusal(app, Ability::Stop).is_some());
}

#[test]
fn remote_control_opens_the_existing_session_and_cannot_be_used_from_the_phone() {
    let remote = chosen(Tool::Antigravity, Some(REMOTE));
    assert_eq!(remote.mode(), Mode::ExistingSession);
    assert!(remote.mode().user_sends());
    assert!(refusal(remote, Ability::Reply).is_none());
    assert!(refusal(remote, Ability::Start).is_none());
    assert!(refusal(remote, Ability::Phone).is_some());
    assert!(refusal(remote, Ability::Stop).is_some());
}

#[test]
fn the_agy_channel_refuses_nothing() {
    let acp = chosen(Tool::Antigravity, None);
    assert!(!acp.mode().user_sends());
    for ability in Ability::ALL {
        if ability == Ability::Answer {
            continue;
        }
        assert!(refusal(acp, ability).is_none(), "{ability:?}");
    }
}

#[test]
fn a_channel_that_claims_an_ability_sits_on_a_capability_the_tool_has() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        for channel in of(tool) {
            for ability in Ability::ALL {
                let Some(capability) = ability.capability() else {
                    continue;
                };
                if channel.abilities().has(ability) {
                    assert!(
                        !matches!(
                            registry.support(tool, capability),
                            Support::NotApplicable { .. }
                        ),
                        "{} {}",
                        channel.id(),
                        capability.id()
                    );
                }
            }
        }
    }
}

#[test]
fn the_registry_lists_a_providers_channels() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        let ids: Vec<&str> = registry.channels(tool).iter().map(|c| c.id()).collect();
        let all: Vec<&str> = of(tool).iter().map(|c| c.id()).collect();
        assert_eq!(ids, all);
    }
}

#[test]
fn the_sidecar_channel_sends_for_itself_and_soft_stops_but_cannot_hard_stop_or_answer_questions() {
    let sidecar = chosen(Tool::Antigravity, Some(SIDECAR));
    assert_eq!(sidecar.id(), SIDECAR);
    assert_eq!(sidecar.title(), "Antigravity app (full)");
    assert_eq!(sidecar.mode(), Mode::Sends);
    assert!(!sidecar.mode().user_sends());
    for ability in [
        Ability::Reply,
        Ability::Start,
        Ability::Phone,
        Ability::SoftStop,
        Ability::Approve,
    ] {
        assert!(refusal(sidecar, ability).is_none(), "{ability:?}");
    }
    assert_eq!(
        sidecar.abilities().missing(),
        vec![Ability::Stop, Ability::Answer]
    );
    assert_eq!(
        refusal(sidecar, Ability::Stop).unwrap(),
        "Antigravity can't be stopped from Notchling while Notchling talks to it through Antigravity app (full)."
    );
    assert!(refusal(sidecar, Ability::Answer).is_some());
}

#[test]
fn the_default_channel_does_everything_its_tool_can_and_the_others_do_less() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        for (i, channel) in of(tool).iter().enumerate() {
            for ability in Ability::ALL {
                let possible = ability.capability().is_none_or(|c| {
                    !matches!(registry.support(tool, c), Support::NotApplicable { .. })
                });
                if i == 0 {
                    assert_eq!(
                        channel.abilities().has(ability),
                        possible,
                        "{} {ability:?}",
                        channel.id()
                    );
                }
            }
            if i > 0 {
                assert_ne!(
                    channel.abilities(),
                    of(tool)[0].abilities(),
                    "{}",
                    channel.id()
                );
            }
        }
    }
    for id in [APP, REMOTE] {
        let c = chosen(Tool::Antigravity, Some(id));
        for ability in [Ability::SoftStop, Ability::Approve, Ability::Answer] {
            assert!(!c.abilities().has(ability), "{id} {ability:?}");
        }
    }
}

#[test]
fn every_ability_has_a_word_and_the_gaps_are_the_abilities_a_channel_lacks() {
    for ability in Ability::ALL {
        assert!(!ability.verb().is_empty());
    }
    assert!(Abilities::ALL.missing().is_empty());
    assert_eq!(Abilities::NONE.missing().len(), Ability::ALL.len());
}

#[test]
fn a_channel_that_sends_explains_every_ability_it_lacks_that_its_tool_has() {
    let registry = Registry::standard();
    for tool in Tool::ALL {
        for channel in of(tool) {
            if channel.mode().user_sends() {
                continue;
            }
            for ability in channel.abilities().missing() {
                let possible = ability.capability().is_none_or(|c| {
                    !matches!(registry.support(tool, c), Support::NotApplicable { .. })
                });
                let noted = channel.gap_notes().iter().any(|(a, _)| *a == ability);
                assert!(noted || !possible, "{} {ability:?}", channel.id());
            }
            for (ability, note) in channel.gap_notes() {
                assert!(!channel.abilities().has(*ability), "{}", channel.id());
                assert!(!note.is_empty());
            }
        }
    }
}
