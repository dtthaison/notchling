use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostError {
    AlreadyRunning,
    PathTooLong,
    Socket(i32),
    ConnectFailed,
    Timeout,
    Closed,
    Refused(String),
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::AlreadyRunning => {
                write!(f, "Another hostd is already running on this socket.")
            }
            HostError::PathTooLong => write!(f, "The socket path is too long."),
            HostError::Socket(code) => write!(f, "Socket error ({code})."),
            HostError::ConnectFailed => write!(f, "Can't connect to hostd."),
            HostError::Timeout => write!(f, "hostd did not answer."),
            HostError::Closed => write!(f, "hostd closed the connection."),
            HostError::Refused(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for HostError {}

impl HostError {
    pub fn socket(error: &std::io::Error) -> HostError {
        HostError::Socket(error.raw_os_error().unwrap_or(-1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_host_error_is_english() {
        let errors = [
            HostError::AlreadyRunning,
            HostError::PathTooLong,
            HostError::Socket(2),
            HostError::ConnectFailed,
            HostError::Timeout,
            HostError::Closed,
        ];
        for error in errors {
            let text = error.to_string();
            assert!(!text.is_empty());
            assert!(text.is_ascii(), "{text}");
        }
    }
}
