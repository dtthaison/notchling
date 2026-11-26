use std::path::Path;

use notchling_engine::inbox::{Action, ActionId, AlertAction, Item, Kind, Place, State};
use notchling_engine::sessions::AgentGroup;
use notchling_engine::transcript::snapshot::PendingQuestion;
use serde_json::{json, Value};

use super::layout::{collapsed_size, NotchState, Rect};
use super::model::{Card, NotchModel};
use super::perch::{self, AxFrame, Perch, PetMode};
use super::pet::{Motion, Track};
use super::requests::{HookRequest, RequestKind};
use super::source::{fixture_clock, DataRoot, Source};
use crate::acting::Acting;
use crate::views::ViewSpec;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Collapsed,
    Peek,
    List,
    Detail,
    Question,
    Permission,
    QuestionTabs,
    PermissionActions,
    Pet,
    Perch,
    Inbox,
    Blocking,
    BlockingMany,
    BlockingClosed,
    BlockingGlance,
    BlockingLastCall,
    BlockingChange,
    BlockingHook,
    BlockingSecret,
}

pub const INBOX_UNREAD: usize = 3;

pub const VIEWS: [ViewSpec; 19] = [
    ViewSpec {
        name: "notch-collapsed",
        size: (560.0, 96.0),
        about: "the notch closed around the camera (fixture data)",
    },
    ViewSpec {
        name: "notch-peek",
        size: (560.0, 200.0),
        about: "a peek at a session that just finished (fixture data)",
    },
    ViewSpec {
        name: "notch-list",
        size: (560.0, 300.0),
        about: "the open notch: tabs and rows with status pills (fixture data)",
    },
    ViewSpec {
        name: "notch-detail",
        size: (560.0, 560.0),
        about: "one session's last lines, read-only (fixture data)",
    },
    ViewSpec {
        name: "notch-question",
        size: (560.0, 600.0),
        about: "a session's question while acting is off: options disabled (fixture data)",
    },
    ViewSpec {
        name: "notch-permission",
        size: (560.0, 460.0),
        about: "a permission prompt peeking out while acting is off (fixture data)",
    },
    ViewSpec {
        name: "notch-question-tabs",
        size: (560.0, 600.0),
        about: "three questions in tabs, single and multi select, acting on (fixture data)",
    },
    ViewSpec {
        name: "notch-permission-actions",
        size: (560.0, 460.0),
        about: "a permission prompt with Allow, Always and Deny live (fixture data)",
    },
    ViewSpec {
        name: "notch-pet",
        size: (560.0, 120.0),
        about: "the pet rolling round the closed notch with the needs badge (fixture data)",
    },
    ViewSpec {
        name: "notch-perch",
        size: (560.0, 260.0),
        about: "Tab Perch: the idle pet on a stand-in window's top edge (fixture data)",
    },
    ViewSpec {
        name: "notch-inbox",
        size: (560.0, 300.0),
        about: "the open notch with the inbox badge, three unread (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking",
        size: (560.0, 300.0),
        about: "the notch open on one stopped task: red header, who, Allow, Deny, Change and Later (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking-many",
        size: (560.0, 300.0),
        about: "three stopped tasks: one card and a 1 of 3 stepper, soonest end first (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking-closed",
        size: (560.0, 96.0),
        about: "the notch closed while a task is stopped: ears with the wait and a red count (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking-glance",
        size: (560.0, 120.0),
        about: "the notch grown for a moment to name the stopped task and how long it waited (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking-last-call",
        size: (560.0, 300.0),
        about: "the last call: the seconds left to answer, with Allow and Deny (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking-change",
        size: (560.0, 340.0),
        about: "Change… opened on a stopped task: a one-line note field and Send under the buttons (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking-hook",
        size: (560.0, 340.0),
        about: "an agent's permission prompt in the blocking card: the command, Allow, Deny, Change… and Later (fixture data)",
    },
    ViewSpec {
        name: "notch-blocking-secret",
        size: (560.0, 300.0),
        about: "a secret use: Allow Once, Deny, Change… and Later (fixture data)",
    },
];

