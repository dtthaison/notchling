use sha2::{Digest as _, Sha256};

pub const CHUNK: usize = 256 * 1024;
pub const MAX_BLOB: u64 = 256 * 1024 * 1024;
const MAX_NAME: usize = 64;

pub fn safe_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_NAME
        && !text.starts_with('-')
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

pub fn chunks(len: u64) -> Vec<(u64, usize)> {
    let step = CHUNK as u64;
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < len {
        let size = (len - offset).min(step);
        out.push((offset, size as usize));
        offset += size;
    }
    out
}

pub fn hex_of(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn sha256_of(data: &[u8]) -> String {
    hex_of(&Sha256::digest(data))
}

#[derive(Default)]
pub struct Digest(Sha256);

impl Digest {
    pub fn update(&mut self, data: &[u8]) {
        self.0.update(data);
    }

    pub fn finish(self) -> String {
        hex_of(&self.0.finalize())
    }
}

pub struct Incoming {
    next: u64,
    cap: u64,
    hasher: Sha256,
}

impl Default for Incoming {
    fn default() -> Incoming {
        Incoming::with_cap(MAX_BLOB)
    }
}

impl Incoming {
    pub fn with_cap(cap: u64) -> Incoming {
        Incoming {
            next: 0,
            cap,
            hasher: Sha256::new(),
        }
    }

    pub fn next(&self) -> u64 {
        self.next
    }

    pub fn put(&mut self, offset: u64, data: &[u8]) -> Result<(), String> {
        if offset != self.next {
            return Err(format!(
                "a chunk came out of order: expected offset {}, got {offset}",
                self.next
            ));
        }
        if data.len() > CHUNK {
            return Err("a chunk is over 256 KB".to_string());
        }
        let end = self.next.saturating_add(data.len() as u64);
        if end > self.cap {
            return Err(format!("the file is over {} MB", self.cap / (1024 * 1024)));
        }
        self.hasher.update(data);
        self.next = end;
        Ok(())
    }

    pub fn finish(self, sha256: &str) -> Result<(), String> {
        let got = hex_of(&self.hasher.finalize());
        if got == sha256 {
            Ok(())
        } else {
            Err("the file arrived changed: its hash doesn't match".to_string())
        }
    }
}
