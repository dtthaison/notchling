//! Systems without a pty port yet (Windows, the BSDs): every call is [`PtyError::Unsupported`].

use super::{PtyCommand, PtyError, ReadOutcome, Signal};
use std::convert::Infallible;
use std::time::Duration;

/// Never made: [`Pty::spawn`] always fails here.
#[derive(Debug)]
pub struct Pty {
    never: Infallible,
}

impl Pty {
    pub fn spawn(_command: &PtyCommand) -> Result<Pty, PtyError> {
        Err(PtyError::Unsupported)
    }
    pub fn pid(&self) -> i32 {
        match self.never {}
    }
    pub fn read(&self, _buf: &mut [u8]) -> Result<ReadOutcome, PtyError> {
        match self.never {}
    }
    pub fn read_timeout(
        &self,
        _buf: &mut [u8],
        _timeout: Duration,
    ) -> Result<ReadOutcome, PtyError> {
        match self.never {}
    }
    pub fn write_all(&self, _bytes: &[u8]) -> Result<(), PtyError> {
        match self.never {}
    }
    pub fn write_all_timeout(&self, _bytes: &[u8], _timeout: Duration) -> Result<(), PtyError> {
        match self.never {}
    }
    pub fn resize(&self, _rows: u16, _cols: u16) -> Result<(), PtyError> {
        match self.never {}
    }
    pub fn try_wait(&self) -> Result<Option<i32>, PtyError> {
        match self.never {}
    }
    pub fn wait(&self) -> Result<i32, PtyError> {
        match self.never {}
    }
    pub fn signal(&self, _signal: Signal) -> Result<(), PtyError> {
        match self.never {}
    }
    pub fn terminate(&self, _grace: Duration) -> Option<i32> {
        match self.never {}
    }
}
