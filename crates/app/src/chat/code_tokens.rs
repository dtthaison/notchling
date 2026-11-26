use std::ops::Range;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Rust,
    Ruby,
    Python,
    Script,
    Go,
    Swift,
    Shell,
    Yaml,
    Toml,
    Json,
    CLike,
    Css,
    Sql,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Class {
    Keyword,
    Str,
    Comment,
    Number,
    Key,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub class: Class,
}

struct Syntax {
    keywords: &'static [&'static str],
    line: &'static [&'static str],
    block: Option<(&'static str, &'static str)>,
    quotes: &'static [u8],
    triple: bool,
    multiline: &'static [u8],
    keys: Keys,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Keys {
    None,
    Json,
    Yaml,
    Toml,
}

const RUST: &[&str] = &[
    "as", "async", "await", "break", "const", "continue", "crate", "dyn", "else", "enum", "extern",
    "false", "fn", "for", "if", "impl", "in", "let", "loop", "match", "mod", "move", "mut", "pub",
    "ref", "return", "self", "Self", "static", "struct", "super", "trait", "true", "type",
    "unsafe", "use", "where", "while",
];
const RUBY: &[&str] = &[
    "alias",
    "and",
    "begin",
    "break",
    "case",
    "class",
    "def",
    "do",
    "else",
    "elsif",
    "end",
    "ensure",
    "false",
    "for",
    "if",
    "in",
    "module",
    "next",
    "nil",
    "not",
    "or",
    "redo",
    "rescue",
    "retry",
    "return",
    "self",
    "super",
    "then",
    "true",
    "undef",
    "unless",
    "until",
    "when",
    "while",
    "yield",
    "require",
    "attr_accessor",
    "attr_reader",
    "attr_writer",
];
const PYTHON: &[&str] = &[
    "and", "as", "assert", "async", "await", "break", "class", "continue", "def", "del", "elif",
    "else", "except", "False", "finally", "for", "from", "global", "if", "import", "in", "is",
    "lambda", "None", "nonlocal", "not", "or", "pass", "raise", "return", "True", "try", "while",
    "with", "yield",
];
const SCRIPT: &[&str] = &[
    "abstract",
    "as",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "const",
    "continue",
    "default",
    "delete",
    "do",
    "else",
    "enum",
    "export",
    "extends",
    "false",
    "finally",
    "for",
    "from",
    "function",
    "if",
    "implements",
    "import",
    "in",
    "instanceof",
    "interface",
    "let",
    "new",
    "null",
    "of",
    "private",
    "protected",
    "public",
    "readonly",
    "return",
    "static",
    "super",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "type",
    "typeof",
    "undefined",
    "var",
    "void",
    "while",
    "yield",
];
const GO: &[&str] = &[
    "break",
    "case",
    "chan",
    "const",
    "continue",
    "default",
    "defer",
    "else",
    "fallthrough",
    "false",
    "for",
    "func",
    "go",
    "goto",
    "if",
    "import",
    "interface",
    "map",
    "nil",
    "package",
    "range",
    "return",
    "select",
    "struct",
    "switch",
    "true",
    "type",
    "var",
];
const SWIFT: &[&str] = &[
    "actor",
    "as",
    "associatedtype",
    "async",
    "await",
    "break",
    "case",
    "catch",
    "class",
    "continue",
    "default",
    "defer",
    "do",
    "else",
    "enum",
    "extension",
    "false",
    "fileprivate",
    "func",
    "guard",
    "if",
    "import",
    "in",
    "init",
    "inout",
    "internal",
    "is",
    "let",
    "nil",
    "open",
    "private",
    "protocol",
    "public",
    "return",
    "self",
    "Self",
    "static",
    "struct",
    "super",
    "switch",
    "throw",
    "throws",
    "true",
    "try",
    "typealias",
    "var",
    "where",
    "while",
];
const SHELL: &[&str] = &[
    "case", "do", "done", "elif", "else", "esac", "export", "fi", "for", "function", "if", "in",
    "local", "return", "set", "then", "unset", "until", "while",
];
const YAML: &[&str] = &["true", "false", "null", "yes", "no"];
const TOML: &[&str] = &["true", "false"];
const JSON: &[&str] = &["true", "false", "null"];
const CLIKE: &[&str] = &[
    "abstract",
    "break",
    "case",
    "catch",
    "char",
    "class",
    "const",
    "continue",
    "default",
    "do",
    "double",
    "else",
    "enum",
    "extends",
    "false",
    "final",
    "finally",
    "float",
    "for",
    "fun",
    "if",
    "implements",
    "import",
    "int",
    "interface",
    "long",
    "namespace",
    "new",
    "null",
    "override",
    "package",
    "private",
    "protected",
    "public",
    "return",
    "static",
    "struct",
    "switch",
    "this",
    "throw",
    "true",
    "try",
    "typedef",
    "using",
    "val",
    "var",
    "virtual",
    "void",
    "while",
];
const CSS: &[&str] = &["important"];
const SQL: &[&str] = &[
    "select", "from", "where", "insert", "into", "values", "update", "set", "delete", "create",
    "table", "drop", "alter", "join", "left", "right", "inner", "outer", "on", "group", "by",
    "order", "having", "limit", "and", "or", "not", "null", "as", "distinct", "union", "index",
    "primary", "key", "SELECT", "FROM", "WHERE", "INSERT", "INTO", "VALUES", "UPDATE", "SET",
    "DELETE", "CREATE", "TABLE", "DROP", "ALTER", "JOIN", "LEFT", "RIGHT", "INNER", "OUTER", "ON",
    "GROUP", "BY", "ORDER", "HAVING", "LIMIT", "AND", "OR", "NOT", "NULL", "AS", "DISTINCT",
    "UNION", "INDEX", "PRIMARY", "KEY",
];

