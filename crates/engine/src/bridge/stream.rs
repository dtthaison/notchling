use super::connection::{lock, Connection};
use super::model::PhoneOptions;
use super::wire::BridgeFailure;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

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

pub fn pick<'a>(windows: &'a [DeviceWindowInfo], hints: &[String]) -> Option<&'a DeviceWindowInfo> {
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

#[derive(Debug, Clone, PartialEq)]
pub struct StreamPlan {
    pub window_id: u32,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
}

impl StreamPlan {
    pub fn new(window: &DeviceWindowInfo, options: &PhoneOptions) -> StreamPlan {
        let scale = options.width as f64 / window.width.max(1.0);
        StreamPlan {
            window_id: window.id,
            width: ((window.width * scale) as u32).max(2),
            height: ((window.height * scale) as u32).max(2),
            fps: options.fps.max(1) as u32,
        }
    }

    pub fn frame_interval(&self) -> std::time::Duration {
        std::time::Duration::from_secs_f64(1.0 / f64::from(self.fps.max(1)))
    }
}

pub fn choose_window(
    windows: &[DeviceWindowInfo],
    requested: Option<u32>,
) -> Result<&DeviceWindowInfo, BridgeFailure> {
    let chosen = requested
        .and_then(|id| windows.iter().find(|w| w.id == id))
        .or_else(|| pick(windows, &[]))
        .ok_or_else(|| {
            BridgeFailure::not_found(
                "No simulator window found (DeviceHub, Simulator or Android Emulator).",
            )
        })?;
    if !chosen.is_on_screen {
        return Err(BridgeFailure::not_found(
            "The simulator window is hidden. Open DeviceHub to see it.",
        ));
    }
    Ok(chosen)
}

pub trait Streamer: Send + Sync {
    fn start(
        &self,
        connection: &Arc<Connection>,
        options: &PhoneOptions,
    ) -> Result<(), BridgeFailure>;

    fn stop(&self, connection: &Arc<Connection>);
}

pub struct UnsupportedStreamer;

impl Streamer for UnsupportedStreamer {
    fn start(&self, _: &Arc<Connection>, _: &PhoneOptions) -> Result<(), BridgeFailure> {
        Err(BridgeFailure::invalid(
            "The Mac can't stream its screen yet.",
        ))
    }

    fn stop(&self, _: &Arc<Connection>) {}
}

pub trait StreamHandle: Send {
    fn stop(self: Box<Self>);
}

pub type FrameSink = Box<dyn Fn(&[u8]) + Send + Sync>;

pub trait ScreenCapture: Send + Sync {
    fn permitted(&self) -> bool;
    fn device_windows(&self) -> Result<Vec<DeviceWindowInfo>, String>;
    fn start(&self, plan: &StreamPlan, sink: FrameSink) -> Result<Box<dyn StreamHandle>, String>;
}

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
                "Notchling doesn't have Screen Recording permission (System Settings › Privacy & Security › Screen Recording).",
            ));
        }
        self.stop(connection);
        let windows = self
            .capture
            .device_windows()
            .map_err(|why| BridgeFailure::internal(format!("Can't list windows: {why}")))?;
        let window = choose_window(&windows, options.device_window_id)?;
        let plan = StreamPlan::new(window, options);
        let target = Arc::clone(connection);
        let handle = self
            .capture
            .start(
                &plan,
                Box::new(move |jpeg| {
                    target.offer_phone_frame(jpeg);
                }),
            )
            .map_err(|why| BridgeFailure::internal(format!("Can't capture the window: {why}")))?;
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
