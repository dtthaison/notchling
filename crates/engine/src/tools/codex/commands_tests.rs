use super::*;
use std::path::PathBuf;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nl-codex-cmds-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn with_prompt(name: &str) -> Vec<SlashCommand> {
    let mut commands = all();
    commands.push(SlashCommand {
        name: name.into(),
        description: String::new(),
        argument_hint: None,
    });
    commands
}

#[test]
fn a_command_is_found_by_its_first_word() {
    let commands = with_prompt("prompts:fix");
    assert_eq!(
        command_in("/review focus on auth", &commands),
        Some("review".into())
    );
    assert_eq!(
        command_in("/prompts:fix", &commands),
        Some("prompts:fix".into())
    );
    assert_eq!(
        command_in("/notacommand", &commands),
        None,
        "sent as a message"
    );
    assert_eq!(command_in("review this", &commands), None);
    assert_eq!(command_in("/", &commands), None);
    assert_eq!(command_in("", &commands), None);
}

#[test]
fn every_command_goes_where_chat_sends_it() {
    assert_eq!(route("model"), Route::ModelPicker);
    assert_eq!(route("new"), Route::NewSession);
    assert_eq!(route("clear"), Route::NewSession);
    assert_eq!(route("delete"), Route::Delete);
    assert_eq!(route("quit"), Route::Ignore);
    assert_eq!(route("exit"), Route::Ignore);
    assert_eq!(route("review"), Route::Terminal);
    assert_eq!(route("prompts:fix"), Route::Terminal);
}

#[test]
fn the_terminal_runs_codex_on_the_session() {
    assert_eq!(script("abc"), "codex resume abc");
    assert_eq!(script(""), "codex");
    // An id that is not one never reaches a shell line.
    assert_eq!(script("abc; rm -rf ~"), "codex");
}

#[test]
fn the_list_is_codex_0_166s() {
    let commands = all();
    assert_eq!(commands.len(), 42);
    assert_eq!(commands[0].name, "model");
    assert_eq!(commands.last().map(|c| c.name.as_str()), Some("quit"));
    assert!(commands.iter().all(|c| c.argument_hint.is_none()));
    let mut names: Vec<_> = commands.iter().map(|c| c.name.clone()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), 42, "no name twice");
}

#[test]
fn a_prompts_description_is_its_front_matter_or_first_line() {
    assert_eq!(
        prompt_description("---\ndescription: \"Fix the bug\"\n---\nDo it"),
        Some("Fix the bug".into())
    );
    assert_eq!(
        prompt_description("Review the PR\nmore"),
        Some("Review the PR".into())
    );
    assert_eq!(
        prompt_description("\n\n  Spaced  \n"),
        Some("Spaced".into())
    );
    assert_eq!(prompt_description(""), None);
    assert_eq!(prompt_description("---\n---\n"), None);
    // Front matter without a description falls to the next line, as the Swift app does.
    assert_eq!(
        prompt_description("---\nname: x\n---\nBody"),
        Some("name: x".into())
    );
    // Only the front matter is searched for the key.
    assert_eq!(
        prompt_description("---\nname: x\n---\ndescription: later"),
        Some("name: x".into())
    );
    let long = "é".repeat(100);
    assert_eq!(
        prompt_description(&long).map(|d| d.chars().count()),
        Some(80)
    );
}

#[test]
fn saved_prompts_follow_codexs_own_commands_sorted() {
    let home = temp("prompts");
    let dir = home.join("prompts");
    std::fs::create_dir_all(&dir).expect("prompts");
    std::fs::write(dir.join("b.md"), "---\ndescription: Bee\n---\nx").expect("write");
    std::fs::write(dir.join("a.md"), "Alpha prompt").expect("write");
    std::fs::write(dir.join("empty.md"), "").expect("write");
    std::fs::write(dir.join("notes.txt"), "not a prompt").expect("write");
    let commands = available(&home);
    assert_eq!(commands.len(), 42 + 3);
    let tail: Vec<_> = commands[42..]
        .iter()
        .map(|c| (c.name.as_str(), c.description.as_str()))
        .collect();
    assert_eq!(
        tail,
        [
            ("prompts:a", "Alpha prompt"),
            ("prompts:b", "Bee"),
            ("prompts:empty", "your saved prompt")
        ]
    );
    // No prompts folder is just Codex's own.
    assert_eq!(available(&home.join("missing")).len(), 42);
    let _ = std::fs::remove_dir_all(&home);
}
