use super::{lock, login, AntigravityStore, Inner, BUNDLE_IDS, EMAIL_ID_PREFIX};
use crate::accounts::book::Saved;
use crate::providers::{AccountError, AddProgress};

impl AntigravityStore {
    pub(super) fn resolve(&self, login: &str) -> Option<Saved> {
        let mut account = login::account_in(login)?;
        if account.id.starts_with(EMAIL_ID_PREFIX) {
            if let Some(known) = lock(&self.inner)
                .accounts
                .iter()
                .find(|a| a.email.to_lowercase() == account.email.to_lowercase())
            {
                account = Saved {
                    id: known.id.clone(),
                    email: known.email.clone(),
                    plan: None,
                    organization: known.organization.clone(),
                };
            }
        }
        Some(account)
    }

    pub(super) fn record(&self, inner: &mut Inner, account: &Saved, login: String) {
        let before = inner.accounts.clone();
        match inner.accounts.iter_mut().find(|a| a.id == account.id) {
            Some(a) => a.email = account.email.clone(),
            None => inner.accounts.push(account.clone()),
        }
        if inner.accounts != before {
            self.save_book(&inner.accounts);
        }
        inner.last_saved = Some(login);
    }

    fn copy_matches(&self, id: &str, login: &str) -> bool {
        let matches = || matches!(self.saved_copy(id), Ok(Some(saved)) if saved == login);
        matches() || (self.save_copy(id, login).is_ok() && matches())
    }

    fn keep_live(&self) -> Result<Option<String>, AccountError> {
        let Some(login) = self.read_login() else {
            lock(&self.inner).active = None;
            return Ok(None);
        };
        let unsaved = || {
            AccountError::new(
                "Couldn't save the current Antigravity login, so it was left in place.",
            )
        };
        let account = self.resolve(&login).ok_or_else(unsaved)?;
        if !self.copy_matches(&account.id, &login) {
            return Err(unsaved());
        }
        Self::fits_live_item(&login)?;
        let mut inner = lock(&self.inner);
        self.record(&mut inner, &account, login);
        inner.active = Some(account.id.clone());
        Ok(Some(account.id))
    }

    pub(super) fn begin_add_locked(&self) -> Result<(), AccountError> {
        let active = self.keep_live()?;
        let reopen = self.quit_apps()?;
        self.sign_out();
        let open = if reopen.is_empty() {
            vec![BUNDLE_IDS[0].to_string()]
        } else {
            reopen
        };
        let mut inner = lock(&self.inner);
        inner.last_saved = None;
        inner.before_add = active;
        inner.active = None;
        inner.added = None;
        inner.adding = true;
        drop(inner);
        self.open_apps(&open);
        Ok(())
    }

    pub(super) fn poll_add_locked(&self) -> AddProgress {
        if !lock(&self.inner).adding {
            return AddProgress::Idle;
        }
        let read = self
            .read_login()
            .and_then(|login| Some((self.resolve(&login)?, login)));
        let Some((account, login)) = read else {
            return AddProgress::Waiting { sign_in_url: None };
        };
        let saved = self.copy_matches(&account.id, &login);
        let mut inner = lock(&self.inner);
        if !inner.adding {
            return AddProgress::Cancelled;
        }
        inner.adding = false;
        inner.before_add = None;
        if !saved {
            return AddProgress::Failed(AccountError::new(
                "Couldn't save the new Antigravity login, so it wasn't added.",
            ));
        }
        self.record(&mut inner, &account, login);
        inner.active = Some(account.id.clone());
        inner.added = Some(account.id.clone());
        drop(inner);
        self.refresh_usage(Some(account.id));
        AddProgress::Done
    }

    pub(super) fn end_add(&self) -> Option<Option<String>> {
        let mut inner = lock(&self.inner);
        if !inner.adding {
            return None;
        }
        inner.adding = false;
        Some(inner.before_add.take())
    }
}
