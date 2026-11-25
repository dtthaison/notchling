use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};

pub const MAX_LINE: usize = 1 << 20;
pub const OUTBOX: usize = 256;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TooLong;

pub struct LineReader {
    cap: usize,
    buf: Vec<u8>,
}

impl LineReader {
    pub fn new(cap: usize) -> LineReader {
        LineReader {
            cap,
            buf: Vec::new(),
        }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Result<Vec<Vec<u8>>, TooLong> {
        let mut out = Vec::new();
        for chunk in bytes.split_inclusive(|b| *b == b'\n') {
            let ends = chunk.last() == Some(&b'\n');
            let body = if ends {
                &chunk[..chunk.len() - 1]
            } else {
                chunk
            };
            if self.buf.len() + body.len() > self.cap {
                self.buf.clear();
                return Err(TooLong);
            }
            self.buf.extend_from_slice(body);
            if ends {
                let line = std::mem::take(&mut self.buf);
                if !line.is_empty() {
                    out.push(line);
                }
            }
        }
        Ok(out)
    }
}

pub struct Outbox {
    tx: SyncSender<Vec<u8>>,
}

impl Outbox {
    pub fn new(cap: usize) -> (Outbox, Receiver<Vec<u8>>) {
        let (tx, rx) = sync_channel(cap);
        (Outbox { tx }, rx)
    }

    pub fn push(&self, line: Vec<u8>) -> bool {
        match self.tx.try_send(line) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => false,
        }
    }
}
