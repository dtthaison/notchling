use std::fmt;

use notchling_engine::vault::entry::{
    default_vars, valid_name, valid_var, Field as Changed, Kind, PrefillChanges, VaultEntry,
    VaultValue,
};
use notchling_engine::vault::expiry;
use notchling_engine::vault::url::renew_refusal;

use super::wire::PendingRequest;

pub const CONFIRM_HOST_BLOCKER: &str = "Confirm the new address before saving.";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Add,
    Edit,
    Renew,
    Request,
}

impl Mode {
    pub fn title(self) -> &'static str {
        match self {
            Mode::Add => "Add Secret",
            Mode::Edit => "Edit Secret",
            Mode::Renew => "Renew Secret",
            Mode::Request => "Secret Vault",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Name,
    Url,
    Username,
    Secret,
    Note,
    HowTo,
    Expires,
    UserVar,
    SecretVar,
}

impl Field {
    pub fn multiline(self) -> bool {
        matches!(self, Field::Note | Field::HowTo)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Marker {
    pub text: &'static str,
    pub was: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct Form {
    pub mode: Mode,
    pub original: Option<VaultEntry>,
    pub request: Option<String>,
    pub reason: Option<String>,
    pub name: String,
    pub url: String,
    pub kind: Kind,
    pub username: String,
    pub secret: String,
    pub note: String,
    pub how_to: String,
    pub editing_how_to: bool,
    pub expires: String,
    pub expiry_chosen: bool,
    pub no_expiry: bool,
    pub ask: bool,
    pub user_var: String,
    pub secret_var: String,
    pub custom_vars: bool,
    pub advanced: bool,
    pub focus: Field,
    pub changes: PrefillChanges,
    pub host_confirmed: bool,
    pub error: Option<String>,
}

impl fmt::Debug for Form {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Form")
            .field("mode", &self.mode)
            .field("name", &self.name)
            .field("secret", &"<hidden>")
            .finish_non_exhaustive()
    }
}

impl Form {
    pub fn add() -> Form {
        let (user_var, secret_var) = default_vars("", Kind::Token);
        Form {
            mode: Mode::Add,
            original: None,
            request: None,
            reason: None,
            name: String::new(),
            url: String::new(),
            kind: Kind::Token,
            username: String::new(),
            secret: String::new(),
            note: String::new(),
            how_to: String::new(),
            editing_how_to: true,
            expires: String::new(),
            expiry_chosen: true,
            no_expiry: true,
            ask: false,
            user_var,
            secret_var,
            custom_vars: false,
            advanced: false,
            focus: Field::Name,
            changes: PrefillChanges::default(),
            host_confirmed: false,
            error: None,
        }
    }

    fn from_entry(entry: &VaultEntry, mode: Mode) -> Form {
        let defaults = default_vars(&entry.name, entry.kind);
        let custom = entry.secret_var != defaults.1
            || (entry.kind == Kind::Login && entry.user_var != defaults.0);
        Form {
            mode,
            original: Some(entry.clone()),
            name: entry.name.clone(),
            url: entry.url.clone().unwrap_or_default(),
            kind: entry.kind,
            note: entry.note.clone(),
            how_to: entry.how_to.clone().unwrap_or_default(),
            editing_how_to: entry.how_to.as_deref().is_none_or(|h| h.trim().is_empty()),
            expires: entry.expires.clone().unwrap_or_default(),
            no_expiry: entry.expires.is_none(),
            ask: entry.asks(),
            user_var: entry.user_var.clone(),
            secret_var: entry.secret_var.clone(),
            custom_vars: custom,
            focus: Field::Secret,
            ..Form::add()
        }
    }

    pub fn edit(entry: &VaultEntry) -> Form {
        let mut form = Form::from_entry(entry, Mode::Edit);
        if entry.kind == Kind::Login {
            form.focus = Field::Username;
        }
        form
    }

    pub fn renew(entry: &VaultEntry) -> Form {
        let mut form = Form::from_entry(entry, Mode::Renew);
        form.expires.clear();
        form.expiry_chosen = false;
        form.no_expiry = false;
        form
    }

    pub fn request(pending: &PendingRequest) -> Form {
        let mut form = Form::from_entry(&pending.entry, Mode::Request);
        if !pending.existing {
            form.original = None;
            form.focus = Field::Secret;
        }
        form.request = Some(pending.id.clone());
        form.reason = pending
            .request
            .reason
            .clone()
            .filter(|r| !r.trim().is_empty());
        form.changes = pending.changes.clone();
        form
    }

    pub fn title(&self) -> &'static str {
        self.mode.title()
    }

    pub fn reason_line(&self) -> Option<String> {
        self.reason.as_ref().map(|r| format!("An agent asks: {r}"))
    }

    pub fn is_login(&self) -> bool {
        self.kind == Kind::Login
    }

    pub fn fields(&self) -> Vec<Field> {
        let mut f = vec![Field::Name, Field::Url];
        if self.is_login() {
            f.push(Field::Username);
        }
        f.push(Field::Secret);
        f.push(Field::Note);
        if self.editing_how_to {
            f.push(Field::HowTo);
        }
        if !self.no_expiry && self.expiry_chosen {
            f.push(Field::Expires);
        }
        if self.advanced {
            if self.is_login() {
                f.push(Field::UserVar);
            }
            f.push(Field::SecretVar);
        }
        f
    }

    pub fn text(&self, field: Field) -> &str {
        match field {
            Field::Name => &self.name,
            Field::Url => &self.url,
            Field::Username => &self.username,
            Field::Secret => &self.secret,
            Field::Note => &self.note,
            Field::HowTo => &self.how_to,
            Field::Expires => &self.expires,
            Field::UserVar => &self.user_var,
            Field::SecretVar => &self.secret_var,
        }
    }

    fn text_mut(&mut self, field: Field) -> &mut String {
        match field {
            Field::Name => &mut self.name,
            Field::Url => &mut self.url,
            Field::Username => &mut self.username,
            Field::Secret => &mut self.secret,
            Field::Note => &mut self.note,
            Field::HowTo => &mut self.how_to,
            Field::Expires => &mut self.expires,
            Field::UserVar => &mut self.user_var,
            Field::SecretVar => &mut self.secret_var,
        }
    }

    pub fn secret_placeholder(&self) -> &'static str {
        match (self.mode, self.original.is_some()) {
            (Mode::Renew, _) => "New value",
            (_, true) => "Unchanged",
            _ => "",
        }
    }