pub fn shown(name: &str) -> Option<Shown> {
    Some(match name {
        "notch-collapsed" => Shown::Collapsed,
        "notch-peek" => Shown::Peek,
        "notch-list" => Shown::List,
        "notch-detail" => Shown::Detail,
        "notch-question" => Shown::Question,
        "notch-permission" => Shown::Permission,
        "notch-question-tabs" => Shown::QuestionTabs,
        "notch-permission-actions" => Shown::PermissionActions,
        "notch-pet" => Shown::Pet,
        "notch-perch" => Shown::Perch,
        "notch-inbox" => Shown::Inbox,
        "notch-blocking" => Shown::Blocking,
        "notch-blocking-many" => Shown::BlockingMany,
        "notch-blocking-closed" => Shown::BlockingClosed,
        "notch-blocking-glance" => Shown::BlockingGlance,
        "notch-blocking-last-call" => Shown::BlockingLastCall,
        "notch-blocking-change" => Shown::BlockingChange,
        "notch-blocking-hook" => Shown::BlockingHook,
        "notch-blocking-secret" => Shown::BlockingSecret,
        _ => return None,
    })
}

pub struct Scene {
    pub model: NotchModel,
    pub acting: Acting,
    pub pet: Option<PetMode>,
    pub motion: Motion,
    pub perch: Option<Perch>,
    pub window: Option<AxFrame>,
    pub question: Option<PendingQuestion>,
    pub picks: Vec<(usize, usize)>,
    pub tab: usize,
}

pub const MESSAGE_ENV: &str = "NOTCHLING_RENDER_NOTCH_MESSAGE";

pub const PERCH_WINDOW: AxFrame = AxFrame {
    x: 60.0,
    y: 92.0,
    w: 380.0,
    h: 150.0,
};

fn tabs_request(session: &str) -> Option<HookRequest> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/notch-scenes/question-tabs.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    value["session_id"] = Value::String(session.to_owned());
    HookRequest::parse(&value)
}

fn blocking_item(id: &str, title: &str, now: i64, ends_in_ms: i64, stopped_ms: i64) -> Item {
    Item {
        id: id.to_owned(),
        kind: Kind::Consent,
        title: title.to_owned(),
        detail: "For this run only.".to_owned(),
        state: State::Open,
        created_ms: now - stopped_ms,
        waiting_since: Some(now - stopped_ms),
        expires_ms: Some(now + ends_in_ms),
        blocking: Some(true),
        alert_actions: vec![AlertAction::Allow, AlertAction::Deny, AlertAction::Change],
        place: Some(Place {
            session_name: Some("Claude Code".to_owned()),
            branch: Some("notchling".to_owned()),
            machine_name: Some("pr-prep-merge".to_owned()),
            ..Place::default()
        }),
        ..Item::default()
    }
}

fn secret_item(now: i64) -> Item {
    let action = |id, label: &str, primary| Action {
        id,
        label: label.to_owned(),
        primary,
    };
    Item {
        kind: Kind::SecretApproval,
        title: "Allow npm-token for cargo publish".to_owned(),
        actions: vec![
            action(ActionId::AllowOnce, "Allow Once", true),
            action(ActionId::AllowAlways, "Always Allow", false),
            action(ActionId::Deny, "Deny", false),
        ],
        alert_actions: vec![AlertAction::Deny, AlertAction::Open, AlertAction::Change],
        open: Some(json!({"vault_approval": "ticket-1"})),
        ..blocking_item(
            "b1",
            "Allow npm-token for cargo publish",
            now,
            9 * 60_000,
            60_000,
        )
    }
}

fn hook_request(now: i64) -> HookRequest {
    HookRequest {
        id: "r-hook".to_owned(),
        session_id: "s-hook".to_owned(),
        created: (now - 20_000) as f64 / 1000.0,
        kind: RequestKind::Permission,
        tool_name: "Bash".to_owned(),
        summary: "cargo test --workspace --locked".to_owned(),
        description: String::new(),
        can_always_allow: true,
        agy: None,
    }
}

