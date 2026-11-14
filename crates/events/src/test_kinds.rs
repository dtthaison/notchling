#![allow(dead_code)]

use crate::field::{Count, Exe, List, Millis};
use crate::field_ref::{Item, Ref, Secret, Session};
use crate::spec::{AgentsRead, Durability, Group, Level, Lock, Proc};

kinds! {
    group: Group::Diagnostics,
    TestKind = "test.kind", kv 1 {
        title: "Test Kind",
        records: "A kind only the tests use.",
        why: "Exercises the macro and the writers.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY | DIAG,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            zeta: Count,
            alpha: bool,
            mid: Option<Count>,
        }
    }
    TestRefs = "test.refs", kv 1 {
        title: "Test Refs",
        records: "A kind with refs.",
        why: "Exercises how refs are written.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            session: Ref<Session>,
            item: Ref<Item>,
            exe: Option<Exe>,
            waited: Option<Millis>,
        }
    }
    TestNames = "test.names", kv 1 {
        title: "Test Names",
        records: "A kind with a list of refs.",
        why: "Exercises the list cut.",
        default: On,
        lock: Lock::Toggle,
        sinks: ACTIVITY,
        durability: Durability::Push,
        level: Level::Info,
        emitters: &[Proc::Daemon],
        agents_read: AgentsRead::No,
        keep_days: 30,
        fields {
            names: List<Ref<Secret>>,
        }
    }
}