    pub fn expires_label(&self) -> &'static str {
        if self.mode == Mode::Renew {
            "New expiry"
        } else {
            "Expires"
        }
    }

    pub fn secret_mask(&self) -> String {
        "•".repeat(self.secret.chars().count())
    }

    pub fn key(&mut self, key: &str, key_char: Option<&str>, modified: bool) -> Typed {
        let field = self.focus;
        match key {
            "tab" => {
                self.focus_next();
                Typed::Changed
            }
            "enter" if field.multiline() => {
                self.text_mut(field).push('\n');
                Typed::Changed
            }
            "enter" if self.mode == Mode::Request => Typed::Ignored,
            "enter" => Typed::Submit,
            "backspace" => {
                if self.text_mut(field).pop().is_some() {
                    self.after_typing(field);
                    Typed::Changed
                } else {
                    Typed::Ignored
                }
            }
            _ if modified => Typed::Ignored,
            _ => match key_char {
                Some(c) if !c.is_empty() && !c.chars().any(char::is_control) => {
                    self.text_mut(field).push_str(c);
                    self.after_typing(field);
                    Typed::Changed
                }
                _ => Typed::Ignored,
            },
        }
    }

    fn after_typing(&mut self, field: Field) {
        match field {
            Field::Name if !self.custom_vars => self.reset_vars(),
            Field::UserVar | Field::SecretVar => self.custom_vars = true,
            _ => {}
        }
        self.error = None;
    }

    fn reset_vars(&mut self) {
        let (user, secret) = default_vars(&self.name, self.kind);
        self.user_var = user;
        self.secret_var = secret;
    }

    pub fn focus_next(&mut self) {
        let fields = self.fields();
        let at = fields.iter().position(|f| *f == self.focus).unwrap_or(0);
        self.focus = fields[(at + 1) % fields.len()];
    }

    pub fn set_kind(&mut self, kind: Kind) {
        self.kind = kind;
        if !self.custom_vars {
            self.reset_vars();
        }
        if self.focus == Field::Username && kind == Kind::Token {
            self.focus = Field::Secret;
        }
    }

    pub fn choose_expiry(&mut self, no_expiry: bool, today: i64) {
        self.expiry_chosen = true;
        self.no_expiry = no_expiry;
        if !no_expiry && self.expires.is_empty() {
            self.expires = expiry::day_text(today + 30);
        }
        self.error = None;
    }

