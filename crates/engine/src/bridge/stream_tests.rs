//! The device-window stream: port of the picker test in `VizEngineTests`, plus the rate and
//! framing logic and the orchestration over a fake capture.

use super::connection::{lock, Connection};
use super::hub::Hub;
use super::model::PhoneOptions;
use super::stream::*;
use super::test_support::RecordingIo;
use notchling_protocol::generated::ErrorCode;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

fn window(id: u32, title: &str, on_screen: bool, active: bool) -> DeviceWindowInfo {
    DeviceWindowInfo {
        id,
        app_bundle_id: "com.apple.dt.Devices".into(),
        app_name: "DeviceHub".into(),
        title: title.into(),
        is_on_screen: on_screen,
        is_app_active: active,
        width: 400.0,
        height: 800.0,
    }
}

fn options(window_id: Option<u32>) -> PhoneOptions {
    PhoneOptions {
        device_window_id: window_id,
        fps: 10,
        width: 200,
    }
}

#[test]
fn only_device_windows_of_a_big_enough_size_count() {
    assert!(DeviceWindowInfo::is_device_window(
        "com.apple.dt.Devices",
        "DeviceHub",
        395.0,
        860.0
    ));
    assert!(DeviceWindowInfo::is_device_window(
        "com.apple.iphonesimulator",
        "Simulator",
        400.0,
        800.0
    ));
    assert!(DeviceWindowInfo::is_device_window(
        "x",
        "qemu-system-aarch64",
        400.0,
        800.0
    ));
    assert!(DeviceWindowInfo::is_device_window(
        "x",
        "Android Emulator - Pixel_8:5554",
        400.0,
        800.0
    ));
    assert!(
        !DeviceWindowInfo::is_device_window("com.apple.dt.Devices", "DeviceHub", 1512.0, 33.0),
        "menu bar sliver"
    );
    assert!(!DeviceWindowInfo::is_device_window(
        "com.apple.Safari",
        "Safari",
        800.0,
        600.0
    ));
    assert!(!DeviceWindowInfo::is_device_window(
        "com.apple.iphonesimulator",
        "Simulator",
        149.0,
        800.0
    ));
}

#[test]
fn the_picker_prefers_a_hint_then_the_active_app_then_any_visible_window() {
    let windows = [
        window(1, "iPhone 16 Pro", false, false),
        window(2, "WorkFlow Links", true, true),
        window(3, "iPad Air", true, false),
    ];
    let id = |hints: &[&str]| {
        let hints: Vec<String> = hints.iter().map(|h| h.to_string()).collect();
        pick(&windows, &hints).map(|w| w.id)
    };
    assert_eq!(
        id(&["iPad Air"]),
        Some(3),
        "title matches a hint from the transcript"
    );
    assert_eq!(
        id(&["ipad"]),
        Some(3),
        "case does not matter, a hint may be part of the title"
    );
    assert_eq!(
        id(&["the iphone 16 pro simulator"]),
        Some(1),
        "or the title part of the hint"
    );
    assert_eq!(
        id(&[]),
        Some(2),
        "otherwise the active app's visible window"
    );
    assert_eq!(id(&[""]), Some(2), "an empty hint matches nothing");
    assert_eq!(
        pick(&windows[..1], &[]).map(|w| w.id),
        Some(1),
        "any window beats none"
    );
    assert_eq!(
        pick(
            &[window(4, "a", false, false), window(5, "b", true, false)],
            &[]
        )
        .map(|w| w.id),
        Some(5),
        "a visible window beats a hidden one"
    );
    assert!(pick(&[], &["iPhone".to_string()]).is_none());
}

#[test]
fn a_window_with_no_title_matches_no_hint() {
    let windows = [window(1, "", true, false), window(2, "Pixel", true, true)];
    assert_eq!(
        pick(&windows, &["anything".to_string()]).map(|w| w.id),
        Some(2)
    );
}

#[test]
fn the_plan_keeps_the_windows_shape_at_the_requested_width() {
    let plan = StreamPlan::new(&window(7, "t", true, true), &options(None));
    assert_eq!(plan.window_id, 7);
    assert_eq!((plan.width, plan.height), (200, 400));
    assert_eq!(plan.fps, 10);
    assert_eq!(plan.frame_interval(), Duration::from_millis(100));
}

