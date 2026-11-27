#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum IndexRange {
    #[default]
    Days14,
    Days30,
    All,
}

impl IndexRange {
    pub fn from_setting(setting: &str) -> IndexRange {
        match setting {
            "14d" => IndexRange::Days14,
            "30d" => IndexRange::Days30,
            _ => IndexRange::All,
        }
    }

    pub fn window_secs(self) -> Option<f64> {
        match self {
            IndexRange::Days14 => Some(14.0 * 86_400.0),
            IndexRange::Days30 => Some(30.0 * 86_400.0),
            IndexRange::All => None,
        }
    }
}
