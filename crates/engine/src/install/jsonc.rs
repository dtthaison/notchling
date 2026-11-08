//! A JSON reader that keeps where every value sits in the text, so a file can be edited by
//! splicing a few bytes and everything else stays as the user wrote it.
//!
//! It reads what Claude Code's settings may hold: JSON, plus `//` and `/* */` comments, trailing
//! commas and a leading byte-order mark (all kept as they are when the file is edited). Anything
//! else is an error, and the error carries a place and a fixed phrase, never a piece of the
//! file: settings.json can hold tokens.

use serde_json::Value;

/// Deeper than this is refused (a hostile file must not overflow the stack).
const MAX_DEPTH: usize = 64;

/// A value and the bytes it covers: `text[start..end]`.
#[derive(Debug, Clone, PartialEq)]
pub struct Node {
    pub start: usize,
    pub end: usize,
    pub kind: Kind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    Object(Vec<Member>),
    Array(Vec<Node>),
    Str(String),
    /// A number, `true`, `false` or `null`; its text is `text[start..end]`.
    Scalar,
}

/// `"key": value`: it starts at the key's quote and ends where the value ends.
#[derive(Debug, Clone, PartialEq)]
pub struct Member {
    pub key: String,
    pub start: usize,
    pub value: Node,
}

/// Why a file could not be read: where, and in fixed words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub at: usize,
    pub what: &'static str,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} (at byte {})", self.what, self.at)
    }
}

