use crate::switches::{SwitchFlag, ACCOUNTS_OFF};
use notchling_engine::accounts::generic_password::{Keychain, KeychainError};
use notchling_engine::accounts::procs::{Apps, DidNotQuit, LoginChild, LoginSpawn, Procs};
use notchling_engine::providers::{HttpClient, HttpRequest, HttpResponse};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

pub struct Gated<T: ?Sized> {
    pub flag: Arc<SwitchFlag>,
    pub inner: Arc<T>,
}

impl<T: ?Sized> Gated<T> {
    pub fn new(flag: &Arc<SwitchFlag>, inner: Arc<T>) -> Arc<Gated<T>> {
        Arc::new(Gated {
            flag: flag.clone(),
            inner,
        })
    }

    fn on(&self) -> bool {
        self.flag.is_on()
    }
}

impl<T: Keychain + ?Sized> Keychain for Gated<T> {
    fn read(&self, service: &str, account: &str) -> Option<String> {
        self.on()
            .then(|| self.inner.read(service, account))
            .flatten()
    }

    fn write(&self, service: &str, account: &str, value: &str) -> Result<(), KeychainError> {
        if !self.on() {
            return Err(KeychainError::Unsupported(ACCOUNTS_OFF.into()));
        }
        self.inner.write(service, account, value)
    }

    fn delete(&self, service: &str, account: &str) {
        if self.on() {
            self.inner.delete(service, account);
        }
    }
}

impl<T: HttpClient + ?Sized> HttpClient for Gated<T> {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        if !self.on() {
            return Err(ACCOUNTS_OFF.into());
        }
        self.inner.send(request)
    }
}

impl<T: Procs + ?Sized> Procs for Gated<T> {
    fn capture(&self, program: &str, args: &[&str], timeout: Duration) -> Option<String> {
        self.on()
            .then(|| self.inner.capture(program, args, timeout))
            .flatten()
    }

    fn run_null(&self, program: &Path, args: &[&str], timeout: Duration) -> Result<(), String> {
        if !self.on() {
            return Err(ACCOUNTS_OFF.into());
        }
        self.inner.run_null(program, args, timeout)
    }

    fn spawn_login(&self, spawn: &LoginSpawn) -> Result<Box<dyn LoginChild>, String> {
        if !self.on() {
            return Err(ACCOUNTS_OFF.into());
        }
        self.inner.spawn_login(spawn)
    }

    fn find_binary(&self, name: &str) -> Option<PathBuf> {
        self.on().then(|| self.inner.find_binary(name)).flatten()
    }
}

impl<T: Apps + ?Sized> Apps for Gated<T> {
    fn is_running(&self, bundle_id: &str) -> bool {
        !self.on() || self.inner.is_running(bundle_id)
    }

    fn quit(&self, bundle_id: &str) -> Result<bool, DidNotQuit> {
        if !self.on() {
            return Err(DidNotQuit);
        }
        self.inner.quit(bundle_id)
    }

    fn open(&self, bundle_id: &str) {
        if self.on() {
            self.inner.open(bundle_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::switches::ACCOUNTS;
    use notchling_engine::accounts::generic_password::FakeKeychain;
    use notchling_engine::accounts::procs::{FakeApps, FakeProcs};
    use notchling_engine::providers::{FakeHttp, Trust};

    #[test]
    fn off_nothing_is_reached_and_on_everything_is() {
        let flag = SwitchFlag::in_memory(ACCOUNTS, ACCOUNTS_OFF, false);
        let keychain = Arc::new(FakeKeychain::new().with("s", "a", "v"));
        let http = Arc::new(FakeHttp::new());
        let procs = Arc::new(FakeProcs::new().binary("codex", Path::new("/bin/codex")));
        let apps = Arc::new(FakeApps::new());
        let (k, h, p, a) = (
            Gated::new(&flag, keychain.clone()),
            Gated::new(&flag, http.clone()),
            Gated::new(&flag, procs.clone()),
            Gated::new(&flag, apps.clone()),
        );
        let request = HttpRequest {
            method: "GET",
            url: "https://example.com/".into(),
            headers: vec![],
            body: None,
            timeout: Duration::from_secs(1),
            trust: Trust::System,
        };
        assert_eq!(k.read("s", "a"), None);
        assert!(k.write("s", "a", "w").is_err());
        k.delete("s", "a");
        assert!(h.send(&request).is_err());
        assert_eq!(p.find_binary("codex"), None);
        assert!(p.capture("/bin/ps", &[], Duration::from_secs(1)).is_none());
        assert!(p
            .run_null(Path::new("/bin/x"), &[], Duration::from_secs(1))
            .is_err());
        assert!(a.is_running("com.openai.codex"), "off counts as running");
        assert_eq!(a.quit("com.openai.codex"), Err(DidNotQuit));
        a.open("com.openai.codex");
        assert!(keychain.calls().is_empty());
        assert_eq!(keychain.get("s", "a").as_deref(), Some("v"));
        assert_eq!(http.calls(), 0);
        assert!(procs.calls().is_empty());
        assert!(apps.events().is_empty());

        flag.set(true);
        assert_eq!(k.read("s", "a").as_deref(), Some("v"));
        assert_eq!(p.find_binary("codex"), Some(PathBuf::from("/bin/codex")));
        assert!(!a.is_running("com.openai.codex"));
        assert_eq!(keychain.calls().len(), 1);
        assert_eq!(procs.calls().len(), 1);
    }
}