fn syntax(lang: Lang) -> Syntax {
    let base = Syntax {
        keywords: &[],
        line: &[],
        block: None,
        quotes: b"\"'",
        triple: false,
        multiline: b"",
        keys: Keys::None,
    };
    match lang {
        Lang::Rust => Syntax {
            keywords: RUST,
            line: &["//"],
            block: Some(("/*", "*/")),
            quotes: b"\"",
            ..base
        },
        Lang::Ruby => Syntax {
            keywords: RUBY,
            line: &["#"],
            ..base
        },
        Lang::Python => Syntax {
            keywords: PYTHON,
            line: &["#"],
            triple: true,
            ..base
        },
        Lang::Script => Syntax {
            keywords: SCRIPT,
            line: &["//"],
            block: Some(("/*", "*/")),
            quotes: b"\"'`",
            multiline: b"`",
            ..base
        },
        Lang::Go => Syntax {
            keywords: GO,
            line: &["//"],
            block: Some(("/*", "*/")),
            quotes: b"\"'`",
            multiline: b"`",
            ..base
        },
        Lang::Swift => Syntax {
            keywords: SWIFT,
            line: &["//"],
            block: Some(("/*", "*/")),
            quotes: b"\"",
            triple: true,
            ..base
        },
        Lang::Shell => Syntax {
            keywords: SHELL,
            line: &["#"],
            ..base
        },
        Lang::Yaml => Syntax {
            keywords: YAML,
            line: &["#"],
            keys: Keys::Yaml,
            ..base
        },
        Lang::Toml => Syntax {
            keywords: TOML,
            line: &["#"],
            triple: true,
            keys: Keys::Toml,
            ..base
        },
        Lang::Json => Syntax {
            keywords: JSON,
            quotes: b"\"",
            keys: Keys::Json,
            ..base
        },
        Lang::CLike => Syntax {
            keywords: CLIKE,
            line: &["//"],
            block: Some(("/*", "*/")),
            ..base
        },
        Lang::Css => Syntax {
            keywords: CSS,
            block: Some(("/*", "*/")),
            ..base
        },
        Lang::Sql => Syntax {
            keywords: SQL,
            line: &["--"],
            block: Some(("/*", "*/")),
            quotes: b"'\"",
            ..base
        },
    }
}

