use notchling_engine::transcript::shells::{lsof_args, parse_writers_by_name};

fn wanted(paths: &[&str]) -> Vec<(String, Vec<String>)> {
    paths
        .iter()
        .map(|p| ((*p).to_owned(), vec![(*p).to_owned()]))
        .collect()
}

#[test]
fn one_call_lists_every_path_after_the_dashes() {
    let paths = vec!["/tmp/a/b1.output".to_owned(), "/tmp/a/b2.output".to_owned()];
    assert_eq!(
        lsof_args(&paths),
        [
            "-w",
            "-F",
            "pcan",
            "--",
            "/tmp/a/b1.output",
            "/tmp/a/b2.output"
        ]
    );
    assert_eq!(lsof_args(&[]), ["-w", "-F", "pcan", "--"]);
}

#[test]
fn each_path_gets_the_writers_of_its_own_file() {
    let text = "p4010\ncsleep\nf1\naw\nn/tmp/a/b1.output\n\
                p4011\nczsh\nf1\nau\nn/tmp/a/b2.output\nf2\naw\nn/tmp/a/b2.output\n\
                p4012\ncsleep\nf1\naw\nn/tmp/a/b1.output\n";
    let got = parse_writers_by_name(text, &wanted(&["/tmp/a/b1.output", "/tmp/a/b2.output"]));
    assert_eq!(got["/tmp/a/b1.output"], [4010, 4012]);
    assert_eq!(
        got["/tmp/a/b2.output"],
        [4011],
        "one process, two descriptors"
    );
}

#[test]
fn readers_claude_and_a_path_nobody_has_open_are_not_writers() {
    let text = "p600\nctail\nf3\nar\nn/tmp/a/b1.output\n\
                p500\ncclaude\nf9\naw\nn/tmp/a/b1.output\n\
                p501\ncnode\nf9\naw\nn/tmp/a/b1.output\n\
                p502\ncNotchling\nf9\naw\nn/tmp/a/b1.output\n";
    let got = parse_writers_by_name(text, &wanted(&["/tmp/a/b1.output", "/tmp/a/b3.output"]));
    assert!(got["/tmp/a/b1.output"].is_empty());
    assert!(got["/tmp/a/b3.output"].is_empty(), "listed, with no entry");
    assert_eq!(got.len(), 2);
}

#[test]
fn a_real_path_stands_for_the_path_it_was_asked_with() {
    let text = "p4010\ncsleep\nf1\naw\nn/private/tmp/a/b1.output\n";
    let mut asked = wanted(&["/tmp/a/b1.output"]);
    asked[0].1.push("/private/tmp/a/b1.output".to_owned());
    assert_eq!(
        parse_writers_by_name(text, &asked)["/tmp/a/b1.output"],
        [4010]
    );
}

#[test]
fn a_name_that_matches_only_one_file_name_still_counts() {
    let text = "p4010\ncsleep\nf1\naw\nn/private/tmp/other/b1.output\n";
    let got = parse_writers_by_name(text, &wanted(&["/tmp/a/b1.output", "/tmp/a/b2.output"]));
    assert_eq!(got["/tmp/a/b1.output"], [4010]);
    assert!(got["/tmp/a/b2.output"].is_empty());

    let twice = parse_writers_by_name(text, &wanted(&["/tmp/a/b1.output", "/tmp/z/b1.output"]));
    assert!(
        twice["/tmp/a/b1.output"].is_empty() && twice["/tmp/z/b1.output"].is_empty(),
        "two shells with one file name: not guessed"
    );
}

#[test]
fn what_lsof_says_when_nothing_is_open_is_every_path_empty() {
    let got = parse_writers_by_name("", &wanted(&["/tmp/a/b1.output"]));
    assert!(got["/tmp/a/b1.output"].is_empty());
}

#[test]
fn a_call_for_one_path_gives_every_writer_it_lists_to_that_path() {
    let escaped = "p4010\ncsleep\nf1\naw\nn/tmp/a/my\\x20job.output\n\
                   p4011\ncsleep\nf4\nau\nn/private/var/elsewhere/renamed\n\
                   p4012\nctail\nf3\nar\nn/tmp/a/my\\x20job.output\n\
                   p500\ncclaude\nf9\naw\nn/tmp/a/my\\x20job.output\n";
    let got = parse_writers_by_name(escaped, &wanted(&["/tmp/a/my job.output"]));
    assert_eq!(
        got["/tmp/a/my job.output"],
        [4010, 4011],
        "lsof only lists the file it was asked for, whatever it calls it; readers and claude still don't count"
    );
}

#[test]
fn with_two_paths_an_escaped_name_is_still_not_guessed() {
    let text = "p4010\ncsleep\nf1\naw\nn/tmp/a/my\\x20job.output\n";
    let got = parse_writers_by_name(
        text,
        &wanted(&["/tmp/a/my job.output", "/tmp/other/b2.output"]),
    );
    assert!(got["/tmp/a/my job.output"].is_empty());
    assert!(got["/tmp/other/b2.output"].is_empty());
}