#[test]
fn the_plan_never_asks_for_less_than_two_pixels_or_one_frame_a_second() {
    let mut thin = window(1, "t", true, true);
    thin.width = 1000.0;
    thin.height = 1.0;
    let plan = StreamPlan::new(
        &thin,
        &PhoneOptions {
            device_window_id: None,
            fps: 0,
            width: 100,
        },
    );
    assert_eq!(plan.height, 2);
    assert_eq!(plan.fps, 1);
    assert_eq!(plan.frame_interval(), Duration::from_secs(1));
}

#[test]
fn a_zero_width_window_does_not_divide_by_zero() {
    let mut flat = window(1, "t", true, true);
    flat.width = 0.0;
    let plan = StreamPlan::new(&flat, &options(None));
    assert!(plan.width >= 2 && plan.height >= 2);
}

#[test]
fn the_named_window_is_used_when_it_is_a_device_window() {
    let windows = [window(1, "a", true, true), window(2, "b", true, false)];
    assert_eq!(choose_window(&windows, Some(2)).expect("window").id, 2);
    assert_eq!(
        choose_window(&windows, Some(99)).expect("falls back").id,
        1,
        "an id that is not a device window gives the picker's choice"
    );
    assert_eq!(choose_window(&windows, None).expect("window").id, 1);
}

#[test]
fn no_windows_and_hidden_windows_are_not_found() {
    let none = choose_window(&[], None).expect_err("none");
    assert_eq!(none.code, ErrorCode::NotFound);
    let hidden = choose_window(&[window(1, "a", false, false)], None).expect_err("hidden");
    assert_eq!(hidden.code, ErrorCode::NotFound);
    assert!(hidden.message.contains("ẩn"));
}

// MARK: Orchestration over a fake capture

#[derive(Default)]
struct CaptureState {
    sinks: Mutex<Vec<FrameSink>>,
    plans: Mutex<Vec<StreamPlan>>,
    stopped: AtomicUsize,
}

struct FakeCapture {
    permitted: bool,
    windows: Result<Vec<DeviceWindowInfo>, String>,
    start_error: Option<String>,
    state: Arc<CaptureState>,
    listed: AtomicBool,
}

struct FakeHandle(Arc<CaptureState>);

impl StreamHandle for FakeHandle {
    fn stop(self: Box<Self>) {
        self.0.stopped.fetch_add(1, Ordering::SeqCst);
    }
}

impl ScreenCapture for FakeCapture {
    fn permitted(&self) -> bool {
        self.permitted
    }
    fn device_windows(&self) -> Result<Vec<DeviceWindowInfo>, String> {
        self.listed.store(true, Ordering::SeqCst);
        self.windows.clone()
    }
    fn start(&self, plan: &StreamPlan, sink: FrameSink) -> Result<Box<dyn StreamHandle>, String> {
        if let Some(why) = &self.start_error {
            return Err(why.clone());
        }
        lock(&self.state.plans).push(plan.clone());
        lock(&self.state.sinks).push(sink);
        Ok(Box::new(FakeHandle(self.state.clone())))
    }
}

fn fake(permitted: bool, windows: Vec<DeviceWindowInfo>) -> (FakeCapture, Arc<CaptureState>) {
    let state = Arc::new(CaptureState::default());
    (
        FakeCapture {
            permitted,
            windows: Ok(windows),
            start_error: None,
            state: state.clone(),
            listed: AtomicBool::new(false),
        },
        state,
    )
}

fn connect() -> (Arc<Connection>, Arc<RecordingIo>) {
    let io = Arc::new(RecordingIo::default());
    // One hub, so every connection has its own id.
    static HUB: OnceLock<Hub> = OnceLock::new();
    let connection = HUB
        .get_or_init(Hub::new)
        .connect("192.168.1.20", false, io.clone());
    (connection, io)
}

