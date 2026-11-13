use crate::Ink;

pub const FLASH_MS: u64 = 300;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Region {
    Sidebar,
    Conversation,
    Details,
    Window,
}

impl Region {
    pub const ALL: [Region; 4] = [
        Region::Sidebar,
        Region::Conversation,
        Region::Details,
        Region::Window,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Region::Sidebar => "Sidebar",
            Region::Conversation => "Conversation",
            Region::Details => "Details Panel",
            Region::Window => "Window",
        }
    }

    pub fn word(self) -> &'static str {
        if self.is_part() {
            "draws alone"
        } else {
            "drawn every frame"
        }
    }

    pub fn ink(self) -> Ink {
        match self {
            Region::Sidebar => Ink::Done,
            Region::Conversation => Ink::Accent,
            Region::Details => Ink::Shell,
            Region::Window => Ink::Guide,
        }
    }

    pub fn on_ink(self) -> Ink {
        match self {
            Region::Sidebar | Region::Conversation => Ink::OnAccent,
            Region::Details => Ink::OnShell,
            Region::Window => Ink::OnGuide,
        }
    }

    pub fn is_part(self) -> bool {
        self != Region::Window
    }
}

pub fn label_inks(region: Option<Region>) -> (Ink, Ink) {
    match region {
        Some(r) => (r.ink(), r.on_ink()),
        None => (Region::Window.ink(), Region::Window.on_ink()),
    }
}

pub fn plural(n: u64, word: &str) -> String {
    if n == 1 {
        format!("1 {word}")
    } else {
        format!("{n} {word}s")
    }
}

