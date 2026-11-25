use super::tests::rig;
use notchling_engine::settings::spec;
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[test]
fn a_written_search_key_pokes_the_search_and_others_do_not() {
    let rig = rig("search-poke");
    let pokes = Arc::new(AtomicUsize::new(0));
    let counted = pokes.clone();
    rig.ops.bind_search_poke(Arc::new(move || {
        counted.fetch_add(1, Ordering::SeqCst);
    }));
    let writer = rig.ops.writer();
    writer
        .write(
            spec("searchIndexRange").unwrap(),
            &json!("14d"),
            "agent",
            None,
        )
        .unwrap();
    writer
        .write(
            spec("searchCategories").unwrap(),
            &json!(["pages"]),
            "agent",
            None,
        )
        .unwrap();
    assert_eq!(pokes.load(Ordering::SeqCst), 2);
    writer
        .write(
            spec("searchCategoryOrder").unwrap(),
            &json!(["pages"]),
            "agent",
            None,
        )
        .unwrap();
    writer
        .write(spec("showPet").unwrap(), &json!(false), "agent", None)
        .unwrap();
    assert_eq!(pokes.load(Ordering::SeqCst), 2);
}
