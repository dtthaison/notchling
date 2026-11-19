use super::*;

fn new(text: &str) -> New {
    New {
        session: "idle0001".to_string(),
        tool: Tool::Claude,
        text: text.to_string(),
        options: None,
        via: Via::Chat,
        clear_box: false,
    }
}

#[test]
fn a_text_of_only_dropped_characters_is_not_saved() {
    let store = OutboxStore::in_memory_for_tests();
    for text in [
        "",
        " \n",
        "\u{200b}",
        "\u{feff}\u{2060}",
        "\u{3164} \u{115f}",
    ] {
        assert_eq!(
            store.insert(&new(text), 1_000).unwrap_err(),
            InsertError::Empty,
            "{text:?}"
        );
    }
    assert!(store.list(None).unwrap().is_empty());
}

#[test]
fn a_text_with_a_visible_character_is_saved_as_typed() {
    let store = OutboxStore::in_memory_for_tests();
    let text = "\u{200b}ok\u{200b}";
    let row = store.insert(&new(text), 1_000).unwrap();
    assert_eq!(row.text, text);
}