pub fn badge_text(region: Region, draws: u64, rows: Option<u64>) -> String {
    let mut text = format!(
        "{} \u{b7} {} \u{b7} {}",
        region.name(),
        region.word(),
        plural(draws, "draw")
    );
    if let Some(rows) = rows {
        text.push_str(" \u{b7} ");
        text.push_str(&plural(rows, "row"));
    }
    text
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Reading {
    pub draws: u64,
    pub rows: u64,
    pub flashing: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Track {
    started: bool,
    last_draws: u64,
    last_rows: u64,
    shown: u64,
    rows_in_last: u64,
    flash_until: u64,
    ignore: u64,
}

impl Track {
    pub fn observe(&mut self, draws: u64, rows: u64, now_ms: u64) -> Reading {
        if !self.started {
            self.started = true;
            self.last_draws = draws;
            self.last_rows = rows;
            return Reading::default();
        }
        let delta = draws.saturating_sub(self.last_draws);
        let own = delta.min(self.ignore);
        self.ignore -= own;
        let counted = delta - own;
        if counted > 0 {
            self.shown += counted;
            self.rows_in_last = rows.saturating_sub(self.last_rows);
            self.flash_until = now_ms + FLASH_MS;
        }
        self.last_draws = draws;
        self.last_rows = rows;
        Reading {
            draws: self.shown,
            rows: self.rows_in_last,
            flashing: now_ms < self.flash_until,
        }
    }

    pub fn ignore_next_draw(&mut self) {
        self.ignore += 1;
    }

    pub fn wake_in(&self, now_ms: u64) -> Option<u64> {
        (now_ms < self.flash_until).then(|| self.flash_until - now_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plural_says_one_draw_and_many_draws() {
        assert_eq!(plural(0, "draw"), "0 draws");
        assert_eq!(plural(1, "draw"), "1 draw");
        assert_eq!(plural(2, "draw"), "2 draws");
        assert_eq!(plural(1, "row"), "1 row");
        assert_eq!(plural(12, "row"), "12 rows");
    }

    #[test]
    fn a_part_badge_names_the_part_and_says_it_draws_alone() {
        assert_eq!(
            badge_text(Region::Sidebar, 12, None),
            "Sidebar \u{b7} draws alone \u{b7} 12 draws"
        );
        assert_eq!(
            badge_text(Region::Details, 1, None),
            "Details Panel \u{b7} draws alone \u{b7} 1 draw"
        );
    }

    #[test]
    fn the_conversation_badge_adds_the_rows_of_its_last_draw() {
        assert_eq!(
            badge_text(Region::Conversation, 3, Some(5)),
            "Conversation \u{b7} draws alone \u{b7} 3 draws \u{b7} 5 rows"
        );
        assert_eq!(
            badge_text(Region::Conversation, 1, Some(1)),
            "Conversation \u{b7} draws alone \u{b7} 1 draw \u{b7} 1 row"
        );
    }

    #[test]
    fn the_window_badge_says_it_is_drawn_every_frame() {
        assert_eq!(
            badge_text(Region::Window, 340, None),
            "Window \u{b7} drawn every frame \u{b7} 340 draws"
        );
    }

    #[test]
    fn each_part_has_its_own_ink_and_the_window_a_shared_one() {
        let inks: Vec<Ink> = Region::ALL.iter().map(|r| r.ink()).collect();
        for (i, a) in inks.iter().enumerate() {
            for b in &inks[i + 1..] {
                assert_ne!(a, b);
            }
        }
        assert!(Region::Sidebar.is_part() && Region::Conversation.is_part());
        assert!(Region::Details.is_part() && !Region::Window.is_part());
    }

    #[test]
    fn a_label_takes_the_ink_of_its_region() {
        assert_eq!(
            label_inks(Some(Region::Sidebar)),
            (Ink::Done, Ink::OnAccent)
        );
        assert_eq!(
            label_inks(Some(Region::Conversation)),
            (Ink::Accent, Ink::OnAccent)
        );
        assert_eq!(
            label_inks(Some(Region::Details)),
            (Ink::Shell, Ink::OnShell)
        );
        assert_eq!(label_inks(Some(Region::Window)), (Ink::Guide, Ink::OnGuide));
    }

    #[test]
    fn a_label_outside_any_region_or_with_the_map_off_is_guide() {
        assert_eq!(label_inks(None), (Ink::Guide, Ink::OnGuide));
    }

    #[test]
    fn the_first_reading_is_a_baseline_with_nothing_drawn() {
        let mut t = Track::default();
        let r = t.observe(40, 7, 1000);
        assert_eq!(r, Reading::default());
        assert_eq!(t.wake_in(1000), None);
    }

    #[test]
    fn a_draw_since_the_last_frame_flashes_and_counts() {
        let mut t = Track::default();
        t.observe(40, 0, 0);
        let r = t.observe(41, 0, 100);
        assert_eq!(r.draws, 1);
        assert!(r.flashing);
        assert_eq!(t.wake_in(100), Some(FLASH_MS));
    }

    #[test]
    fn no_draw_no_flash_and_the_count_stays() {
        let mut t = Track::default();
        t.observe(5, 0, 0);
        t.observe(6, 0, 10);
        let r = t.observe(6, 0, 20);
        assert_eq!(r.draws, 1);
        assert!(r.flashing, "still inside the flash");
        let r = t.observe(6, 0, 10 + FLASH_MS);
        assert!(!r.flashing, "the flash ends {FLASH_MS} ms after the draw");
        assert_eq!(r.draws, 1);
        assert_eq!(t.wake_in(10 + FLASH_MS), None);
    }

    #[test]
    fn several_draws_between_frames_count_each() {
        let mut t = Track::default();
        t.observe(0, 0, 0);
        assert_eq!(t.observe(3, 0, 16).draws, 3);
    }

    #[test]
    fn a_new_draw_extends_the_flash() {
        let mut t = Track::default();
        t.observe(0, 0, 0);
        t.observe(1, 0, 10);
        t.observe(2, 0, 200);
        assert!(t.observe(2, 0, 400).flashing);
        assert!(!t.observe(2, 0, 500).flashing);
    }

    #[test]
    fn rows_are_those_built_since_the_frame_before_the_latest_draw() {
        let mut t = Track::default();
        t.observe(1, 10, 0);
        assert_eq!(t.observe(2, 15, 16).rows, 5);
        assert_eq!(t.observe(2, 15, 32).rows, 5);
        assert_eq!(t.observe(3, 15, 48).rows, 0);
    }

    #[test]
    fn a_draw_the_map_caused_is_not_counted_or_flashed() {
        let mut t = Track::default();
        t.observe(100, 0, 0);
        t.ignore_next_draw();
        let r = t.observe(101, 0, 300);
        assert_eq!(r.draws, 0);
        assert!(!r.flashing);
        let r = t.observe(102, 0, 316);
        assert_eq!(r.draws, 1);
        assert!(r.flashing);
    }

    #[test]
    fn an_ignored_draw_that_has_not_happened_yet_waits_for_it() {
        let mut t = Track::default();
        t.observe(10, 0, 0);
        t.ignore_next_draw();
        let r = t.observe(10, 0, 5);
        assert_eq!(r.draws, 0);
        assert!(!r.flashing);
    }

    #[test]
    fn a_counter_that_went_back_does_not_underflow() {
        let mut t = Track::default();
        t.observe(10, 10, 0);
        let r = t.observe(0, 0, 5);
        assert_eq!(r.draws, 0);
    }
}