fn blocking_scene(model: &mut NotchModel, shown: Shown) -> NotchState {
    let now = model.now_ms;
    let min = 60_000;
    let items = match shown {
        Shown::BlockingMany => vec![
            blocking_item("b1", "Turn on Machines", now, 12 * min, 2 * min),
            blocking_item("b2", "Run cargo test on m1", now, 5 * min, 3 * min),
            blocking_item("b3", "Stop the session pr-prep", now, 9 * min, min),
        ],
        Shown::BlockingLastCall => vec![blocking_item(
            "b1",
            "Turn on Machines",
            now,
            52_000,
            14 * min,
        )],
        Shown::BlockingGlance => vec![blocking_item("b1", "Turn on Machines", now, 14 * min, min)],
        Shown::BlockingClosed => vec![blocking_item("b1", "Turn on Machines", now, 14 * min, min)],
        Shown::BlockingHook => Vec::new(),
        Shown::BlockingSecret => vec![secret_item(now)],
        _ => vec![blocking_item(
            "b1",
            "Turn on Machines",
            now,
            14 * min,
            4_000,
        )],
    };
    model.requests.clear();
    if shown == Shown::BlockingHook {
        model
            .requests
            .insert("s-hook".to_owned(), hook_request(now));
    }
    model.set_inbox_items(&items, now);
    model.blocking.farewell = None;
    if shown != Shown::BlockingMany {
        model.blocking.select("b1");
    }
    model.blocking.shown_ms = now - 10_000;
    if shown == Shown::BlockingChange {
        model.blocking.open_note("b1");
        model
            .blocking
            .set_draft("b1", "Use the staging key instead");
    }
    match shown {
        Shown::BlockingClosed => NotchState::Collapsed,
        Shown::BlockingGlance => NotchState::Glance,
        _ => NotchState::Blocking,
    }
}

pub fn model(shown: Shown) -> NotchModel {
    scene(shown).model
}

