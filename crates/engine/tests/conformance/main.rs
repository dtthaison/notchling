#[macro_use]
mod run;

use notchling_engine::contract::{Capability, Tool};

macro_rules! cell_test {
    ($test:ident, $variant:ident, $tool:expr, $name:literal) => {
        #[test]
        fn $test() {
            run::cell($tool, Capability::$variant, run::root($name));
        }
    };
}

macro_rules! conformance {
    ($module:ident, $tool:expr, $name:literal) => {
        mod $module {
            use super::{run, Capability, Tool};

            each_capability!(cell_test, $tool, $name);
        }
    };
}

conformance!(claude, Tool::Claude, "claude");
conformance!(codex, Tool::Codex, "codex");
conformance!(antigravity, Tool::Antigravity, "antigravity");

#[test]
fn summary() {
    println!("{}", run::summary());
}
