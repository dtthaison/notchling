//! The Settings sheet's Notch switches as the notch reads them: whether it shows at all
//! (`hideNotch`), its width, the pet (`showPet`), the last reply in a peek (`showLastReply`) and
//! answering prompts from it (`answerFromNotch`). Read from the settings store on every poll, so a
//! change in Settings reaches the notch within one poll; a key never set reads as its default.

use std::path::Path;

use notchling_engine::settings::Settings;

use super::layout::NotchWidth;
use crate::acting::Control;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NotchPrefs {
    /// "Show agents in the notch": off orders the panel out.
    pub shown: bool,
    pub width: NotchWidth,
    /// "Animate pet": off hides the pet, as the Swift app did.
    pub show_pet: bool,
    pub show_last_reply: bool,
    /// "Answer prompts from the notch": off disables a card's options.
    pub answer: bool,
}

impl Default for NotchPrefs {
    /// The keys' defaults.
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
    /// From a store; each key falls back to its default.
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

    /// From the store in `data_dir`; `None` when it can't be read, so the notch keeps what it has.
    pub fn read(data_dir: &Path) -> Option<NotchPrefs> {
        Settings::open(&data_dir.join("settings.json"))
            .ok()
            .map(|s| NotchPrefs::from_settings(&s))
    }

    /// A card's options: off in Settings beats whatever acting says.
    pub fn answer_gate(&self, acting: Control) -> Control {
        if self.answer {
            acting
        } else {
            Control {
                enabled: false,
                tooltip: Some(ANSWER_OFF),
            }
        }
    }
}

/// Why a card's options are disabled while answering from the notch is off.
pub const ANSWER_OFF: &str = "Answering from the notch is off in Settings.";

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
                tooltip: Some(ANSWER_OFF)
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
