use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use std::path::Path;

pub(crate) mod sealed {
    pub trait Sealed {}
}

pub const UNKNOWN: &str = "unknown";
pub const LIST_MAX: usize = 16;
pub const ERROR_CODES: [&str; 7] = [
    "bad_request",
    "unknown_op",
    "forbidden",
    "needs_approval",
    "internal",
    "unimplemented",
    "failed",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FieldDesc {
    pub ty: &'static str,
    pub choices: &'static [&'static str],
    pub optional: bool,
    pub list: bool,
    pub ref_kind: Option<&'static str>,
    pub always_hashed: bool,
}

impl FieldDesc {
    pub const fn scalar(ty: &'static str) -> FieldDesc {
        FieldDesc {
            ty,
            choices: &[],
            optional: false,
            list: false,
            ref_kind: None,
            always_hashed: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldValue {
    Null,
    Bool(bool),
    Int(i64),
    UInt(u64),
    Name(&'static str),
    Token(Box<str>),
    Ref {
        kind: &'static str,
        id: Box<str>,
        always_hashed: bool,
    },
    List(Vec<FieldValue>),
}

pub trait LogField: sealed::Sealed {
    const DESC: FieldDesc;
    fn value(&self) -> FieldValue;
}

pub trait Choice: LogField + Copy {
    fn from_name(name: &str) -> Option<Self>;
    fn other() -> Self;
}

pub fn choice_or_other<E: Choice>(name: &str) -> E {
    E::from_name(name).unwrap_or_else(E::other)
}

macro_rules! scalar {
    ($($ty:ident($inner:ty) = $name:literal => $variant:ident;)+) => {
        $(
            #[derive(Clone, Copy, Debug, PartialEq, Eq)]
            pub struct $ty(pub $inner);

            impl sealed::Sealed for $ty {}

            impl LogField for $ty {
                const DESC: FieldDesc = FieldDesc::scalar($name);

                fn value(&self) -> FieldValue {
                    FieldValue::$variant(self.0)
                }
            }
        )+
    };
}

scalar! {
    Count(u64) = "count" => UInt;
    Bytes(u64) = "bytes" => UInt;
    Tokens(u64) = "tokens" => UInt;
    Millis(u64) = "millis" => UInt;
    Time(i64) = "time" => Int;
    Money(i64) = "money" => Int;
}

impl sealed::Sealed for bool {}

impl LogField for bool {
    const DESC: FieldDesc = FieldDesc::scalar("flag");

    fn value(&self) -> FieldValue {
        FieldValue::Bool(*self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percent(u64);

impl Percent {
    pub fn new(value: u64) -> Percent {
        Percent(value.min(100))
    }
}

impl sealed::Sealed for Percent {}

impl LogField for Percent {
    const DESC: FieldDesc = FieldDesc::scalar("percent");

    fn value(&self) -> FieldValue {
        FieldValue::UInt(self.0)
    }
}

fn token(text: &str, max: usize, allowed: impl Fn(char) -> bool) -> Box<str> {
    let ok = !text.is_empty() && text.len() <= max && text.chars().all(allowed);
    if ok {
        text.into()
    } else {
        UNKNOWN.into()
    }
}

macro_rules! token_type {
    ($($ty:ident = $name:literal;)+) => {
        $(
            #[derive(Clone, Debug, PartialEq, Eq)]
            pub struct $ty(Box<str>);

            impl $ty {
                pub fn as_str(&self) -> &str {
                    &self.0
                }
            }

            impl sealed::Sealed for $ty {}

            impl LogField for $ty {
                const DESC: FieldDesc = FieldDesc::scalar($name);

                fn value(&self) -> FieldValue {
                    FieldValue::Token(self.0.clone())
                }
            }
        )+
    };
}

token_type! {
    Version = "version";
    Exe = "exe";
    Tool = "tool";
    CodeLoc = "code_loc";
    Seal = "seal";
    Hex = "hex";
}

impl Version {
    pub fn new(text: &str) -> Version {
        Version(token(text, 32, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '+' | '-')
        }))
    }
}

impl Exe {
    pub fn named(name: &str) -> Exe {
        let dots_only = name.chars().all(|c| c == '.');
        if dots_only {
            return Exe(UNKNOWN.into());
        }
        Exe(token(name, 64, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-')
        }))
    }

    pub fn from_path(path: &Path) -> Exe {
        match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => Exe::named(name),
            None => Exe(UNKNOWN.into()),
        }
    }
}

impl Tool {
    pub fn new(name: &str) -> Tool {
        Tool(token(name, 64, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '.' | '-')
        }))
    }
}

impl CodeLoc {
    pub fn new(file: &'static str, line: u32) -> CodeLoc {
        let place = format!("{file}:{line}");
        CodeLoc(token(&place, 200, |c| {
            c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '/' | '-' | ':')
        }))
    }
}

