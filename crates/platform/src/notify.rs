//! User notifications.
//!
//! [`Notifier`] is what the rest of the code calls; tests use [`RecordingNotifier`]. The macOS
//! implementation is not exercised by tests, and never goes through `osascript`, whose banners
//! carry Script Editor's name and icon.

use std::sync::Mutex;

use crate::{Error, Result};

/// One banner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
}

impl Notification {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Notification {
        Notification {
            title: title.into(),
            body: body.into(),
        }
    }
}

/// Shows a banner to the user.
pub trait Notifier: Send + Sync {
    fn notify(&self, notification: &Notification) -> Result<()>;
}

/// Keeps what it was asked to show, for tests.
#[derive(Default)]
pub struct RecordingNotifier {
    sent: Mutex<Vec<Notification>>,
}

impl RecordingNotifier {
    pub fn new() -> RecordingNotifier {
        RecordingNotifier::default()
    }

    /// Everything shown so far, oldest first.
    pub fn sent(&self) -> Vec<Notification> {
        match self.sent.lock() {
            Ok(list) => list.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        }
    }
}

impl Notifier for RecordingNotifier {
    fn notify(&self, notification: &Notification) -> Result<()> {
        match self.sent.lock() {
            Ok(mut list) => list.push(notification.clone()),
            Err(poisoned) => poisoned.into_inner().push(notification.clone()),
        }
        Ok(())
    }
}

/// The notifier for this system: the notification center on macOS, otherwise one that says it is
/// not supported yet.
pub fn system_notifier() -> Box<dyn Notifier> {
    #[cfg(target_os = "macos")]
    {
        Box::new(mac::UserNotificationCenter)
    }
    #[cfg(not(target_os = "macos"))]
    {
        Box::new(Unsupported)
    }
}

/// Fails every call with [`Error::Unsupported`].
pub struct Unsupported;

impl Notifier for Unsupported {
    fn notify(&self, _notification: &Notification) -> Result<()> {
        Err(Error::Unsupported("notifications"))
    }
}

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use block2::RcBlock;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSBundle, NSError, NSString};
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNMutableNotificationContent, UNNotificationRequest,
        UNUserNotificationCenter,
    };
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::mpsc;
    use std::time::Duration;

    /// How long to wait for macOS to answer before giving up.
    const ANSWER_WAIT: Duration = Duration::from_secs(10);

    /// macOS's notification center (`UNUserNotificationCenter`). Not covered by tests: it needs an
    /// app bundle and the user's permission. A process without a bundle (the daemon on its own) gets
    /// [`Error::Unsupported`] instead of the exception macOS would throw; the app then shows its own
    /// banner window, as the Swift app does when macOS refuses an ad hoc signed build.
    pub struct UserNotificationCenter;

    fn has_bundle() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    fn message(error: *mut NSError) -> Option<String> {
        // SAFETY: macOS passes either null or a live NSError for the duration of the callback.
        unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string())
    }

    impl Notifier for UserNotificationCenter {
        fn notify(&self, notification: &Notification) -> Result<()> {
            static N: AtomicU64 = AtomicU64::new(0);
            if !has_bundle() {
                return Err(Error::Unsupported(
                    "system notifications without an app bundle",
                ));
            }
            let center = UNUserNotificationCenter::currentNotificationCenter();

            let (grant_tx, grant_rx) = mpsc::channel::<std::result::Result<bool, String>>();
            let ask = RcBlock::new(move |granted: Bool, error: *mut NSError| {
                let _ = grant_tx.send(match message(error) {
                    Some(why) => Err(why),
                    None => Ok(granted.as_bool()),
                });
            });
            center.requestAuthorizationWithOptions_completionHandler(
                UNAuthorizationOptions::Alert | UNAuthorizationOptions::Sound,
                &ask,
            );
            match grant_rx.recv_timeout(ANSWER_WAIT) {
                Ok(Ok(true)) => {}
                Ok(Ok(false)) => return Err(Error::System("notifications are not allowed".into())),
                Ok(Err(why)) => return Err(Error::System(why)),
                Err(_) => return Err(Error::System("macOS did not answer in time".into())),
            }

            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&notification.title));
            content.setBody(&NSString::from_str(&notification.body));
            let id = format!("notchling-{}", N.fetch_add(1, Ordering::SeqCst));
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(&id),
                &content,
                None,
            );
            let (done_tx, done_rx) = mpsc::channel::<Option<String>>();
            let added = RcBlock::new(move |error: *mut NSError| {
                let _ = done_tx.send(message(error));
            });
            center.addNotificationRequest_withCompletionHandler(&request, Some(&added));
            match done_rx.recv_timeout(ANSWER_WAIT) {
                Ok(None) => Ok(()),
                Ok(Some(why)) => Err(Error::System(why)),
                Err(_) => Err(Error::System("macOS did not answer in time".into())),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recording_notifier_keeps_order() {
        let n = RecordingNotifier::new();
        n.notify(&Notification::new("a", "1")).unwrap();
        n.notify(&Notification::new("b", "2")).unwrap();
        let sent = n.sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].title, "a");
        assert_eq!(sent[1].body, "2");
    }

    #[test]
    fn works_through_a_trait_object() {
        let n: Box<dyn Notifier> = Box::new(RecordingNotifier::new());
        assert!(n.notify(&Notification::new("t", "b")).is_ok());
    }

    #[test]
    fn unsupported_notifier_says_so() {
        let err = Unsupported
            .notify(&Notification::new("t", "b"))
            .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }
}
