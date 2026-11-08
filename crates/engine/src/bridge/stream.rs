//! The device-window stream: a simulator or emulator window as JPEG frames for one phone (Swift
//! `PhoneStreamer`, `DeviceWindowInfo` and `DeviceWindowPicker`).
//!
//! What is pure lives here: which windows count as devices, which one is picked, the size and rate
//! a stream is asked for and the messages. The frames then go through the connection's
//! [`FrameGate`](super::connection::FrameGate), which drops a frame while the last is still leaving.
//! Capture is a [`ScreenCapture`]: on macOS the Swift app uses ScreenCaptureKit, which needs
//! bindings this workspace does not have, so every platform answers [`UnsupportedStreamer`] until
//! one is written. [`WindowStreamer`] is the whole orchestration over any [`ScreenCapture`].

use super::connection::{lock, Connection};
use super::model::PhoneOptions;
use super::wire::BridgeFailure;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

/// One window a capture could list.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceWindowInfo {
    pub id: u32,
    pub app_bundle_id: String,
    pub app_name: String,
    pub title: String,
    pub is_on_screen: bool,
    pub is_app_active: bool,
    pub width: f64,
    pub height: f64,
}

impl DeviceWindowInfo {
    /// iOS devices in Xcode's DeviceHub (`com.apple.dt.Devices`) or the old Simulator, and Android
    /// emulators. A sliver (a menu bar, a toolbar) is not a device.
    pub fn is_device_window(bundle_id: &str, app_name: &str, width: f64, height: f64) -> bool {
        if width < 150.0 || height < 300.0 {
            return false;
        }
        if bundle_id == "com.apple.dt.Devices" || bundle_id == "com.apple.iphonesimulator" {
            return true;
        }
        let name = app_name.to_lowercase();
        name.contains("qemu-system") || name.contains("android emulator")
    }
}

/// The window whose title matches a device named in the transcript, else one that is on screen in
/// the active app, else any on-screen one, else the first.
pub fn pick<'a>(windows: &'a [DeviceWindowInfo], hints: &[String]) -> Option<&'a DeviceWindowInfo> {
    // An empty hint would match every title.
    let lowered: Vec<String> = hints
        .iter()
        .filter(|h| !h.is_empty())
        .map(|h| h.to_lowercase())
        .collect();
    let by_title = windows.iter().find(|w| {
        let title = w.title.to_lowercase();
        !title.is_empty()
            && lowered
                .iter()
                .any(|h| title.contains(h) || h.contains(&title))
    });
    by_title
        .or_else(|| windows.iter().find(|w| w.is_on_screen && w.is_app_active))
        .or_else(|| windows.iter().find(|w| w.is_on_screen))
        .or_else(|| windows.first())
}

/// What the capture is asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamPlan {
    pub window_id: u32,
    /// Pixels; at least 2 each way.
    pub width: u32,
    pub height: u32,
    /// Frames per second at most (1 to 15 after the router's clamp).
    pub fps: u32,
}

impl StreamPlan {
    /// The window scaled so its width is the requested one, keeping its shape.
    pub fn new(window: &DeviceWindowInfo, options: &PhoneOptions) -> StreamPlan {
        let scale = options.width as f64 / window.width.max(1.0);
        StreamPlan {
            window_id: window.id,
            width: ((window.width * scale) as u32).max(2),
            height: ((window.height * scale) as u32).max(2),
            fps: options.fps.max(1) as u32,
        }
    }

    /// The least time between two frames.
    pub fn frame_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(1.0 / f64::from(self.fps.max(1)))
    }
}

/// The window to stream: the one the phone named if it is a device window, else the picker's.
pub fn choose_window(
    windows: &[DeviceWindowInfo],
    requested: Option<u32>,
) -> Result<&DeviceWindowInfo, BridgeFailure> {
    let chosen = requested
        .and_then(|id| windows.iter().find(|w| w.id == id))
        .or_else(|| pick(windows, &[]))
        .ok_or_else(|| {
            BridgeFailure::not_found(
                "Không thấy cửa sổ máy ảo (DeviceHub, Simulator hoặc Android Emulator).",
            )
        })?;
    if !chosen.is_on_screen {
        return Err(BridgeFailure::not_found(
            "Cửa sổ máy ảo đang ẩn — mở DeviceHub để xem.",
        ));
    }
    Ok(chosen)
}

