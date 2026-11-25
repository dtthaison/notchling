use super::*;
use std::io;

const BUSY: &str = "Notchling is busy. Try again.";

type Answer = Box<dyn Fn(&str) -> Result<Value, ClientError> + Send + Sync>;

struct Fake(Answer);

impl Transport for Fake {
    fn call(&self, op: &str, _args: Value) -> Result<Value, ClientError> {
        (self.0)(op)
    }
}

fn over(answer: impl Fn(&str) -> Result<Value, ClientError> + Send + Sync + 'static) -> Actions {
    Actions::over(Box::new(Fake(Box::new(answer))))
}

fn slow_ones() -> Vec<ClientError> {
    vec![
        ClientError::Timeout,
        ClientError::Io(io::Error::from(io::ErrorKind::WouldBlock)),
        ClientError::Io(io::Error::from(io::ErrorKind::TimedOut)),
        ClientError::Io(io::Error::from(io::ErrorKind::Interrupted)),
    ]
}

fn gone_ones() -> Vec<ClientError> {
    vec![
        ClientError::NotRunning(io::Error::from(io::ErrorKind::NotFound)),
        ClientError::NotRunning(io::Error::from(io::ErrorKind::ConnectionRefused)),
        ClientError::Closed,
        ClientError::Unsupported,
        ClientError::UntrustedServer("another user".into()),
        ClientError::Io(io::Error::from(io::ErrorKind::BrokenPipe)),
        ClientError::Io(io::Error::from(io::ErrorKind::ConnectionReset)),
        ClientError::Io(io::Error::from(io::ErrorKind::UnexpectedEof)),
    ]
}

fn once(error: ClientError) -> impl Fn(&str) -> Result<Value, ClientError> + Send + Sync {
    let error = Mutex::new(Some(error));
    move |op| match op {
        "daemon.status" => Ok(json!({"acting": true})),
        _ => match error.lock().unwrap().take() {
            Some(e) => Err(e),
            None => Ok(json!({"ok": true})),
        },
    }
}

#[test]
fn a_send_that_timed_out_says_busy_and_acting_stays_on() {
    for error in slow_ones() {
        let label = error.to_string();
        let actions = over(once(error));
        assert_eq!(actions.refresh(), Acting::On);
        let failure = actions.send_full(&Request::send("s1", "hi")).unwrap_err();
        assert_eq!(actions.acting(true), Acting::On, "{label}");
        assert_eq!(failure.message, BUSY, "{label}");
        assert_eq!(failure.data, None);
        assert!(actions.send(&Request::send("s1", "hi")).is_ok(), "{label}");
        assert_eq!(actions.acting(true), Acting::On, "{label}");
    }
}

#[test]
fn a_read_that_timed_out_says_busy_and_changes_nothing() {
    for error in slow_ones() {
        let label = error.to_string();
        let actions = over(once(error));
        assert_eq!(actions.refresh(), Acting::On);
        let read = Request::question("s1");
        assert_eq!(actions.read(&read).unwrap_err(), BUSY, "{label}");
        assert_eq!(actions.acting(true), Acting::On, "{label}");
        assert!(actions.read(&read).is_ok(), "{label}");
    }
}

#[test]
fn a_status_check_that_timed_out_keeps_what_was_said() {
    for error in slow_ones() {
        let label = error.to_string();
        let slow = Mutex::new(Some(error));
        let first = std::sync::atomic::AtomicBool::new(true);
        let actions = over(move |_| {
            if first.swap(false, std::sync::atomic::Ordering::SeqCst) {
                return Ok(json!({"acting": true}));
            }
            match slow.lock().unwrap().take() {
                Some(e) => Err(e),
                None => Ok(json!({"acting": true})),
            }
        });
        assert_eq!(actions.refresh(), Acting::On);
        assert_eq!(actions.refresh(), Acting::On, "{label}");
        assert_eq!(actions.acting(true), Acting::On, "{label}");
    }
}

#[test]
fn a_switch_that_timed_out_says_busy_and_acting_stays_on() {
    let actions = over(once(ClientError::Timeout));
    assert_eq!(actions.refresh(), Acting::On);
    assert_eq!(actions.choose(false).unwrap_err(), BUSY);
    assert_eq!(actions.acting(true), Acting::On);
}

#[test]
fn a_daemon_that_is_gone_still_detaches_on_every_path() {
    for error in gone_ones() {
        let label = error.to_string();
        let sent = over(once(error));
        assert_eq!(sent.refresh(), Acting::On);
        assert!(sent.send(&Request::send("s1", "hi")).is_err(), "{label}");
        assert_eq!(sent.acting(true), Acting::Detached, "{label}");
    }
    for error in gone_ones() {
        let label = error.to_string();
        let read = over(once(error));
        assert_eq!(read.refresh(), Acting::On);
        assert!(read.read(&Request::question("s1")).is_err(), "{label}");
        assert_eq!(read.acting(true), Acting::Detached, "{label}");
    }
    for error in gone_ones() {
        let label = error.to_string();
        let status = Mutex::new(Some(error));
        let checked = over(move |_| match status.lock().unwrap().take() {
            Some(e) => Err(e),
            None => Ok(json!({"acting": true})),
        });
        assert_eq!(checked.refresh(), Acting::Detached, "{label}");
    }
}

#[test]
fn busy_is_not_a_lost_daemon_and_a_lost_one_is_not_busy() {
    for error in slow_ones() {
        assert_eq!(acting::after_error(&error), None, "{error}");
    }
    for error in gone_ones() {
        assert_eq!(
            acting::after_error(&error),
            Some(Acting::Detached),
            "{error}"
        );
    }
}
