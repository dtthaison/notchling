use notchling_kit::{Icon, Ink};

pub const NOTICE_MS: i64 = 12_000;
pub const LIMIT_SWITCH_MS: i64 = 3_600_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    LimitSwitch,
    Error,
    Notice,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Banner {
    pub kind: Kind,
    pub title: Option<String>,
    pub text: String,
    pub at_ms: i64,
    pub lane: Option<String>,
}

impl Banner {
    pub fn switched(from: &str, to: &str, used_percent: f64, at_ms: i64) -> Banner {
        Banner {
            kind: Kind::LimitSwitch,
            title: None,
            text: format!(
                "Switched from {from} to {to} ({}% used) because of the usage limit.",
                used_percent.round() as i64
            ),
            at_ms,
            lane: Some(String::new()),
        }
    }

    pub fn lane_switched(lane: &str, from: &str, to: &str, at_ms: i64) -> Banner {
        Banner {
            kind: Kind::LimitSwitch,
            title: None,
            text: format!(
                "The {lane} lane switched from {from} to {to} because of the usage limit. Its \
                 sessions carry on."
            ),
            at_ms,
            lane: Some(lane.to_owned()),
        }
    }

    pub fn error(text: impl Into<String>, at_ms: i64) -> Banner {
        Banner {
            kind: Kind::Error,
            title: None,
            text: text.into(),
            at_ms,
            lane: None,
        }
    }

    pub fn notice(title: impl Into<String>, text: impl Into<String>, at_ms: i64) -> Banner {
        Banner {
            kind: Kind::Notice,
            title: Some(title.into()),
            text: text.into(),
            at_ms,
            lane: None,
        }
    }

    pub fn from_announcement(text: &str, at_ms: i64) -> Option<Banner> {
        let lane = if let Some(rest) = text.strip_prefix("The ") {
            rest.split_once(" lane switched from ")?.0.to_owned()
        } else if text.starts_with("Switched from ") {
            String::new()
        } else {
            return None;
        };
        Some(Banner {
            kind: Kind::LimitSwitch,
            title: None,
            text: text.to_owned(),
            at_ms,
            lane: Some(lane),
        })
    }

    pub fn icon(&self) -> Icon {
        match self.kind {
            Kind::LimitSwitch => Icon::Resume,
            Kind::Error => Icon::Needs,
            Kind::Notice => Icon::Info,
        }
    }

    pub fn ink(&self) -> Ink {
        match self.kind {
            Kind::LimitSwitch => Ink::AccentText,
            Kind::Error => Ink::Needs,
            Kind::Notice => Ink::Text,
        }
    }

    pub fn expired(&self, now_ms: i64) -> bool {
        let life = match self.kind {
            Kind::Notice => NOTICE_MS,
            Kind::LimitSwitch => LIMIT_SWITCH_MS,
            Kind::Error => return false,
        };
        now_ms - self.at_ms >= life
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Banners {
    list: Vec<Banner>,
}

impl Banners {
    pub fn push(&mut self, banner: Banner) {
        self.list
            .retain(|b| b.text != banner.text && !(banner.lane.is_some() && b.lane == banner.lane));
        self.list.insert(0, banner);
    }

    pub fn dismiss(&mut self, index: usize) {
        if index < self.list.len() {
            self.list.remove(index);
        }
    }

    pub fn prune(&mut self, now_ms: i64) -> bool {
        let before = self.list.len();
        self.list.retain(|b| !b.expired(now_ms));
        before != self.list.len()
    }

    pub fn for_lane(&self, lane: Option<&str>) -> Vec<&Banner> {
        let lane = lane.unwrap_or("");
        self.list
            .iter()
            .filter(|b| b.kind != Kind::LimitSwitch || b.lane.as_deref() == Some(lane))
            .collect()
    }

    pub fn all(&self) -> &[Banner] {
        &self.list
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn switches_read_back_from_their_announcements() {
        let main = Banner::switched("a@x.com", "b@x.com", 41.6, 5);
        assert_eq!(
            main.text,
            "Switched from a@x.com to b@x.com (42% used) because of the usage limit."
        );
        assert_eq!(Banner::from_announcement(&main.text, 5), Some(main));
        let lane = Banner::lane_switched("night", "a@x.com", "c@x.com", 7);
        assert_eq!(Banner::from_announcement(&lane.text, 7), Some(lane.clone()));
        assert_eq!(lane.lane.as_deref(), Some("night"));
        assert_eq!(Banner::from_announcement("Keep Mac Awake is on", 1), None);
        assert_eq!(Banner::from_announcement("The end", 1), None);
    }

    #[test]
    fn notices_go_errors_stay_and_switches_last_an_hour() {
        let notice = Banner::notice("Notchling", "Notifications are off", 0);
        assert!(!notice.expired(NOTICE_MS - 1) && notice.expired(NOTICE_MS));
        assert!(!Banner::error("Couldn't stop it", 0).expired(i64::MAX / 2));
        let switch = Banner::switched("a", "b", 10.0, 0);
        assert!(!switch.expired(LIMIT_SWITCH_MS - 1) && switch.expired(LIMIT_SWITCH_MS));
        assert_eq!(switch.icon(), Icon::Resume);
        assert_eq!(Banner::error("x", 0).ink(), Ink::Needs);
    }

    #[test]
    fn the_list_dedupes_replaces_per_lane_and_prunes() {
        let mut b = Banners::default();
        b.push(Banner::error("Couldn't stop it", 0));
        b.push(Banner::switched("a", "b", 10.0, 0));
        b.push(Banner::lane_switched("night", "c", "d", 0));
        b.push(Banner::error("Couldn't stop it", 1));
        assert_eq!(b.all().len(), 3);
        assert_eq!(b.all()[0].at_ms, 1, "the same text moves to the top");
        b.push(Banner::switched("b", "e", 20.0, 2));
        assert_eq!(b.all().len(), 3, "a newer switch replaces the main login's");
        let night: Vec<&str> = b
            .for_lane(Some("night"))
            .iter()
            .map(|x| x.text.as_str())
            .collect();
        assert_eq!(night.len(), 2);
        assert!(night.iter().any(|t| t.contains("night lane")));
        assert_eq!(b.for_lane(None).len(), 2);
        b.push(Banner::notice("Notchling", "hi", 0));
        assert!(b.prune(NOTICE_MS));
        assert!(!b.prune(NOTICE_MS));
        b.dismiss(0);
        b.dismiss(99);
        assert_eq!(b.all().len(), 2);
    }
}
