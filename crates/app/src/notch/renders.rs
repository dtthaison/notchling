//! The notch's views for `--render` (part U10): one per state, the cards, Peek, the pet and Tab
//! Perch, each over the fixture data root (`source::DataRoot::for_render`), never the real home.
//! A [`Scene`] adds what a fixture can't hold: whether acting is on, the pet's place, a window to
//! perch on, picks already made on a question.

use std::path::Path;

use notchling_engine::sessions::AgentGroup;
use notchling_engine::transcript::snapshot::PendingQuestion;
use serde_json::Value;

use super::layout::{collapsed_size, NotchState, Rect};
use super::model::{Card, NotchModel};
use super::perch::{self, AxFrame, Perch, PetMode};
use super::pet::{Motion, Track};
use super::requests::HookRequest;
use super::source::{fixture_clock, DataRoot, Source};
use crate::acting::Acting;
use crate::views::ViewSpec;

/// What a notch render shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shown {
    Collapsed,
    /// A finished session's last reply.
    Peek,
    List,
    /// A working session's last lines.
    Detail,
    /// A session asking a question, open, with acting off: its options disabled.
    Question,
    /// A permission prompt peeking out, with acting off.
    Permission,
    /// Three questions in tabs, the first answered and two picked on the multi-select second,
    /// with acting on.
    QuestionTabs,
    /// A permission prompt peeking out with Allow, Always and Deny live.
    PermissionActions,
    /// The pet rolling round the closed notch, towing the needs badge.
    Pet,
    /// Nothing going on: the pet sits on the frontmost window, right of its traffic lights.
    Perch,
    Inbox,
}

pub const INBOX_UNREAD: usize = 3;

/// Width fits the canvas; heights fit each state's shape with a margin.
pub const VIEWS: [ViewSpec; 11] = [
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
];

/// The render for a view name.
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
        _ => return None,
    })
}

/// A render's model and what goes with it.
pub struct Scene {
    pub model: NotchModel,
    pub acting: Acting,
    /// The pet's mode, forced; `None` follows the sessions.
    pub pet: Option<PetMode>,
    pub motion: Motion,
    pub perch: Option<Perch>,
    /// The stand-in window the perch is on, in the canvas (y down from its top).
    pub window: Option<AxFrame>,
    /// The question on show, and picks to make on it as (tab, option).
    pub question: Option<PendingQuestion>,
    pub picks: Vec<(usize, usize)>,
    /// The tab to show at the end.
    pub tab: usize,
}

/// The perch scene's window, in a canvas 560 wide whose camera is at its middle.
pub const PERCH_WINDOW: AxFrame = AxFrame {
    x: 60.0,
    y: 92.0,
    w: 380.0,
    h: 150.0,
};

/// The question-tabs scene's hook request, checked in beside the notch fixture.
fn tabs_request(session: &str) -> Option<HookRequest> {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/notch-scenes/question-tabs.json");
    let mut value: Value = serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
    value["session_id"] = Value::String(session.to_owned());
    HookRequest::parse(&value)
}

/// The model over the render's data root, on the fixture's fixed clock, showing `shown`. The
/// session each state shows is picked by rule, so another fixture root works too.
pub fn model(shown: Shown) -> NotchModel {
    scene(shown).model
}

/// The whole scene for a render.
pub fn scene(shown: Shown) -> Scene {
    let mut source = Source::keeping_everything(DataRoot::for_render());
    let first = source.load(0);
    let now = fixture_clock(&first.agents);
    let loaded = source.load(now);
    let mut model = NotchModel::default();
    model.now_ms = now;
    model.update(loaded.agents, loaded.snapshots, loaded.requests);
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
            // A session waiting on you without a permission held for it takes the questions.
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
            // Postgres, then Sign-in and Search on the multi-select page on show.
            scene.picks = vec![(0, 1), (1, 1), (1, 3)];
            scene.tab = 1;
            id.map(NotchState::Peek)
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
            // The canvas stands in for the screen: AX coordinates are the canvas's own.
            let screen = Rect::new(0.0, 0.0, 560.0, 260.0);
            scene.perch = perch::find(PERCH_WINDOW, false, screen, 280.0, 260.0);
            scene.window = Some(PERCH_WINDOW);
            Some(NotchState::Collapsed)
        }
    };
    // A fixture without such a session shows the list rather than nothing.
    model.go(state.unwrap_or(NotchState::List));
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
