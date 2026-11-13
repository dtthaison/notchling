use std::fmt;

use notchling_engine::accounts::Secret;

use super::model::{MachinesModel, Sheet};

#[derive(Default)]
pub struct AdminPassword(String);

impl AdminPassword {
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }

    pub(super) fn take(&mut self) -> Option<Secret> {
        let typed = std::mem::take(&mut self.0);
        (!typed.is_empty()).then(|| Secret::new(typed))
    }
}

impl fmt::Debug for AdminPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AdminPassword(<hidden>)")
    }
}

impl MachinesModel {
    pub fn sheet(&self) -> Option<&Sheet> {
        self.sheet.as_ref()
    }

    pub fn sheet_mut(&mut self) -> Option<&mut Sheet> {
        self.sheet.as_mut()
    }

    pub fn open_sheet(&mut self, sheet: Sheet) -> bool {
        if self.sheet.is_some() {
            return false;
        }
        self.set_sheet(Some(sheet));
        true
    }

    pub fn close_sheet(&mut self) {
        self.cancel_sheet();
    }

    pub(super) fn set_sheet(&mut self, sheet: Option<Sheet>) {
        self.password.clear();
        self.sheet = sheet;
    }

    pub fn password_field(&mut self) -> Option<&mut String> {
        match &self.sheet {
            Some(Sheet::Fix(prompt)) if prompt.needs_password() => Some(&mut self.password.0),
            _ => None,
        }
    }

    pub fn password_len(&self) -> usize {
        self.password.0.chars().count()
    }
}
