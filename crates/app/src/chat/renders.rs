//! Chat's own views for `--render` that aren't the window: a probe of GPUI's `list` (Task 2.1 of
//! the Chat state split), kept as a regression probe.

use crate::views::ViewSpec;

pub const VIEWS: [ViewSpec; 1] = [ViewSpec {
    name: "chat-list-probe",
    size: (900.0, 700.0),
    about: "GPUI's list over 300 made-up rows: follow, anchor, cost, resize, spinners (set NOTCHLING_RENDER_BENCH=<seconds> to run its checks and print list-probe)",
}];
