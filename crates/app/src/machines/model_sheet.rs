//! The sheet on show and the admin password typed into it. One sheet at a time: while one is open
//! a request for another (a click that reached a card under it) is refused, so the Run on screen is
//! always the plan it shows. The password lives only while its sheet does: every change of sheet
//! clears it, and the one confirm takes it.

use std::fmt;

use notchling_engine::accounts::Secret;

use super::model::{MachinesModel, Sheet};

/// The Fix sheet's admin password, for its one Run. `Debug` never prints it.
#[derive(Default)]
pub struct AdminPassword(String);

impl AdminPassword {
    pub(super) fn clear(&mut self) {
        self.0.clear();
    }

    /// The password for the one confirm, leaving none behind.
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

    /// The sheet's fields, for typing. It doesn't change which sheet is on show.
    pub fn sheet_mut(&mut self) -> Option<&mut Sheet> {
        self.sheet.as_mut()
    }

    /// Shows an Add or Job sheet, unless a sheet is on show already. Whether it was shown.
    pub fn open_sheet(&mut self, sheet: Sheet) -> bool {
        if self.sheet.is_some() {
            return false;
        }
        self.set_sheet(Some(sheet));
        true
    }

    /// Closes the sheet on show: a Fix sheet as its Cancel does.
    pub fn close_sheet(&mut self) {
        self.cancel_sheet();
    }

    /// Every change of sheet comes here, and the password goes with the old one.
    pub(super) fn set_sheet(&mut self, sheet: Option<Sheet>) {
        self.password.clear();
        self.sheet = sheet;
    }

    /// The password field, while the sheet on show asks for one.
    pub fn password_field(&mut self) -> Option<&mut String> {
        match &self.sheet {
            Some(Sheet::Fix(prompt)) if prompt.needs_password() => Some(&mut self.password.0),
            _ => None,
        }
    }

    /// For the dots and for whether Run may be pressed.
    pub fn password_len(&self) -> usize {
        self.password.0.chars().count()
    }
}
