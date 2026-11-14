use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use fluent_bundle::types::FluentNumber;
use fluent_bundle::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use fluent_syntax::ast;
use notchling_locale::Arg;
use unic_langid::LanguageIdentifier;

pub const SAMPLES: [i64; 9] = [0, 1, 2, 3, 5, 11, 21, 101, 1000];

pub fn files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(read) = fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<PathBuf> = read.filter_map(|e| e.ok().map(|e| e.path())).collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            files(&path, out);
        } else if path.extension().is_some_and(|e| e == "ftl") {
            out.push(path);
        }
    }
}

pub fn parse(path: &Path) -> FluentResource {
    let source = fs::read_to_string(path).expect("read ftl");
    match FluentResource::try_new(source) {
        Ok(resource) => resource,
        Err((_, errors)) => panic!("{}: {errors:?}", path.display()),
    }
}

fn inline_vars(inline: &ast::InlineExpression<&str>, out: &mut Vec<String>) {
    match inline {
        ast::InlineExpression::VariableReference { id } => {
            if !out.iter().any(|v| v == id.name) {
                out.push(id.name.to_string());
            }
        }
        ast::InlineExpression::FunctionReference { arguments, .. } => {
            for positional in &arguments.positional {
                inline_vars(positional, out);
            }
        }
        _ => {}
    }
}

fn pattern_vars(pattern: &ast::Pattern<&str>, out: &mut Vec<String>) {
    for element in &pattern.elements {
        if let ast::PatternElement::Placeable { expression } = element {
            match expression {
                ast::Expression::Inline(inline) => inline_vars(inline, out),
                ast::Expression::Select { selector, variants } => {
                    inline_vars(selector, out);
                    for variant in variants {
                        pattern_vars(&variant.value, out);
                    }
                }
            }
        }
    }
}

pub fn english_vars(root: &Path) -> HashMap<String, Vec<String>> {
    let mut paths = Vec::new();
    files(&root.join("en"), &mut paths);
    let mut out = HashMap::new();
    for path in paths {
        let resource = parse(&path);
        for entry in resource.entries() {
            if let ast::Entry::Message(message) = entry {
                let mut vars = Vec::new();
                if let Some(pattern) = &message.value {
                    pattern_vars(pattern, &mut vars);
                }
                out.insert(message.id.name.to_string(), vars);
            }
        }
    }
    out
}

fn brand_terms(root: &Path) -> String {
    let source = fs::read_to_string(root.join("en/shared.ftl")).expect("read shared");
    source
        .lines()
        .filter(|l| l.starts_with("-brand-"))
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}

pub fn bundle(root: &Path, tag: &str) -> FluentBundle<FluentResource> {
    let langid: LanguageIdentifier = tag.parse().expect("language tag");
    let mut bundle = FluentBundle::new(vec![langid]);
    bundle.set_use_isolating(false);
    bundle.add_builtins().expect("builtins");
    let mut paths = Vec::new();
    files(&root.join(tag), &mut paths);
    for path in paths {
        bundle.add_resource(parse(&path)).expect("add resource");
    }
    if tag != "en" {
        let terms = FluentResource::try_new(brand_terms(root)).expect("terms");
        bundle.add_resource(terms).expect("add terms");
    }
    bundle
}

pub type Ours<'a> = &'a dyn Fn(&str, &[Arg<'_>]) -> Option<String>;

pub fn compare(root: &Path, tag: &str, messages: Vec<(&str, Vec<bool>)>, ours: Ours<'_>) -> usize {
    let vars = english_vars(root);
    let bundle = bundle(root, tag);
    let mut compared = 0;
    for (key, numeric) in messages {
        let names = vars
            .get(key)
            .unwrap_or_else(|| panic!("{tag}: {key} is not in English"));
        let message = bundle
            .get_message(key)
            .unwrap_or_else(|| panic!("{tag}: {key} is not in the source"));
        let pattern = message.value().expect("a value");
        let has_number = numeric.iter().any(|n| *n);
        let samples: Vec<i64> = if has_number {
            SAMPLES.to_vec()
        } else {
            vec![0]
        };
        for sample in samples {
            let mut fluent = FluentArgs::new();
            let mut args = Vec::new();
            for (i, name) in names.iter().enumerate() {
                if numeric.get(i).copied().unwrap_or(false) {
                    let number = FluentNumber::new(sample as f64, Default::default());
                    fluent.set(name.as_str(), FluentValue::Number(number));
                    args.push(Arg::Num(sample.into()));
                } else {
                    fluent.set(name.as_str(), FluentValue::from("x"));
                    args.push(Arg::Str("x"));
                }
            }
            let mut errors = Vec::new();
            let want = bundle.format_pattern(pattern, Some(&fluent), &mut errors);
            assert!(errors.is_empty(), "{tag}: {key}: {errors:?}");
            let got = ours(key, &args).unwrap_or_else(|| panic!("{tag}: {key} has no body"));
            assert_eq!(got, want.as_ref(), "{tag}: {key} with {sample}");
            compared += 1;
        }
    }
    compared
}
