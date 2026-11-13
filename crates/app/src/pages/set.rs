pub fn counts_as_open(visible: bool, miniaturized: bool) -> bool {
    visible || miniaturized
}

#[derive(Debug)]
pub struct PageSet<W> {
    items: Vec<(u64, W)>,
}

impl<W> Default for PageSet<W> {
    fn default() -> Self {
        PageSet { items: Vec::new() }
    }
}

impl<W> PageSet<W> {
    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn push(&mut self, id: u64, window: W) {
        self.items.push((id, window));
    }

    pub fn forget(&mut self, id: u64) -> bool {
        let before = self.items.len();
        self.items.retain(|(held, _)| *held != id);
        self.items.len() != before
    }

    pub fn retain_open(&mut self, open: impl Fn(&W) -> bool) {
        self.items.retain(|(_, window)| open(window));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_minimized_window_is_still_open() {
        assert!(counts_as_open(true, false), "on screen");
        assert!(counts_as_open(false, true), "in the Dock");
        assert!(!counts_as_open(false, false), "closed");
    }

    #[derive(Debug)]
    struct Fake {
        visible: bool,
        miniaturized: bool,
    }

    fn open(w: &Fake) -> bool {
        counts_as_open(w.visible, w.miniaturized)
    }

    #[test]
    fn pruning_keeps_the_minimized_and_drops_the_closed() {
        let mut set = PageSet::default();
        set.push(
            1,
            Fake {
                visible: true,
                miniaturized: false,
            },
        );
        set.push(
            2,
            Fake {
                visible: false,
                miniaturized: true,
            },
        );
        set.push(
            3,
            Fake {
                visible: false,
                miniaturized: false,
            },
        );
        set.retain_open(open);
        assert_eq!(set.len(), 2, "the minimized one is kept");
        assert!(!set.forget(3), "the closed one is already gone");
    }

    #[test]
    fn a_close_notice_lets_go_of_that_window_only() {
        let mut set = PageSet::default();
        for id in [10, 11, 12] {
            set.push(
                id,
                Fake {
                    visible: true,
                    miniaturized: false,
                },
            );
        }
        assert!(set.forget(11));
        assert_eq!(set.len(), 2);
        assert!(!set.forget(11), "a second notice finds nothing");
        assert!(!set.forget(99), "nor does a stranger's");
        assert!(set.forget(10) && set.forget(12));
        assert!(set.is_empty());
    }
}
