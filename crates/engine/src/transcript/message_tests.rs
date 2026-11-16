use super::*;

fn exists(path: &Path) -> bool {
    path == Path::new("/tmp/Shot 1.png") || path == Path::new("/tmp/a.png")
}

#[test]
fn parse_cuts_picture_paths_and_squeezes() {
    let message = Message::parse("look  at\nthis\n\n'/tmp/Shot 1.png'", exists);
    assert_eq!(message.text, "look at this");
    assert_eq!(message.pictures, 1);
    assert_eq!(message.bash, None);
    assert_eq!(message.command, None);
}

#[test]
fn parse_keeps_a_path_that_does_not_exist() {
    let message = Message::parse("see /tmp/gone.png now", exists);
    assert_eq!(message.text, "see /tmp/gone.png now");
    assert_eq!(message.pictures, 0);
}

#[test]
fn parse_bash_and_command() {
    let bash = Message::parse("!git branch", exists);
    assert_eq!(bash.bash.as_deref(), Some("git branch"));
    assert_eq!(bash.command, None);
    let command = Message::parse("/model opus", exists);
    assert_eq!(
        command.command,
        Some(("model".to_string(), "opus".to_string()))
    );
    assert_eq!(command.bash, None);
    let bare = Message::parse("/clear", exists);
    assert_eq!(bare.command, Some(("clear".to_string(), String::new())));
    let spaced = Message::parse("/effort   high\n", exists);
    assert_eq!(
        spaced.command,
        Some(("effort".to_string(), "high".to_string()))
    );
}

#[test]
fn split_pictures_returns_the_words_and_the_paths() {
    let (words, pictures) = split_pictures("a '/tmp/Shot 1.png' b /tmp/gone.png", exists);
    assert_eq!(words, "a b /tmp/gone.png");
    assert_eq!(pictures, vec![PathBuf::from("/tmp/Shot 1.png")]);
}

#[test]
fn chips_are_cut_from_a_transcript_line() {
    assert_eq!(cut_chips("[Image #20]why? [Image #3]"), "why? ");
    assert_eq!(cut_chips("[Image #x] [Image #]"), "[Image #x] [Image #]");
}
