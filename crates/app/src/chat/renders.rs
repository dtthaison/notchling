use crate::views::ViewSpec;

pub const VIEWS: [ViewSpec; 2] = [
    ViewSpec {
    name: "chat-list-probe",
    size: (900.0, 700.0),
    about: "GPUI's list over 300 made-up rows: follow, anchor, cost, resize, spinners (set NOTCHLING_RENDER_BENCH=<seconds> to run its checks and print list-probe)",
    },
    ViewSpec {
        name: "chat-git",
        size: (1240.0, 780.0),
        about: "Chat over the fixture with made-up git data in the Details Panel (NOTCHLING_RENDER_GIT_DIFF=1 opens a file's diff in the file panel)",
    },
];
