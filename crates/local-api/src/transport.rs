use crate::peer::PeerInfo;
use std::io;
use std::path::Path;

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener as Listener, UnixStream as Stream};

#[cfg(unix)]
pub fn peer(stream: &Stream) -> io::Result<PeerInfo> {
    use std::os::fd::AsRawFd;
    crate::peer::peer_of_fd(stream.as_raw_fd())
}

#[cfg(unix)]
pub fn connect(path: &Path) -> io::Result<Stream> {
    Stream::connect(path)
}

#[cfg(unix)]
pub fn is_socket(path: &Path) -> io::Result<bool> {
    use std::os::unix::fs::FileTypeExt;
    Ok(std::fs::symlink_metadata(path)?.file_type().is_socket())
}

#[cfg(unix)]
pub fn bind(path: &Path) -> io::Result<Listener> {
    use std::os::unix::fs::PermissionsExt;
    let listener = Listener::bind(path)?;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    Ok(listener)
}

#[cfg(not(unix))]
mod pipe {
    use std::io;
    use std::time::Duration;

    #[derive(Debug)]
    pub enum Stream {}

    impl Stream {
        pub fn try_clone(&self) -> io::Result<Stream> {
            match *self {}
        }
        pub fn set_read_timeout(&self, _: Option<Duration>) -> io::Result<()> {
            match *self {}
        }
        pub fn set_write_timeout(&self, _: Option<Duration>) -> io::Result<()> {
            match *self {}
        }
        pub fn set_nonblocking(&self, _: bool) -> io::Result<()> {
            match *self {}
        }
        pub fn shutdown(&self, _: std::net::Shutdown) -> io::Result<()> {
            match *self {}
        }
    }

    impl io::Read for Stream {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            match *self {}
        }
    }

    impl io::Write for Stream {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            match *self {}
        }
        fn flush(&mut self) -> io::Result<()> {
            match *self {}
        }
    }

    impl io::Read for &Stream {
        fn read(&mut self, _: &mut [u8]) -> io::Result<usize> {
            match **self {}
        }
    }

    #[derive(Debug)]
    pub enum Listener {}

    impl Listener {
        pub fn accept(&self) -> io::Result<(Stream, ())> {
            match *self {}
        }
        pub fn set_nonblocking(&self, _: bool) -> io::Result<()> {
            match *self {}
        }
    }
}

#[cfg(not(unix))]
pub use pipe::{Listener, Stream};

fn unsupported() -> io::Error {
    io::Error::new(
        io::ErrorKind::Unsupported,
        "the local API's named pipe is not built yet",
    )
}

#[cfg(not(unix))]
pub fn peer(stream: &Stream) -> io::Result<PeerInfo> {
    match *stream {}
}

#[cfg(not(unix))]
pub fn connect(_path: &Path) -> io::Result<Stream> {
    Err(unsupported())
}

#[cfg(not(unix))]
pub fn is_socket(_path: &Path) -> io::Result<bool> {
    Err(unsupported())
}

#[cfg(not(unix))]
pub fn bind(_path: &Path) -> io::Result<Listener> {
    Err(unsupported())
}

pub fn unsupported_error() -> io::Error {
    unsupported()
}