    pub fn vars_summary(&self) -> String {
        let mut vars = Vec::new();
        if self.is_login() {
            vars.push(self.user_var.clone());
        }
        vars.push(self.secret_var.clone());
        if !self.url.trim().is_empty() {
            vars.push(format!(
                "{}_URL",
                notchling_engine::vault::entry::prefix(&self.name)
            ));
        }
        vars.iter()
            .map(|v| format!("${v}"))
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn marker(&self, field: Changed) -> Option<Marker> {
        let old = self.changes.old.get(&field)?;
        let none = match field {
            Changed::Note => "nothing",
            Changed::Url => "no URL",
            Changed::HowTo => "nothing",
            Changed::Expires => "no expiry",
        };
        Some(Marker {
            text: field.marker(),
            was: if old.trim().is_empty() {
                format!("Was: {none}")
            } else {
                format!("Was: {old}")
            },
        })
    }

    pub fn host_warning(&self) -> Option<String> {
        self.changes.host_change.as_ref().map(|h| h.warning())
    }

    pub fn renew_note(&self) -> Option<String> {
        if self.mode != Mode::Renew {
            return None;
        }
        renew_refusal(Some(self.url.as_str()))
            .map(|url| format!("This address isn't http or https, so Renew won't open it: {url}"))
    }

    pub fn can_save(&self) -> bool {
        if self.name.trim().is_empty() || self.host_unconfirmed() {
            return false;
        }
        match self.mode {
            Mode::Renew => !self.secret.is_empty() && self.expiry_chosen,
            _ => self.original.is_some() || !self.secret.is_empty(),
        }
    }

    pub fn blocker(&self) -> Option<&'static str> {
        if self.host_unconfirmed() {
            return Some(CONFIRM_HOST_BLOCKER);
        }
        if self.mode != Mode::Renew {
            return None;
        }
        if self.secret.is_empty() {
            Some("Enter the new value.")
        } else if !self.expiry_chosen {
            Some("Pick the new expiry date, or No expiry.")
        } else {
            None
        }
    }

    pub fn to_save(&self) -> Result<(VaultEntry, Option<VaultValue>, Option<String>), String> {
        let name = self.name.trim();
        if !valid_name(name) {
            return Err(
                "The name starts with a letter and has only letters, digits, _ . and -.".into(),
            );
        }
        if !valid_var(&self.secret_var) || (self.is_login() && !valid_var(&self.user_var)) {
            return Err(
                "A variable name has only letters, digits and _, and doesn't start with a digit."
                    .into(),
            );
        }
        let expires = match (self.no_expiry, self.expires.trim()) {
            (true, _) | (false, "") => None,
            (false, day) => Some(expiry::parse(day).ok_or("The expiry is a day as YYYY-MM-DD.")?),
        };
        let mut entry = match &self.original {
            Some(o) => o.renamed(name),
            None => VaultEntry::new(name, self.kind),
        };
        entry.kind = self.kind;
        entry.url = Some(self.url.trim().to_owned()).filter(|u| !u.is_empty());
        entry.note = self.note.trim().to_owned();
        entry.how_to = Some(self.how_to.trim().to_owned()).filter(|h| !h.is_empty());
        entry.expires = expires;
        entry.ask = self.ask.then_some(true);
        let (user_var, secret_var) = if self.custom_vars {
            (self.user_var.clone(), self.secret_var.clone())
        } else {
            default_vars(name, self.kind)
        };
        entry.secret_var = secret_var;
        entry.user_var = if self.is_login() {
            user_var
        } else {
            String::new()
        };
        let value = (!self.secret.is_empty()).then(|| VaultValue {
            username: self.username.clone(),
            secret: self.secret.clone(),
        });
        let replacing = self
            .original
            .as_ref()
            .filter(|o| o.name != name)
            .map(|o| o.name.clone());
        Ok((entry, value, replacing))
    }

    pub fn paste(&mut self, text: &str) -> Typed {
        let field = self.focus;
        let clean: String = text
            .chars()
            .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
            .collect();
        let clean = if field.multiline() {
            clean
        } else {
            clean.trim_end_matches('\n').replace('\n', " ")
        };
        if clean.is_empty() {
            return Typed::Ignored;
        }
        self.text_mut(field).push_str(&clean);
        self.after_typing(field);
        Typed::Changed
    }

    pub fn copy(&self) -> Option<String> {
        (self.focus != Field::Secret).then(|| self.text(self.focus).to_owned())
    }

    pub fn cut(&mut self) -> Option<String> {
        let text = self.copy()?;
        let field = self.focus;
        self.text_mut(field).clear();
        self.after_typing(field);
        Some(text)
    }

    pub fn confirm_host(&mut self) {
        self.host_confirmed = !self.host_confirmed;
        self.error = None;
    }

    pub fn host_unconfirmed(&self) -> bool {
        self.mode == Mode::Request && self.changes.host_change.is_some() && !self.host_confirmed
    }

    pub fn forget_value(&mut self) {
        self.secret.clear();
        self.username.clear();
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Typed {
    Changed,
    Submit,
    Ignored,
}

#[cfg(test)]
#[path = "form_tests.rs"]
mod tests;