pub fn lang_of(path: &Path) -> Option<Lang> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    match name.as_str() {
        "gemfile" | "rakefile" | "podfile" | "fastfile" | "guardfile" => return Some(Lang::Ruby),
        "makefile" | "dockerfile" | ".bashrc" | ".zshrc" | ".profile" | ".envrc" => {
            return Some(Lang::Shell)
        }
        _ => {}
    }
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    Some(match ext.as_str() {
        "rs" => Lang::Rust,
        "rb" | "rake" | "gemspec" | "ru" | "erb" => Lang::Ruby,
        "py" | "pyi" => Lang::Python,
        "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" | "mts" | "cts" => Lang::Script,
        "go" => Lang::Go,
        "swift" => Lang::Swift,
        "sh" | "bash" | "zsh" | "fish" | "command" => Lang::Shell,
        "yml" | "yaml" => Lang::Yaml,
        "toml" => Lang::Toml,
        "json" | "jsonc" | "json5" => Lang::Json,
        "c" | "h" | "cc" | "cpp" | "cxx" | "hpp" | "hh" | "m" | "mm" | "java" | "kt" | "kts"
        | "cs" | "dart" | "scala" | "php" => Lang::CLike,
        "css" | "scss" | "less" => Lang::Css,
        "sql" => Lang::Sql,
        _ => return None,
    })
}

fn is_word(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b >= 0x80
}

fn char_len(b: u8) -> usize {
    match b {
        0..=0x7f => 1,
        0xc0..=0xdf => 2,
        0xe0..=0xef => 3,
        _ => 4,
    }
}

fn string_end(s: &[u8], from: usize, quote: u8, multiline: bool) -> usize {
    let mut i = from;
    while i < s.len() {
        match s[i] {
            b'\\' => i += 2,
            b'\n' if !multiline => return i,
            b if b == quote => return i + 1,
            _ => i += 1,
        }
    }
    s.len()
}

fn key_end(syntax: &Syntax, s: &[u8], at: usize) -> Option<usize> {
    let mut i = at;
    match syntax.keys {
        Keys::Yaml => {
            while s.get(i) == Some(&b'-') && s.get(i + 1) == Some(&b' ') {
                i += 2;
                while s.get(i) == Some(&b' ') {
                    i += 1;
                }
            }
            let start = i;
            while i < s.len() && (is_word(s[i]) || matches!(s[i], b'.' | b'-' | b'/')) {
                i += 1;
            }
            let end = i;
            (end > start
                && s.get(end) == Some(&b':')
                && matches!(s.get(end + 1), None | Some(b' ' | b'\n' | b'\r')))
            .then_some(end)
        }
        Keys::Toml => {
            if s.get(i) == Some(&b'[') {
                let mut j = i;
                while j < s.len() && s[j] != b'\n' && s[j] != b']' {
                    j += 1;
                }
                if s.get(j) == Some(&b']') {
                    if s.get(j + 1) == Some(&b']') {
                        j += 1;
                    }
                    return Some(j + 1);
                }
                return None;
            }
            let start = i;
            while i < s.len() && (is_word(s[i]) || matches!(s[i], b'.' | b'-')) {
                i += 1;
            }
            let end = i;
            while s.get(i) == Some(&b' ') {
                i += 1;
            }
            (end > start && s.get(i) == Some(&b'=')).then_some(end)
        }
        _ => None,
    }
}

