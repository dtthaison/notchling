use crate::menu::{MenuState, TrayLook};

pub trait TrayPort {
    fn set_look(&mut self, look: &TrayLook);
    fn set_menu(&mut self, state: &MenuState);
}

#[derive(Default)]
pub struct TraySync {
    look: Option<TrayLook>,
    menu: Option<MenuState>,
}

impl TraySync {
    pub fn refresh(&mut self, port: &mut dyn TrayPort, look: TrayLook, menu: &MenuState) {
        if self.look.as_ref() != Some(&look) {
            port.set_look(&look);
            self.look = Some(look);
        }
        if self.menu.as_ref() != Some(menu) {
            port.set_menu(menu);
            self.menu = Some(menu.clone());
        }
    }
}

#[cfg(test)]
#[path = "tray_sync_tests.rs"]
mod tests;