impl Node {
    pub fn as_str(&self) -> Option<&str> {
        match &self.kind {
            Kind::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn members(&self) -> Option<&[Member]> {
        match &self.kind {
            Kind::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn items(&self) -> Option<&[Node]> {
        match &self.kind {
            Kind::Array(a) => Some(a),
            _ => None,
        }
    }

    /// The first member with this key.
    pub fn get(&self, key: &str) -> Option<&Node> {
        self.members()?
            .iter()
            .find(|m| m.key == key)
            .map(|m| &m.value)
    }

    /// How many members have this key (a duplicate makes an edit ambiguous).
    pub fn count_key(&self, key: &str) -> usize {
        self.members()
            .map_or(0, |m| m.iter().filter(|m| m.key == key).count())
    }
}

impl Node {
    /// True when this value, or any object inside it, has a key twice. Claude Code takes the
    /// last of two, [`Node::get`] the first, so an edit planned from one would miss the other.
    pub fn has_repeated_key(&self) -> bool {
        match &self.kind {
            Kind::Object(members) => {
                let mut seen = std::collections::HashSet::new();
                members
                    .iter()
                    .any(|m| !seen.insert(m.key.as_str()) || m.value.has_repeated_key())
            }
            Kind::Array(items) => items.iter().any(Node::has_repeated_key),
            Kind::Str(_) | Kind::Scalar => false,
        }
    }
}

/// Where the value starts: after a byte-order mark, if there is one.
pub fn bom_len(text: &str) -> usize {
    if text.starts_with('\u{feff}') {
        3
    } else {
        0
    }
}

/// True when the text is empty or only space (a byte-order mark counts as nothing).
pub fn is_blank(text: &str) -> bool {
    text[bom_len(text)..].trim().is_empty()
}

/// Reads one value with nothing but space and comments around it.
pub fn parse(text: &str) -> Result<Node, ParseError> {
    let mut p = Parser {
        text,
        bytes: text.as_bytes(),
        pos: bom_len(text),
    };
    p.trivia()?;
    let node = p.value(0)?;
    p.trivia()?;
    if p.pos != p.bytes.len() {
        return Err(p.err("more text after the value"));
    }
    Ok(node)
}

/// The next byte that is not space or a comment, or the end of the text.
pub fn next_significant(text: &str, from: usize) -> usize {
    let mut p = Parser {
        text,
        bytes: text.as_bytes(),
        pos: from,
    };
    let _ = p.trivia();
    p.pos
}

/// The value as `serde_json` sees it, for comparing what an edit did with what was meant.
pub fn to_value(node: &Node, text: &str) -> Value {
    match &node.kind {
        Kind::Object(members) => Value::Object(
            members
                .iter()
                .map(|m| (m.key.clone(), to_value(&m.value, text)))
                .collect(),
        ),
        Kind::Array(items) => Value::Array(items.iter().map(|n| to_value(n, text)).collect()),
        Kind::Str(s) => Value::String(s.clone()),
        Kind::Scalar => serde_json::from_str(&text[node.start..node.end]).unwrap_or(Value::Null),
    }
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    pos: usize,
}

impl Parser<'_> {
    fn err(&self, what: &'static str) -> ParseError {
        ParseError { at: self.pos, what }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    /// Space and comments.
    fn trivia(&mut self) -> Result<(), ParseError> {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\n' | b'\r') => self.pos += 1,
                Some(b'/') => match self.bytes.get(self.pos + 1) {
                    Some(b'/') => {
                        while !matches!(self.peek(), None | Some(b'\n')) {
                            self.pos += 1;
                        }
                    }
                    Some(b'*') => {
                        let from = self.pos + 2;
                        match self.text[from..].find("*/") {
                            Some(i) => self.pos = from + i + 2,
                            None => return Err(self.err("a comment that never ends")),
                        }
                    }
                    _ => return Err(self.err("a stray slash")),
                },
                _ => return Ok(()),
            }
        }
    }

    fn value(&mut self, depth: usize) -> Result<Node, ParseError> {
        if depth > MAX_DEPTH {
            return Err(self.err("nested too deeply"));
        }
        let start = self.pos;
        let kind = match self.peek() {
            Some(b'{') => self.object(depth)?,
            Some(b'[') => self.array(depth)?,
            Some(b'"') => Kind::Str(self.string()?),
            Some(b'-' | b'0'..=b'9' | b't' | b'f' | b'n') => {
                self.scalar()?;
                Kind::Scalar
            }
            Some(_) => return Err(self.err("not a value")),
            None => return Err(self.err("the text ends where a value should be")),
        };
        Ok(Node {
            start,
            end: self.pos,
            kind,
        })
    }

    fn object(&mut self, depth: usize) -> Result<Kind, ParseError> {
        self.pos += 1;
        let mut members = Vec::new();
        loop {
            self.trivia()?;
            match self.peek() {
                Some(b'}') => {
                    self.pos += 1;
                    return Ok(Kind::Object(members));
                }
                Some(b'"') => {}
                Some(_) => return Err(self.err("expected a key or }")),
                None => return Err(self.err("the object is not closed")),
            }
            let start = self.pos;
            let key = self.string()?;
            self.trivia()?;
            if self.peek() != Some(b':') {
                return Err(self.err("expected a colon"));
            }
            self.pos += 1;
            self.trivia()?;
            let value = self.value(depth + 1)?;
            members.push(Member { key, start, value });
            self.trivia()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {}
                Some(_) => return Err(self.err("expected a comma or }")),
                None => return Err(self.err("the object is not closed")),
            }
        }
    }

    fn array(&mut self, depth: usize) -> Result<Kind, ParseError> {
        self.pos += 1;
        let mut items = Vec::new();
        loop {
            self.trivia()?;
            match self.peek() {
                Some(b']') => {
                    self.pos += 1;
                    return Ok(Kind::Array(items));
                }
                Some(_) => {}
                None => return Err(self.err("the array is not closed")),
            }
            items.push(self.value(depth + 1)?);
            self.trivia()?;
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {}
                Some(_) => return Err(self.err("expected a comma or ]")),
                None => return Err(self.err("the array is not closed")),
            }
        }
    }

    /// A string literal, unescaped by `serde_json` (which also refuses a raw control character).
    fn string(&mut self) -> Result<String, ParseError> {
        let start = self.pos;
        self.pos += 1;
        loop {
            match self.peek() {
                Some(b'\\') => self.pos += 2,
                Some(b'"') => {
                    self.pos += 1;
                    break;
                }
                Some(b) if b < 0x20 => return Err(self.err("a control character in a string")),
                Some(_) => self.pos += 1,
                None => return Err(self.err("a string that never ends")),
            }
            if self.pos > self.bytes.len() {
                return Err(self.err("a string that never ends"));
            }
        }
        serde_json::from_str(&self.text[start..self.pos]).map_err(|_| ParseError {
            at: start,
            what: "a string with a bad escape",
        })
    }

    fn scalar(&mut self) -> Result<(), ParseError> {
        let start = self.pos;
        while matches!(self.peek(), Some(b) if b.is_ascii_alphanumeric() || matches!(b, b'+' | b'-' | b'.'))
        {
            self.pos += 1;
        }
        match serde_json::from_str::<Value>(&self.text[start..self.pos]) {
            Ok(Value::Null | Value::Bool(_) | Value::Number(_)) => Ok(()),
            _ => Err(ParseError {
                at: start,
                what: "not a number, true, false or null",
            }),
        }
    }
}