pub fn scene(shown: Shown) -> Scene {
    let mut source = Source::keeping_everything(DataRoot::for_render());
    let first = source.load(0);
    let now = fixture_clock(&first.agents);
    let loaded = source.load(now);
    let mut model = NotchModel::default();
    model.now_ms = now;
    model.update(loaded.agents, loaded.snapshots, loaded.requests);
    if std::env::var_os(MESSAGE_ENV).is_some() {
        for group in [AgentGroup::Working, AgentGroup::Completed] {
            if let Some(agent) = model.agents.iter().find(|a| a.group == group) {
                model.message_needs.insert(agent.id.clone());
            }
        }
    }
    let pick = |model: &NotchModel, wanted: &dyn Fn(&NotchModel, &str) -> bool| {
        model
            .agents
            .iter()
            .find(|a| wanted(model, &a.id))
            .map(|a| a.id.clone())
    };
    let mut scene = Scene {
        model: NotchModel::default(),
        acting: Acting::Off,
        pet: None,
        motion: Motion::default(),
        perch: None,
        window: None,
        question: None,
        picks: Vec::new(),
        tab: 0,
    };
    let state = match shown {
        Shown::Collapsed => Some(NotchState::Collapsed),
        Shown::List => Some(NotchState::List),
        Shown::Inbox => {
            model.inbox_unread = INBOX_UNREAD;
            Some(NotchState::List)
        }
        Shown::Peek => pick(&model, &|m, id| {
            m.agent(id)
                .is_some_and(|a| a.group == AgentGroup::Completed)
                && m.snapshot(id).is_some_and(|s| s.last_reply().is_some())
        })
        .map(NotchState::Peek),
        Shown::Detail => pick(&model, &|m, id| {
            m.agent(id).is_some_and(|a| a.group == AgentGroup::Working)
                && m.snapshot(id).is_some_and(|s| !s.lines.is_empty())
        })
        .map(NotchState::Detail),
        Shown::Question => pick(&model, &|m, id| {
            matches!(m.card(id), Some(Card::Question(_)))
        })
        .map(NotchState::Detail),
        Shown::Permission | Shown::PermissionActions => {
            scene.acting = if shown == Shown::Permission {
                Acting::Off
            } else {
                Acting::On
            };
            pick(&model, &|m, id| {
                matches!(m.card(id), Some(Card::Permission(_)))
            })
            .map(NotchState::Peek)
        }
        Shown::QuestionTabs => {
            scene.acting = Acting::On;
            let id = pick(&model, &|m, id| {
                m.agent(id)
                    .is_some_and(|a| a.group == AgentGroup::NeedsInput)
                    && m.permission(id).is_none()
            })
            .or_else(|| model.agents.first().map(|a| a.id.clone()));
            if let Some(request) = id.as_deref().and_then(tabs_request) {
                scene.question = request.question().cloned();
                model.requests.insert(request.session_id.clone(), request);
            }
            scene.picks = vec![(0, 1), (1, 1), (1, 3)];
            scene.tab = 1;
            id.map(NotchState::Peek)
        }
        Shown::Blocking
        | Shown::BlockingMany
        | Shown::BlockingClosed
        | Shown::BlockingGlance
        | Shown::BlockingLastCall
        | Shown::BlockingChange
        | Shown::BlockingHook
        | Shown::BlockingSecret => {
            scene.acting = Acting::On;
            Some(blocking_scene(&mut model, shown))
        }
        Shown::Pet => {
            scene.pet = Some(PetMode::Walking);
            let track = Track::new(collapsed_size(model.notch));
            scene.motion = Motion {
                d: track.length() * 0.58,
                heading: 1.0,
                turn: None,
            };
            Some(NotchState::Collapsed)
        }
        Shown::Perch => {
            scene.pet = Some(PetMode::Idle);
            let screen = Rect::new(0.0, 0.0, 560.0, 260.0);
            scene.perch = perch::find(PERCH_WINDOW, false, screen, 280.0, 260.0);
            scene.window = Some(PERCH_WINDOW);
            Some(NotchState::Collapsed)
        }
    };
    model.go(state.unwrap_or(NotchState::List));
    if let Ok(how) = std::env::var(MESSAGE_ENV) {
        model.tab = if how == "done" {
            AgentGroup::Completed
        } else {
            AgentGroup::Working
        };
    }
    scene.model = model;
    scene
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_view_has_a_state_and_the_fixture_fills_it() {
        for spec in VIEWS {
            let shown = shown(spec.name).unwrap();
            let m = model(shown);
            let size = m.size(&m.state);
            assert!(
                size.w <= f64::from(spec.size.0) && size.h <= f64::from(spec.size.1),
                "{}: {size:?} doesn't fit {:?}",
                spec.name,
                spec.size
            );
            let expected = match shown {
                Shown::Collapsed | Shown::Pet | Shown::Perch => {
                    matches!(m.state, NotchState::Collapsed)
                }
                Shown::List | Shown::Inbox => m.state == NotchState::List,
                Shown::Blocking
                | Shown::BlockingMany
                | Shown::BlockingLastCall
                | Shown::BlockingChange
                | Shown::BlockingHook
                | Shown::BlockingSecret => m.state == NotchState::Blocking,
                Shown::BlockingClosed => m.state == NotchState::Collapsed,
                Shown::BlockingGlance => m.state == NotchState::Glance,
                Shown::Peek
                | Shown::Permission
                | Shown::PermissionActions
                | Shown::QuestionTabs => matches!(m.state, NotchState::Peek(_)),
                Shown::Detail | Shown::Question => matches!(m.state, NotchState::Detail(_)),
            };
            assert!(expected, "{}: {:?}", spec.name, m.state);
        }
        assert_eq!(shown("notch"), None);
    }

    #[test]
    fn the_question_and_permission_renders_show_their_cards() {
        let q = model(Shown::Question);
        let NotchState::Detail(id) = &q.state else {
            panic!("{:?}", q.state)
        };
        assert!(matches!(q.card(id), Some(Card::Question(_))));
        for shown in [Shown::Permission, Shown::PermissionActions] {
            let p = scene(shown);
            let NotchState::Peek(id) = &p.model.state else {
                panic!("{:?}", p.model.state)
            };
            assert!(matches!(p.model.card(id), Some(Card::Permission(_))));
            let want = if shown == Shown::Permission {
                Acting::Off
            } else {
                Acting::On
            };
            assert_eq!(p.acting, want);
        }
    }

    #[test]
    fn the_tabs_scene_holds_three_questions_with_picks_to_make() {
        let s = scene(Shown::QuestionTabs);
        let NotchState::Peek(id) = &s.model.state else {
            panic!("{:?}", s.model.state)
        };
        let Some(Card::Question(q)) = s.model.card(id) else {
            panic!("no question card")
        };
        assert_eq!(q.items.len(), 3);
        assert!(q.items[1].multi_select);
        assert_eq!(Some(q), s.question);
        assert_eq!((s.tab, s.acting), (1, Acting::On));
    }

    #[test]
    fn only_the_inbox_render_has_unread_items() {
        assert_eq!(model(Shown::Inbox).inbox_unread, INBOX_UNREAD);
        assert_eq!(model(Shown::List).inbox_unread, 0);
    }

    #[test]
    fn the_perch_scene_sits_on_its_window() {
        let s = scene(Shown::Perch);
        let p = s.perch.expect("a perch");
        assert_eq!(p.y, PERCH_WINDOW.y);
        assert_eq!(p.mid_x() + 280.0, PERCH_WINDOW.x + 92.0);
        assert_eq!(s.pet, Some(PetMode::Idle));
        let pet = scene(Shown::Pet);
        assert_eq!(pet.pet, Some(PetMode::Walking));
        assert!(pet.motion.d > 0.0);
    }
}
