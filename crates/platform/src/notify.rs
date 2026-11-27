use std::collections::BTreeSet;
use std::sync::{Arc, Mutex};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BannerAction {
    Allow,
    Deny,
    Change,
    Open,
    Reply,
    Dismiss,
    OpenForm,
    Cancel,
}

impl BannerAction {
    pub fn id(self) -> &'static str {
        match self {
            BannerAction::Allow => "allow",
            BannerAction::Deny => "deny",
            BannerAction::Change => "change",
            BannerAction::Open => "open",
            BannerAction::Reply => "reply",
            BannerAction::Dismiss => "dismiss",
            BannerAction::OpenForm => "open_form",
            BannerAction::Cancel => "cancel",
        }
    }

    pub fn from_id(id: &str) -> Option<BannerAction> {
        [
            BannerAction::Allow,
            BannerAction::Deny,
            BannerAction::Change,
            BannerAction::Open,
            BannerAction::Reply,
            BannerAction::Dismiss,
            BannerAction::OpenForm,
            BannerAction::Cancel,
        ]
        .into_iter()
        .find(|a| a.id() == id)
    }

    pub fn needs_unlock(self) -> bool {
        self == BannerAction::Allow
    }

    pub fn opens_app(self) -> bool {
        matches!(
            self,
            BannerAction::Open
                | BannerAction::Change
                | BannerAction::Reply
                | BannerAction::OpenForm
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Category {
    InboxAllow,
    InboxOpen,
    InboxOpenOnly,
    SecretRequest,
    Ask,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::InboxAllow,
        Category::InboxOpen,
        Category::InboxOpenOnly,
        Category::SecretRequest,
        Category::Ask,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Category::InboxAllow => "inbox-allow",
            Category::InboxOpen => "inbox-open",
            Category::InboxOpenOnly => "inbox-open-only",
            Category::SecretRequest => "secret-request",
            Category::Ask => "ask",
        }
    }

    pub fn actions(self) -> &'static [BannerAction] {
        match self {
            Category::InboxAllow => &[
                BannerAction::Allow,
                BannerAction::Deny,
                BannerAction::Change,
            ],
            Category::InboxOpen => &[BannerAction::Deny, BannerAction::Open, BannerAction::Change],
            Category::InboxOpenOnly => &[BannerAction::Deny, BannerAction::Open],
            Category::SecretRequest => &[BannerAction::OpenForm, BannerAction::Cancel],
            Category::Ask => &[BannerAction::Reply, BannerAction::Dismiss],
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Notification {
    pub title: String,
    pub body: String,
    pub category: Option<Category>,
    pub thread: Option<String>,
    pub sound: Option<String>,
}

impl Notification {
    pub fn new(title: impl Into<String>, body: impl Into<String>) -> Notification {
        Notification {
            title: title.into(),
            body: body.into(),
            category: None,
            thread: None,
            sound: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Authorization {
    Allowed,
    Denied,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Response {
    pub id: String,
    pub action: Option<BannerAction>,
}

pub type ResponseHandler = Arc<dyn Fn(Response) + Send + Sync>;

pub trait Notifier: Send + Sync {
    fn upsert(&self, id: &str, notification: &Notification) -> Result<()>;
    fn remove(&self, id: &str) -> Result<()>;
    fn request_authorization(&self) -> Result<()>;
    fn register_categories(&self, title_of: &dyn Fn(BannerAction) -> String) -> Result<()>;
    fn authorization(&self) -> Authorization;
    fn delivered(&self) -> Vec<String>;
}

#[derive(Default)]
struct Record {
    upserts: Vec<(String, Notification)>,
    removed: Vec<String>,
    delivered: BTreeSet<String>,
    denied: bool,
    registered: usize,
    asked: usize,
}

#[derive(Default)]
pub struct RecordingNotifier {
    record: Mutex<Record>,
}

impl RecordingNotifier {
    pub fn new() -> RecordingNotifier {
        RecordingNotifier::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Record> {
        self.record.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn sent(&self) -> Vec<Notification> {
        self.lock().upserts.iter().map(|(_, n)| n.clone()).collect()
    }

    pub fn upserts(&self) -> Vec<(String, Notification)> {
        self.lock().upserts.clone()
    }

    pub fn removed(&self) -> Vec<String> {
        self.lock().removed.clone()
    }

    pub fn deny(&self) {
        self.lock().denied = true;
    }

    pub fn registered(&self) -> usize {
        self.lock().registered
    }

    pub fn asked(&self) -> usize {
        self.lock().asked
    }

    pub fn simulate_action(&self, id: &str, action: BannerAction) -> Option<Response> {
        let record = self.lock();
        let (_, shown) = record.upserts.iter().rev().find(|(held, _)| held == id)?;
        let offered = shown
            .category
            .is_some_and(|c| c.actions().contains(&action));
        if !offered || !record.delivered.contains(id) {
            return None;
        }
        Some(Response {
            id: id.to_owned(),
            action: Some(action),
        })
    }
}

impl Notifier for RecordingNotifier {
    fn upsert(&self, id: &str, notification: &Notification) -> Result<()> {
        let mut record = self.lock();
        if record.denied {
            return Err(Error::System("notifications are not allowed".into()));
        }
        record.delivered.insert(id.to_owned());
        record.upserts.push((id.to_owned(), notification.clone()));
        Ok(())
    }

    fn remove(&self, id: &str) -> Result<()> {
        let mut record = self.lock();
        record.delivered.remove(id);
        record.removed.push(id.to_owned());
        Ok(())
    }

    fn request_authorization(&self) -> Result<()> {
        self.lock().asked += 1;
        Ok(())
    }

    fn register_categories(&self, _title_of: &dyn Fn(BannerAction) -> String) -> Result<()> {
        self.lock().registered += 1;
        Ok(())
    }

    fn authorization(&self) -> Authorization {
        if self.lock().denied {
            Authorization::Denied
        } else {
            Authorization::Allowed
        }
    }

    fn delivered(&self) -> Vec<String> {
        self.lock().delivered.iter().cloned().collect()
    }
}

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

pub struct Unsupported;

impl Notifier for Unsupported {
    fn upsert(&self, _id: &str, _notification: &Notification) -> Result<()> {
        Err(Error::Unsupported("notifications"))
    }

    fn remove(&self, _id: &str) -> Result<()> {
        Err(Error::Unsupported("notifications"))
    }

    fn request_authorization(&self) -> Result<()> {
        Err(Error::Unsupported("notifications"))
    }

    fn register_categories(&self, _title_of: &dyn Fn(BannerAction) -> String) -> Result<()> {
        Err(Error::Unsupported("notifications"))
    }

    fn authorization(&self) -> Authorization {
        Authorization::Denied
    }

    fn delivered(&self) -> Vec<String> {
        Vec::new()
    }
}

#[cfg(target_os = "macos")]
pub use mac::{install_delegate, DelegateGuard};

#[cfg(target_os = "macos")]
mod mac {
    use super::*;
    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{Bool, ProtocolObject};
    use objc2::{define_class, msg_send, AnyThread, DefinedClass, MainThreadMarker};
    use objc2_foundation::{
        NSArray, NSBundle, NSError, NSObject, NSObjectProtocol, NSSet, NSString,
    };
    use objc2_user_notifications::{
        UNAuthorizationOptions, UNAuthorizationStatus, UNMutableNotificationContent,
        UNNotification, UNNotificationAction, UNNotificationActionOptions, UNNotificationCategory,
        UNNotificationCategoryOptions, UNNotificationPresentationOptions, UNNotificationRequest,
        UNNotificationResponse, UNNotificationSettings, UNNotificationSound,
        UNUserNotificationCenter, UNUserNotificationCenterDelegate,
    };
    use std::ptr::NonNull;
    use std::sync::mpsc;
    use std::time::Duration;

    const ANSWER_WAIT: Duration = Duration::from_secs(10);
    const DEFAULT_ACTION: &str = "com.apple.UNNotificationDefaultActionIdentifier";

    pub struct UserNotificationCenter;

    fn has_bundle() -> bool {
        NSBundle::mainBundle().bundleIdentifier().is_some()
    }

    fn message(error: *mut NSError) -> Option<String> {
        // SAFETY: macOS passes either null or a live NSError for the duration of the callback.
        unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string())
    }

    fn center() -> Result<Retained<UNUserNotificationCenter>> {
        if !has_bundle() {
            return Err(Error::Unsupported(
                "system notifications without an app bundle",
            ));
        }
        Ok(UNUserNotificationCenter::currentNotificationCenter())
    }

    fn action_options(action: BannerAction) -> UNNotificationActionOptions {
        let mut options = UNNotificationActionOptions::empty();
        if action.needs_unlock() {
            options |= UNNotificationActionOptions::AuthenticationRequired;
        }
        if action.opens_app() {
            options |= UNNotificationActionOptions::Foreground;
        }
        if action == BannerAction::Deny {
            options |= UNNotificationActionOptions::Destructive;
        }
        options
    }

    impl Notifier for UserNotificationCenter {
        fn upsert(&self, id: &str, notification: &Notification) -> Result<()> {
            let center = center()?;
            let content = UNMutableNotificationContent::new();
            content.setTitle(&NSString::from_str(&notification.title));
            content.setBody(&NSString::from_str(&notification.body));
            if let Some(category) = notification.category {
                content.setCategoryIdentifier(&NSString::from_str(category.id()));
            }
            if let Some(thread) = &notification.thread {
                content.setThreadIdentifier(&NSString::from_str(thread));
            }
            if let Some(sound) = &notification.sound {
                let name = NSString::from_str(sound);
                content.setSound(Some(&UNNotificationSound::soundNamed(&name)));
            }
            let request = UNNotificationRequest::requestWithIdentifier_content_trigger(
                &NSString::from_str(id),
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

        fn remove(&self, id: &str) -> Result<()> {
            let center = center()?;
            let ids = NSArray::from_retained_slice(&[NSString::from_str(id)]);
            center.removeDeliveredNotificationsWithIdentifiers(&ids);
            center.removePendingNotificationRequestsWithIdentifiers(&ids);
            Ok(())
        }

        fn request_authorization(&self) -> Result<()> {
            let center = center()?;
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
                Ok(Ok(true)) => Ok(()),
                Ok(Ok(false)) => Err(Error::System("notifications are not allowed".into())),
                Ok(Err(why)) => Err(Error::System(why)),
                Err(_) => Err(Error::System("macOS did not answer in time".into())),
            }
        }

        fn register_categories(&self, title_of: &dyn Fn(BannerAction) -> String) -> Result<()> {
            let center = center()?;
            let mut categories = Vec::new();
            for category in Category::ALL {
                let actions: Vec<Retained<UNNotificationAction>> = category
                    .actions()
                    .iter()
                    .map(|action| {
                        UNNotificationAction::actionWithIdentifier_title_options(
                            &NSString::from_str(action.id()),
                            &NSString::from_str(&title_of(*action)),
                            action_options(*action),
                        )
                    })
                    .collect();
                let actions = NSArray::from_retained_slice(&actions);
                let intents: Retained<NSArray<NSString>> = NSArray::new();
                categories.push(
                    UNNotificationCategory::categoryWithIdentifier_actions_intentIdentifiers_options(
                        &NSString::from_str(category.id()),
                        &actions,
                        &intents,
                        UNNotificationCategoryOptions::empty(),
                    ),
                );
            }
            center.setNotificationCategories(&NSSet::from_retained_slice(&categories));
            Ok(())
        }

        fn authorization(&self) -> Authorization {
            let Ok(center) = center() else {
                return Authorization::Denied;
            };
            let (tx, rx) = mpsc::channel::<bool>();
            let read = RcBlock::new(move |settings: NonNull<UNNotificationSettings>| {
                // SAFETY: macOS passes a live settings object for the duration of the callback.
                let status = unsafe { settings.as_ref() }.authorizationStatus();
                let _ = tx.send(status != UNAuthorizationStatus::Denied);
            });
            center.getNotificationSettingsWithCompletionHandler(&read);
            match rx.recv_timeout(ANSWER_WAIT) {
                Ok(false) => Authorization::Denied,
                _ => Authorization::Allowed,
            }
        }

        fn delivered(&self) -> Vec<String> {
            let Ok(center) = center() else {
                return Vec::new();
            };
            let (tx, rx) = mpsc::channel::<Vec<String>>();
            let read = RcBlock::new(move |list: NonNull<NSArray<UNNotification>>| {
                // SAFETY: macOS passes a live array for the duration of the callback.
                let list = unsafe { list.as_ref() };
                let ids = (0..list.count())
                    .map(|i| list.objectAtIndex(i).request().identifier().to_string())
                    .collect();
                let _ = tx.send(ids);
            });
            center.getDeliveredNotificationsWithCompletionHandler(&read);
            rx.recv_timeout(ANSWER_WAIT).unwrap_or_default()
        }
    }

    struct DelegateIvars {
        handler: ResponseHandler,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "NotchlingNotificationDelegate"]
        #[ivars = DelegateIvars]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl UNUserNotificationCenterDelegate for Delegate {
            #[unsafe(method(userNotificationCenter:didReceiveNotificationResponse:withCompletionHandler:))]
            fn did_receive(
                &self,
                _center: &UNUserNotificationCenter,
                response: &UNNotificationResponse,
                completion: &block2::DynBlock<dyn Fn()>,
            ) {
                let id = response.notification().request().identifier().to_string();
                let action_id = response.actionIdentifier().to_string();
                let action = if action_id == DEFAULT_ACTION {
                    None
                } else {
                    BannerAction::from_id(&action_id)
                };
                (self.ivars().handler)(Response { id, action });
                completion.call(());
            }

            #[unsafe(method(userNotificationCenter:willPresentNotification:withCompletionHandler:))]
            fn will_present(
                &self,
                _center: &UNUserNotificationCenter,
                _notification: &UNNotification,
                completion: &block2::DynBlock<dyn Fn(UNNotificationPresentationOptions)>,
            ) {
                completion.call((UNNotificationPresentationOptions::Banner
                    | UNNotificationPresentationOptions::List
                    | UNNotificationPresentationOptions::Sound,));
            }
        }
    );

    pub struct DelegateGuard {
        _delegate: Retained<Delegate>,
    }

    pub fn install_delegate(handler: ResponseHandler) -> Option<DelegateGuard> {
        MainThreadMarker::new()?;
        if !has_bundle() {
            return None;
        }
        let this = Delegate::alloc().set_ivars(DelegateIvars { handler });
        let delegate: Retained<Delegate> = unsafe { msg_send![super(this), init] };
        let center = UNUserNotificationCenter::currentNotificationCenter();
        center.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        Some(DelegateGuard {
            _delegate: delegate,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn banner(category: Category) -> Notification {
        Notification {
            category: Some(category),
            ..Notification::new("t", "b")
        }
    }

    #[test]
    fn recording_notifier_keeps_order() {
        let n = RecordingNotifier::new();
        n.upsert("a", &Notification::new("a", "1")).unwrap();
        n.upsert("b", &Notification::new("b", "2")).unwrap();
        let sent = n.sent();
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[0].title, "a");
        assert_eq!(sent[1].body, "2");
        assert_eq!(n.delivered(), ["a", "b"]);
    }

    #[test]
    fn an_upsert_with_the_same_id_is_one_delivered_banner() {
        let n = RecordingNotifier::new();
        n.upsert("a", &Notification::new("a", "1")).unwrap();
        n.upsert("a", &Notification::new("a", "2")).unwrap();
        assert_eq!(n.delivered(), ["a"]);
        assert_eq!(n.upserts().len(), 2);
        n.remove("a").unwrap();
        assert!(n.delivered().is_empty());
        assert_eq!(n.removed(), ["a"]);
    }

    #[test]
    fn simulated_actions_only_exist_when_the_category_offers_them() {
        let n = RecordingNotifier::new();
        n.upsert("m", &banner(Category::InboxOpen)).unwrap();
        assert!(n.simulate_action("m", BannerAction::Allow).is_none());
        assert_eq!(
            n.simulate_action("m", BannerAction::Deny),
            Some(Response {
                id: "m".to_owned(),
                action: Some(BannerAction::Deny)
            })
        );
        assert!(n.simulate_action("missing", BannerAction::Deny).is_none());
    }

    #[test]
    fn categories_carry_the_documented_actions() {
        assert_eq!(
            Category::InboxAllow.actions(),
            [
                BannerAction::Allow,
                BannerAction::Deny,
                BannerAction::Change
            ]
        );
        assert_eq!(
            Category::InboxOpenOnly.actions(),
            [BannerAction::Deny, BannerAction::Open]
        );
        for category in Category::ALL {
            for action in category.actions() {
                assert_eq!(BannerAction::from_id(action.id()), Some(*action));
            }
        }
        assert!(Category::ALL
            .iter()
            .filter(|c| c.actions().contains(&BannerAction::Allow))
            .all(|c| *c == Category::InboxAllow));
    }

    #[test]
    fn a_denied_notifier_refuses_banners_and_says_so() {
        let n = RecordingNotifier::new();
        n.deny();
        assert_eq!(n.authorization(), Authorization::Denied);
        assert!(n.upsert("a", &Notification::new("t", "b")).is_err());
    }

    #[test]
    fn works_through_a_trait_object() {
        let n: Box<dyn Notifier> = Box::new(RecordingNotifier::new());
        assert!(n.upsert("a", &Notification::new("t", "b")).is_ok());
    }

    #[test]
    fn unsupported_notifier_says_so() {
        let err = Unsupported
            .upsert("a", &Notification::new("t", "b"))
            .unwrap_err();
        assert!(matches!(err, Error::Unsupported(_)));
    }
}
