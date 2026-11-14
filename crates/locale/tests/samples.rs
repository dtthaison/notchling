use notchling_locale::{shipped, testing, Arg};

const SAMPLES: [i64; 9] = [0, 1, 2, 3, 5, 11, 21, 101, 1000];

#[test]
fn every_message_formats_with_sample_numbers() {
    for tag in shipped() {
        for (key, numeric) in testing::messages(tag) {
            for sample in SAMPLES {
                let args: Vec<Arg<'_>> = numeric
                    .iter()
                    .map(|n| {
                        if *n {
                            Arg::Num(sample.into())
                        } else {
                            Arg::Str("x")
                        }
                    })
                    .collect();
                let out = testing::format_plain_numbers(tag, key, &args)
                    .unwrap_or_else(|| panic!("{tag}: {key} has no body"));
                assert!(!out.is_empty(), "{tag}: {key} with {sample} is empty");
            }
        }
    }
}

#[test]
fn every_plural_category_used_exists() {
    for tag in shipped() {
        let reachable = testing::reachable_categories(tag);
        for (key, used) in testing::plural_categories(tag) {
            for category in used {
                assert!(
                    reachable.contains(&category),
                    "{tag}: {key} has [{category}], which {tag} never uses (it uses {reachable:?})"
                );
            }
        }
    }
}

#[test]
fn english_reaches_one_and_other() {
    let reachable = testing::reachable_categories("en");
    assert!(reachable.contains(&"one") && reachable.contains(&"other"));
    assert!(!reachable.contains(&"few"));
    assert!(testing::reachable_categories("pl").contains(&"few"));
}