#[test]
fn a_stream_sends_the_captures_frames_through_the_connections_gate() {
    let (capture, state) = fake(true, vec![window(1, "iPhone", true, true)]);
    let streamer = WindowStreamer::new(capture);
    let (connection, io) = connect();
    streamer
        .start(&connection, &options(None))
        .expect("started");
    assert_eq!(streamer.active(), 1);
    assert_eq!(lock(&state.plans)[0].width, 200);

    let frame = [0xFF, 0xD8, 0xFF, 0xD9];
    io.hold_binary.store(true, Ordering::SeqCst);
    {
        let sinks = lock(&state.sinks);
        sinks[0](&frame);
        sinks[0](&frame);
    }
    assert_eq!(
        io.binaries().len(),
        1,
        "the second frame was dropped while the first was leaving"
    );
    assert_eq!(&io.binaries()[0][..4], b"PHN1");
    io.release_binary();
    lock(&state.sinks)[0](&frame);
    assert_eq!(io.binaries().len(), 2);
}

#[test]
fn without_the_screen_recording_permission_nothing_is_listed() {
    let (capture, state) = fake(false, vec![window(1, "iPhone", true, true)]);
    let streamer = WindowStreamer::new(capture);
    let (connection, _) = connect();
    let failure = streamer
        .start(&connection, &options(None))
        .expect_err("refused");
    assert_eq!(failure.code, ErrorCode::Internal);
    assert!(failure.message.contains("Ghi màn hình"));
    assert!(lock(&state.plans).is_empty());
    assert_eq!(streamer.active(), 0);
}

#[test]
fn a_second_start_replaces_the_first_stream() {
    let (capture, state) = fake(true, vec![window(1, "iPhone", true, true)]);
    let streamer = WindowStreamer::new(capture);
    let (connection, _) = connect();
    streamer.start(&connection, &options(None)).expect("first");
    streamer.start(&connection, &options(None)).expect("second");
    assert_eq!(state.stopped.load(Ordering::SeqCst), 1);
    assert_eq!(streamer.active(), 1);
}

#[test]
fn stop_ends_the_stream_and_a_second_stop_does_nothing() {
    let (capture, state) = fake(true, vec![window(1, "iPhone", true, true)]);
    let streamer = WindowStreamer::new(capture);
    let (connection, _) = connect();
    streamer
        .start(&connection, &options(None))
        .expect("started");
    streamer.stop(&connection);
    streamer.stop(&connection);
    assert_eq!(state.stopped.load(Ordering::SeqCst), 1);
    assert_eq!(streamer.active(), 0);
}

#[test]
fn each_connection_has_its_own_stream() {
    let (capture, state) = fake(true, vec![window(1, "iPhone", true, true)]);
    let streamer = WindowStreamer::new(capture);
    let (one, _) = connect();
    let (two, _) = connect();
    streamer.start(&one, &options(None)).expect("one");
    streamer.start(&two, &options(None)).expect("two");
    assert_eq!(streamer.active(), 2);
    streamer.stop(&one);
    assert_eq!(streamer.active(), 1);
    assert_eq!(state.stopped.load(Ordering::SeqCst), 1);
}

#[test]
fn a_capture_that_cannot_list_or_start_is_an_internal_error_with_its_reason() {
    let (mut capture, _) = fake(true, vec![]);
    capture.windows = Err("denied".into());
    let (connection, _) = connect();
    let failure = WindowStreamer::new(capture)
        .start(&connection, &options(None))
        .expect_err("list");
    assert_eq!(failure.code, ErrorCode::Internal);
    assert!(failure.message.contains("denied"));

    let (mut capture, _) = fake(true, vec![window(1, "iPhone", true, true)]);
    capture.start_error = Some("no display".into());
    let streamer = WindowStreamer::new(capture);
    let failure = streamer
        .start(&connection, &options(None))
        .expect_err("start");
    assert_eq!(failure.code, ErrorCode::Internal);
    assert!(failure.message.contains("no display"));
    assert_eq!(streamer.active(), 0);
}

#[test]
fn no_device_window_is_not_found() {
    let (capture, _) = fake(true, vec![]);
    let (connection, _) = connect();
    let failure = WindowStreamer::new(capture)
        .start(&connection, &options(None))
        .expect_err("none");
    assert_eq!(failure.code, ErrorCode::NotFound);
}

#[test]
fn the_unsupported_streamer_says_so_and_stop_is_harmless() {
    let (connection, _) = connect();
    let failure = UnsupportedStreamer
        .start(&connection, &options(None))
        .expect_err("unsupported");
    assert_eq!(failure.code, ErrorCode::Invalid);
    UnsupportedStreamer.stop(&connection);
}
