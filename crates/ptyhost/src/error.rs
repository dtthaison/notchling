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
            HostError::AlreadyRunning => write!(f, "Một hostd khác đang chạy trên socket này."),
            HostError::PathTooLong => write!(f, "Đường dẫn socket quá dài."),
            HostError::Socket(code) => write!(f, "Lỗi socket ({code})."),
            HostError::ConnectFailed => write!(f, "Không kết nối được tới hostd."),
            HostError::Timeout => write!(f, "hostd không trả lời."),
            HostError::Closed => write!(f, "hostd đã đóng kết nối."),
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