pub fn highlight(lang: Lang, text: &str) -> Vec<Span> {
    let syntax = syntax(lang);
    let s = text.as_bytes();
    let mut out: Vec<Span> = Vec::new();
    let mut push = |start: usize, end: usize, class: Class| {
        if end > start {
            out.push(Span { start, end, class });
        }
    };
    let mut i = 0;
    let mut line_start = true;
    while i < s.len() {
        let b = s[i];
        if b == b'\n' {
            line_start = true;
            i += 1;
            continue;
        }
        if line_start && (b == b' ' || b == b'\t') {
            i += 1;
            continue;
        }
        let at_start = line_start;
        line_start = false;
        if at_start && syntax.keys != Keys::None && syntax.keys != Keys::Json {
            if let Some(end) = key_end(&syntax, s, i) {
                let lead = if syntax.keys == Keys::Yaml {
                    let mut j = i;
                    while s.get(j) == Some(&b'-') && s.get(j + 1) == Some(&b' ') {
                        j += 2;
                        while s.get(j) == Some(&b' ') {
                            j += 1;
                        }
                    }
                    j
                } else {
                    i
                };
                push(lead, end, Class::Key);
                i = end;
                continue;
            }
        }
        if let Some((open, close)) = syntax.block {
            if text[i..].starts_with(open) {
                let end = text[i + open.len()..]
                    .find(close)
                    .map_or(s.len(), |k| i + open.len() + k + close.len());
                push(i, end, Class::Comment);
                i = end;
                continue;
            }
        }
        if let Some(open) = syntax.line.iter().find(|m| text[i..].starts_with(**m)) {
            let hash = *open == "#";
            let before = if i == 0 { b' ' } else { s[i - 1] };
            if !hash || before.is_ascii_whitespace() {
                let end = text[i..].find('\n').map_or(s.len(), |k| i + k);
                push(i, end, Class::Comment);
                i = end;
                continue;
            }
        }
        if syntax.quotes.contains(&b) {
            let triple = [b; 3];
            let end = if syntax.triple && s[i..].starts_with(&triple) {
                text[i + 3..]
                    .find(std::str::from_utf8(&triple).unwrap_or("\"\"\""))
                    .map_or(s.len(), |k| i + 3 + k + 3)
            } else {
                string_end(s, i + 1, b, syntax.multiline.contains(&b))
            }
            .min(s.len());
            let mut class = Class::Str;
            if syntax.keys == Keys::Json {
                let mut j = end;
                while s.get(j).is_some_and(|c| *c == b' ' || *c == b'\t') {
                    j += 1;
                }
                if s.get(j) == Some(&b':') {
                    class = Class::Key;
                }
            }
            push(i, end, class);
            i = end;
            continue;
        }
        if b.is_ascii_digit() && (i == 0 || !is_word(s[i - 1])) {
            let mut j = i + 1;
            while j < s.len()
                && (s[j].is_ascii_alphanumeric()
                    || s[j] == b'_'
                    || (s[j] == b'.' && s.get(j + 1).is_some_and(u8::is_ascii_digit))
                    || (matches!(s[j], b'+' | b'-') && matches!(s[j - 1], b'e' | b'E')))
            {
                j += 1;
            }
            push(i, j, Class::Number);
            i = j;
            continue;
        }
        if b.is_ascii_alphabetic() || b == b'_' {
            let mut j = i + 1;
            while j < s.len() && is_word(s[j]) {
                j += 1;
            }
            let prev_dot = i > 0 && s[i - 1] == b'.' && lang != Lang::Sql;
            if !prev_dot && syntax.keywords.contains(&&text[i..j]) {
                push(i, j, Class::Keyword);
            }
            i = j;
            continue;
        }
        i += char_len(b);
    }
    out
}

pub fn segments(spans: &[Span], range: Range<usize>) -> Vec<(usize, Option<Class>)> {
    let mut out = Vec::new();
    let mut at = range.start;
    let first = spans.partition_point(|s| s.end <= range.start);
    for span in &spans[first..] {
        if span.start >= range.end {
            break;
        }
        let start = span.start.max(at);
        let end = span.end.min(range.end);
        if start > at {
            out.push((start - at, None));
        }
        if end > start {
            out.push((end - start, Some(span.class)));
        }
        at = at.max(end);
    }
    if at < range.end {
        out.push((range.end - at, None));
    }
    out
}
