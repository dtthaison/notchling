use notchling_engine::control::slash::{matches, SlashCommand};

pub const ROWS: usize = 8;

pub fn query(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('/')?;
    (!rest.contains(char::is_whitespace)).then_some(rest)
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SlashMenu {
    pub selected: usize,
    pub hidden_at: Option<String>,
}

impl SlashMenu {
    pub fn rows(&self, text: &str, commands: &[SlashCommand]) -> Vec<SlashCommand> {
        if self.hidden_at.as_deref() == Some(text) {
            return Vec::new();
        }
        match query(text) {
            Some(typed) => matches(typed, commands, ROWS),
            None => Vec::new(),
        }
    }

    pub fn step(&mut self, by: i32, count: usize) {
        if count == 0 {
            self.selected = 0;
            return;
        }
        let count = count as i32;
        let at = (self.selected.min(count as usize - 1) as i32 + by).rem_euclid(count);
        self.selected = at as usize;
    }

    pub fn typed(&mut self, text: &str) {
        self.selected = 0;
        if self.hidden_at.as_deref() != Some(text) {
            self.hidden_at = None;
        }
    }
}

pub fn picked(command: &SlashCommand) -> String {
    format!("/{} ", command.name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn command(name: &str) -> SlashCommand {
        SlashCommand {
            name: name.into(),
            description: String::new(),
            argument_hint: None,
        }
    }

    #[test]
    fn the_list_shows_only_while_a_command_is_typed() {
        assert_eq!(query("/"), Some(""));
        assert_eq!(query("/com"), Some("com"));
        assert_eq!(query("/compact now"), None);
        assert_eq!(query("fix /com"), None);
        assert_eq!(query(""), None);
    }

    #[test]
    fn rows_narrow_as_you_type_and_esc_hides_them_until_the_text_changes() {
        let all = vec![command("clear"), command("compact"), command("context")];
        let mut menu = SlashMenu::default();
        assert_eq!(menu.rows("/", &all).len(), 3);
        let names: Vec<String> = menu.rows("/co", &all).into_iter().map(|c| c.name).collect();
        assert_eq!(names, ["compact", "context"]);
        menu.hidden_at = Some("/co".into());
        assert!(menu.rows("/co", &all).is_empty());
        menu.typed("/com");
        assert_eq!(menu.rows("/com", &all).len(), 1);
    }

    #[test]
    fn up_and_down_wrap_round() {
        let mut menu = SlashMenu::default();
        menu.step(-1, 3);
        assert_eq!(menu.selected, 2);
        menu.step(1, 3);
        assert_eq!(menu.selected, 0);
        menu.selected = 7;
        menu.step(1, 3);
        assert_eq!(menu.selected, 0, "a list that shrank starts from its end");
    }

    #[test]
    fn a_picked_command_leaves_room_for_its_arguments() {
        assert_eq!(picked(&command("review")), "/review ");
    }
}
