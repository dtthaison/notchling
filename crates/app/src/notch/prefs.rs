use std::path::Path;

use notchling_engine::settings::Settings;
use notchling_locale as t;

use super::layout::NotchWidth;
use crate::acting::Control;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotchPrefs {
    pub shown: bool,
    pub width: NotchWidth,
    pub show_pet: bool,
    pub show_last_reply: bool,
    pub answer: bool,
}

impl Default for NotchPrefs {
    fn default() -> Self {
        NotchPrefs {
            shown: true,
            width: NotchWidth::Medium,
            show_pet: true,
            show_last_reply: true,
            answer: true,
        }
    }
}

impl NotchPrefs {
    pub fn from_settings(s: &Settings) -> NotchPrefs {
        let d = NotchPrefs::default();
        NotchPrefs {
            shown: !s.get_bool("hideNotch").unwrap_or(!d.shown),
            width: s
                .get_str("notchWidth")
                .and_then(|w| NotchWidth::from_key(&w))
                .unwrap_or(d.width),
            show_pet: s.get_bool("showPet").unwrap_or(d.show_pet),
            show_last_reply: s.get_bool("showLastReply").unwrap_or(d.show_last_reply),
            answer: s.get_bool("answerFromNotch").unwrap_or(d.answer),
        }
    }

    pub fn read(data_dir: &Path) -> Option<NotchPrefs> {
        Settings::open(&data_dir.join("settings.json"))
            .ok()
            .map(|s| NotchPrefs::from_settings(&s))
    }

    pub fn answer_gate(&self, acting: Control) -> Control {
        if self.answer {
            acting
        } else {
            Control {
                enabled: false,
                tooltip: Some(t::notch::answer_off()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    struct Dir(std::path::PathBuf);
    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    fn dir(tag: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("nl-notch-prefs-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    #[test]
    fn an_empty_store_reads_the_defaults() {
        let d = dir("empty");
        assert_eq!(NotchPrefs::read(&d.0), Some(NotchPrefs::default()));
    }

    #[test]
    fn the_settings_sheet_switches_reach_the_notch() {
        let d = dir("set");
        let mut s = Settings::open(&d.0.join("settings.json")).unwrap();
        for (key, value) in [
            ("hideNotch", json!(true)),
            ("notchWidth", json!("large")),
            ("showPet", json!(false)),
            ("showLastReply", json!(false)),
            ("answerFromNotch", json!(false)),
        ] {
            s.set(key, value).unwrap();
        }
        assert_eq!(
            NotchPrefs::read(&d.0),
            Some(NotchPrefs {
                shown: false,
                width: NotchWidth::Large,
                show_pet: false,
                show_last_reply: false,
                answer: false,
            })
        );
    }

    #[test]
    fn answering_off_disables_the_options_even_while_acting_is_on() {
        let on = Control {
            enabled: true,
            tooltip: None,
        };
        let off = NotchPrefs {
            answer: false,
            ..NotchPrefs::default()
        };
        assert_eq!(NotchPrefs::default().answer_gate(on), on);
        assert_eq!(
            off.answer_gate(on),
            Control {
                enabled: false,
                tooltip: Some(t::notch::answer_off())
            }
        );
    }

    #[test]
    fn a_later_change_is_read_on_the_next_read() {
        let d = dir("later");
        let mut s = Settings::open(&d.0.join("settings.json")).unwrap();
        s.set("showPet", json!(false)).unwrap();
        assert!(!NotchPrefs::read(&d.0).unwrap().show_pet);
        s.set("showPet", json!(true)).unwrap();
        assert!(NotchPrefs::read(&d.0).unwrap().show_pet);
    }
}
