use std::fmt;

use notchling_engine::accounts::Secret;
use notchling_engine::machines::engine_plan::AdminAnswer;

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
        self.vault_pick = Default::default();
        self.vault_error = None;
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

impl MachinesModel {
    fn sheet_machine(&self) -> Option<String> {
        match &self.sheet {
            Some(Sheet::Fix(prompt)) => Some(prompt.plan.machine.clone()),
            _ => None,
        }
    }

    pub fn admin_entry(&self) -> Option<String> {
        let name = self.sheet_machine()?;
        self.machine(&name)?.admin_secret().map(str::to_owned)
    }

    pub fn password_ready(&self) -> bool {
        AdminAnswer::ready(self.password_len(), self.admin_entry().as_deref())
    }

    pub fn set_admin_entry(&mut self, entry: Option<&str>) {
        let Some(name) = self.sheet_machine() else {
            return;
        };
        self.vault_error = self.parts.engine.set_admin_secret(&name, entry).err();
        if self.vault_error.is_none() && entry.is_some() {
            self.password.0.clear();
        }
        self.refresh_machines();
    }

    pub fn open_vault_pick(&mut self) {
        if self.vault_pick.toggle() {
            let listing = self.parts.vault.listing();
            self.vault_pick.loaded(listing);
        }
    }
}