impl Seal {
    pub fn from_bytes(bytes: [u8; 32]) -> Seal {
        Seal(STANDARD.encode(bytes).into())
    }

    pub fn parse(text: &str) -> Option<Seal> {
        if text.len() != 44 {
            return None;
        }
        match STANDARD.decode(text) {
            Ok(bytes) if bytes.len() == 32 => Some(Seal(text.into())),
            _ => None,
        }
    }
}

pub fn hex_lower(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(DIGITS[(b >> 4) as usize] as char);
        out.push(DIGITS[(b & 0x0f) as usize] as char);
    }
    out
}

impl Hex {
    pub fn new(text: &str) -> Option<Hex> {
        let ok = (8..=64).contains(&text.len())
            && text
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        ok.then(|| Hex(text.into()))
    }

    pub fn from_bytes(bytes: &[u8]) -> Hex {
        let mut text = hex_lower(bytes);
        text.truncate(64);
        while text.len() < 8 {
            text.push('0');
        }
        Hex(text.into())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Op(&'static str);

impl Op {
    pub fn named(name: &'static str) -> Op {
        Op(name)
    }

    pub fn parse(text: &str, known: &[&'static str]) -> Op {
        match known.iter().find(|k| **k == text) {
            Some(name) => Op(name),
            None => Op(UNKNOWN),
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl sealed::Sealed for Op {}

impl LogField for Op {
    const DESC: FieldDesc = FieldDesc::scalar("op");

    fn value(&self) -> FieldValue {
        FieldValue::Name(self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ErrorCode(&'static str);

impl ErrorCode {
    pub fn named(name: &'static str) -> ErrorCode {
        ErrorCode(name)
    }

    pub fn parse(text: &str) -> ErrorCode {
        match ERROR_CODES.iter().find(|k| **k == text) {
            Some(name) => ErrorCode(name),
            None => ErrorCode(UNKNOWN),
        }
    }

    pub fn as_str(&self) -> &'static str {
        self.0
    }
}

impl sealed::Sealed for ErrorCode {}

impl LogField for ErrorCode {
    const DESC: FieldDesc = FieldDesc {
        ty: "error_code",
        choices: &ERROR_CODES,
        optional: false,
        list: false,
        ref_kind: None,
        always_hashed: false,
    };

    fn value(&self) -> FieldValue {
        FieldValue::Name(self.0)
    }
}

impl<T: LogField> sealed::Sealed for Option<T> {}

impl<T: LogField> LogField for Option<T> {
    const DESC: FieldDesc = FieldDesc {
        optional: true,
        ..T::DESC
    };

    fn value(&self) -> FieldValue {
        match self {
            Some(inner) => inner.value(),
            None => FieldValue::Null,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct List<T>(Vec<T>);

impl<T> List<T> {
    pub fn new(mut items: Vec<T>) -> List<T> {
        items.truncate(LIST_MAX);
        List(items)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn items(&self) -> &[T] {
        &self.0
    }
}

impl<T: LogField> sealed::Sealed for List<T> {}

impl<T: LogField> LogField for List<T> {
    const DESC: FieldDesc = FieldDesc {
        list: true,
        ..T::DESC
    };

    fn value(&self) -> FieldValue {
        FieldValue::List(self.0.iter().map(LogField::value).collect())
    }
}