/// Starts and stops one phone's stream; the backend's `start_phone` and `stop_phone`.
pub trait Streamer: Send + Sync {
    fn start(
        &self,
        connection: &Arc<Connection>,
        options: &PhoneOptions,
    ) -> Result<(), BridgeFailure>;

    fn stop(&self, connection: &Arc<Connection>);
}

/// No capture on this computer.
pub struct UnsupportedStreamer;

impl Streamer for UnsupportedStreamer {
    fn start(&self, _: &Arc<Connection>, _: &PhoneOptions) -> Result<(), BridgeFailure> {
        Err(BridgeFailure::invalid("Máy này chưa phát được màn hình."))
    }

    fn stop(&self, _: &Arc<Connection>) {}
}

/// A running capture; stopping it ends the frames.
pub trait StreamHandle: Send {
    fn stop(self: Box<Self>);
}

/// Receives each JPEG frame; may be called from the capture's thread.
pub type FrameSink = Box<dyn Fn(&[u8]) + Send + Sync>;

/// Lists device windows and captures one (ScreenCaptureKit on macOS).
pub trait ScreenCapture: Send + Sync {
    /// Whether the app may record the screen. Without it nothing can be listed or captured.
    fn permitted(&self) -> bool;
    /// The windows that [`DeviceWindowInfo::is_device_window`] accepts.
    fn device_windows(&self) -> Result<Vec<DeviceWindowInfo>, String>;
    /// Frames of the window as JPEG (quality 0.6, no cursor) at most `plan.fps` a second.
    fn start(&self, plan: &StreamPlan, sink: FrameSink) -> Result<Box<dyn StreamHandle>, String>;
}

/// The stream orchestration over a [`ScreenCapture`]: one stream per connection, frames offered to
/// the connection's gate.
pub struct WindowStreamer<C: ScreenCapture> {
    capture: C,
    streams: Mutex<HashMap<u64, Box<dyn StreamHandle>>>,
}

impl<C: ScreenCapture> WindowStreamer<C> {
    pub fn new(capture: C) -> WindowStreamer<C> {
        WindowStreamer {
            capture,
            streams: Mutex::new(HashMap::new()),
        }
    }

    /// Streams running now.
    pub fn active(&self) -> usize {
        lock(&self.streams).len()
    }
}

impl<C: ScreenCapture> Streamer for WindowStreamer<C> {
    fn start(
        &self,
        connection: &Arc<Connection>,
        options: &PhoneOptions,
    ) -> Result<(), BridgeFailure> {
        if !self.capture.permitted() {
            return Err(BridgeFailure::internal(
                "Notchling chưa có quyền Ghi màn hình (System Settings › Quyền riêng tư & Bảo mật › Ghi màn hình).",
            ));
        }
        // A new request replaces the connection's old stream.
        self.stop(connection);
        let windows = self
            .capture
            .device_windows()
            .map_err(|why| BridgeFailure::internal(format!("Không liệt kê được cửa sổ: {why}")))?;
        let window = choose_window(&windows, options.device_window_id)?;
        let plan = StreamPlan::new(window, options);
        let target = Arc::clone(connection);
        let handle = self
            .capture
            .start(
                &plan,
                Box::new(move |jpeg| {
                    // Encoding costs more than dropping: the gate refuses while the last frame is out.
                    target.offer_phone_frame(jpeg);
                }),
            )
            .map_err(|why| BridgeFailure::internal(format!("Không quay được cửa sổ: {why}")))?;
        lock(&self.streams).insert(connection.id, handle);
        Ok(())
    }

    fn stop(&self, connection: &Arc<Connection>) {
        let handle = lock(&self.streams).remove(&connection.id);
        if let Some(handle) = handle {
            handle.stop();
        }
    }
}
