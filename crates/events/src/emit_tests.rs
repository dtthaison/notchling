use crate::emit::{Emit, MemorySink, NoEmit, Unwritable, UnwritableWhy};
use crate::event::Event;
use crate::field::Count;
use crate::spec::Outcome;
use crate::test_kinds::TestKind;

fn event() -> Event {
    Event::new(
        &TestKind {
            zeta: Count(1),
            alpha: true,
            mid: None,
        },
        Outcome::Ok,
        1,
    )
}

#[test]
fn memory_sink_records_pushes_and_commits() {
    let sink = MemorySink::new();
    sink.push(event());
    sink.push(event());
    assert!(sink.commit(event()).is_ok());
    assert_eq!(sink.events().len(), 3);
    assert_eq!(sink.commits(), 1);
    assert_eq!(sink.names(), vec!["test.kind"; 3]);
}

#[test]
fn memory_sink_can_fail_commits() {
    let sink = MemorySink::new();
    sink.fail_commits(true);
    assert_eq!(
        sink.commit(event()),
        Err(Unwritable {
            why: UnwritableWhy::WriteFailed
        })
    );
    assert!(sink.events().is_empty());
    assert_eq!(sink.commits(), 0);
    sink.fail_commits(false);
    assert!(sink.commit(event()).is_ok());
}

#[test]
fn no_emit_commits_ok() {
    let sink = NoEmit;
    sink.push(event());
    let done = sink.commit(event()).expect("commits");
    assert_eq!(done.seq, 0);
    assert_eq!(done.audit_n, None);
}
