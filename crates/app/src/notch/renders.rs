//! The notch's views for `--render` (part U10): one per state, plus a question and a permission,
//! each over the fixture data root (`source::DataRoot::for_render`), never the real home.

use notchling_engine::sessions::AgentGroup;

use super::layout::NotchState;
use super::model::{Card, NotchModel};
use super::source::{fixture_clock, DataRoot, Source};
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
    /// A session asking a question, open, with its options disabled.
    Question,
    /// A permission prompt peeking out, with its options disabled.
    Permission,
}

/// Width fits the canvas; heights fit each state's shape with a margin.
pub const VIEWS: [ViewSpec; 6] = [
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
        size: (560.0, 560.0),
        about: "a session's question with its options disabled (fixture data)",
    },
    ViewSpec {
        name: "notch-permission",
        size: (560.0, 420.0),
        about: "a permission prompt peeking out, options disabled (fixture data)",
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
        _ => return None,
    })
}

/// The model over the render's data root, on the fixture's fixed clock, showing `shown`. The
/// session each state shows is picked by rule, so another fixture root works too.
pub fn model(shown: Shown) -> NotchModel {
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
    let state = match shown {
        Shown::Collapsed => Some(NotchState::Collapsed),
        Shown::List => Some(NotchState::List),
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
        Shown::Permission => pick(&model, &|m, id| {
            matches!(m.card(id), Some(Card::Permission(_)))
        })
        .map(NotchState::Peek),
    };
    // A fixture without such a session shows the list rather than nothing.
    model.go(state.unwrap_or(NotchState::List));
    model
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
                Shown::Collapsed => matches!(m.state, NotchState::Collapsed),
                Shown::List => m.state == NotchState::List,
                Shown::Peek | Shown::Permission => matches!(m.state, NotchState::Peek(_)),
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
        let p = model(Shown::Permission);
        let NotchState::Peek(id) = &p.state else {
            panic!("{:?}", p.state)
        };
        assert!(matches!(p.card(id), Some(Card::Permission(_))));
    }
}
