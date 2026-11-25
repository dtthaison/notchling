use super::{known_color, Document, Problem, COLORS, DEFAULT_COLOR};

#[test]
fn the_default_is_one_of_the_choices_and_names_are_unique() {
    assert!(COLORS.contains(&DEFAULT_COLOR));
    let mut sorted = COLORS.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), COLORS.len());
    assert!(COLORS.iter().all(|c| known_color(c)));
}

#[test]
fn a_new_organization_or_a_colour_change_takes_only_a_palette_name() {
    let mut doc = Document::personal_only();
    assert_eq!(
        doc.create("Acme", None, Some("chartreuse")),
        Err(vec![Problem::ColorInvalid("chartreuse".into())])
    );
    let id = doc.create("Acme", None, Some(" purple ")).unwrap();
    assert_eq!(doc.find(&id).unwrap().color, "purple");
    assert_eq!(
        doc.set_color(&id, "chartreuse"),
        Err(Problem::ColorInvalid("chartreuse".into()))
    );
    assert_eq!(
        doc.set_color(&id, "RED"),
        Err(Problem::ColorInvalid("RED".into()))
    );
    assert!(doc.set_color(&id, "red").is_ok());
}

#[test]
fn a_stored_colour_the_palette_lacks_still_reads_and_other_edits_still_save() {
    let text = r#"{"version":1,"organizations":[{"id":"personal","name":"Personal","color":"grey","folders":[]},{"id":"acme","name":"Acme","color":"mauve","folders":[]}]}"#;
    let mut doc = Document::parse(text).unwrap();
    assert!(doc.rename("acme", "Acme Inc").is_ok());
    assert!(doc.problems().is_empty());
}
