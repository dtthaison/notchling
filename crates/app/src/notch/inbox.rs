use super::model::NotchModel;

const MOST: usize = 9;

pub fn badge_text(unread: usize) -> Option<String> {
    match unread {
        0 => None,
        n if n > MOST => Some(format!("{MOST}+")),
        n => Some(n.to_string()),
    }
}

impl NotchModel {
    pub fn set_inbox_unread(&mut self, unread: usize) -> bool {
        let changed = self.inbox_unread != unread;
        self.inbox_unread = unread;
        changed
    }
}

#[cfg(test)]
#[path = "inbox_tests.rs"]
mod tests;
