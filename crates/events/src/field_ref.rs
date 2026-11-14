use crate::field::{hex_lower, sealed, FieldDesc, FieldValue, Hex, LogField};
use sha2::{Digest, Sha256};
use std::marker::PhantomData;

pub const REF_MAX: usize = 256;

pub trait RefKind {
    const NAME: &'static str;
    const ALWAYS_HASHED: bool = false;
}

macro_rules! ref_kinds {
    ($($ty:ident = $name:literal $(, $hashed:ident)?;)+) => {
        $(
            #[derive(Clone, Copy, Debug)]
            pub struct $ty;

            impl RefKind for $ty {
                const NAME: &'static str = $name;
                $(const $hashed: bool = true;)?
            }
        )+
    };
}

ref_kinds! {
    Session = "session";
    Folder = "folder";
    Account = "account";
    Device = "device";
    Secret = "secret";
    Lane = "lane";
    Item = "item", ALWAYS_HASHED;
    Page = "page";
    Model = "model";
    Machine = "machine";
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ref<K: RefKind> {
    id: Box<str>,
    kind: PhantomData<fn() -> K>,
}

impl<K: RefKind> Ref<K> {
    pub fn new(id: &str) -> Ref<K> {
        let clean: String = id
            .chars()
            .take(REF_MAX)
            .map(|c| if c.is_control() { '?' } else { c })
            .collect();
        Ref {
            id: clean.into(),
            kind: PhantomData,
        }
    }

    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl<K: RefKind> sealed::Sealed for Ref<K> {}

impl<K: RefKind> LogField for Ref<K> {
    const DESC: FieldDesc = FieldDesc {
        ty: "ref",
        choices: &[],
        optional: false,
        list: false,
        ref_kind: Some(K::NAME),
        always_hashed: K::ALWAYS_HASHED,
    };

    fn value(&self) -> FieldValue {
        FieldValue::Ref {
            kind: K::NAME,
            id: self.id.clone(),
            always_hashed: K::ALWAYS_HASHED,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Salt(pub [u8; 32]);

impl Salt {
    pub fn id(&self) -> Hex {
        let digest = Sha256::digest(self.0);
        Hex::from_bytes(&digest[..4])
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RefMode {
    Plain(Salt),
    Hashed(Salt),
}

impl RefMode {
    pub fn salt(&self) -> &Salt {
        match self {
            RefMode::Plain(salt) | RefMode::Hashed(salt) => salt,
        }
    }

    pub fn hashes(&self, always_hashed: bool) -> bool {
        matches!(self, RefMode::Hashed(_)) || always_hashed
    }
}

pub fn hash_ref(salt: &Salt, kind: &str, id: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.0);
    hasher.update(kind.as_bytes());
    hasher.update([0u8]);
    hasher.update(id.as_bytes());
    let digest = hasher.finalize();
    format!("h:{}", hex_lower(&digest[..4]))
}
