//! The phone extras of the live backend (E15): Visualize, chat pictures, web snapshots and the
//! device-window stream, with the services each needs. Everything that touches the screen or a
//! web view is behind a trait whose default is "not supported on this computer".

use super::foreign;
use super::images::{ImageCrate, Imaging, ImagingError};
use super::live::{Acting, LiveBackend, Sources};
use super::model::BridgeViz;
use super::snapshot::{Unsupported, WebSnapshots};
use super::stream::{Streamer, UnsupportedStreamer};
use super::wire::BridgeFailure;
use crate::control::{Launcher, RealLauncher};
use crate::transcript::chat::images::{self, ImageError, ImageOrigin, ImageRef};
use crate::transcript::chat::{self};
use crate::transcript::util::now_secs;
use crate::viz::suggest::suggest;
use crate::viz::{git, VizEngine};
use notchling_protocol::generated as wire;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Messages kept for a picture lookup: what Chat keeps (`defaultLimit * 2`).
const PICTURE_LOOKUP_MESSAGES: usize = chat::DEFAULT_LIMIT * 2;

pub struct Extras {
    pub viz: VizEngine,
    pub snapshots: WebSnapshots,
    pub streamer: Box<dyn Streamer>,
    pub imaging: Box<dyn Imaging>,
    /// Pictures a chat names are shown from here and `/tmp` only.
    pub home: PathBuf,
}

impl Extras {
    /// The real viz engine (git through [`RealLauncher`]) and picture encoder; page and window
    /// capture are [`Unsupported`] and [`UnsupportedStreamer`] until a platform provides them.
    pub fn real(home: PathBuf) -> Extras {
        let launcher: Arc<dyn Launcher> = Arc::new(RealLauncher::new(&home));
        let viz = VizEngine::new(
            Box::new(now_secs),
            Box::new(move |cwd| git::read(cwd, launcher.as_ref())),
        )
        .with_home(home.clone());
        Extras {
            viz,
            snapshots: WebSnapshots::new(Box::new(Unsupported)),
            streamer: Box::new(UnsupportedStreamer),
            imaging: Box::new(ImageCrate),
            home,
        }
    }
}

/// A chat picture as the wire has it: a description, never the bytes.
pub fn wire_image(image: &ImageRef) -> wire::Image {
    wire::Image {
        id: image.id.clone(),
        media_type: image.media_type.clone(),
        width: image.width,
        height: image.height,
        origin: match image.origin {
            ImageOrigin::User => wire::ImageOrigin::User,
            ImageOrigin::ToolResult => wire::ImageOrigin::ToolResult,
            ImageOrigin::File => wire::ImageOrigin::File,
        },
        bytes: image.byte_count,
    }
}

impl<S: Sources, A: Acting> LiveBackend<S, A> {
    /// `chat.image`: the JPEG of a picture this session's chat has shown, at most `max_width` wide.
    /// A phone can only name pictures the chat listed, never an arbitrary file.
    pub(super) fn picture(
        &self,
        session_id: &str,
        image_id: &str,
        max_width: i64,
    ) -> Result<Vec<u8>, BridgeFailure> {
        if foreign::parse(session_id).is_some() {
            return Err(BridgeFailure::not_found("Không thấy ảnh này."));
        }
        let session = self.find(session_id)?;
        let path = session
            .transcript_path
            .ok_or_else(|| BridgeFailure::not_found("Chưa tìm thấy transcript."))?;
        let shown = chat::read_in(&path, true, PICTURE_LOOKUP_MESSAGES, &self.extras.home);
        let picture = shown
            .messages
            .iter()
            .flat_map(|m| &m.images)
            .find(|i| i.id == image_id)
            .ok_or_else(|| not_in_transcript(ImageError::NotFound))?;
        let bytes = images::data(picture, Path::new(&path), &self.extras.home)
            .map_err(not_in_transcript)?;
        self.extras
            .imaging
            .jpeg(&bytes, max_width)
            .map_err(|error| match error {
                ImagingError::Unreadable => BridgeFailure::not_found("Không đọc được ảnh."),
                ImagingError::Unsupported(why) => BridgeFailure::invalid(why),
            })
    }

    /// `viz.get`: the flow, diagram and chart specs (and entity when asked), the tab to open first
    /// and the local web page. Codex and Antigravity sessions have no Visualize tabs yet.
    pub(super) fn viz_bundle(
        &self,
        session_id: &str,
        include_entity: bool,
    ) -> Result<BridgeViz, BridgeFailure> {
        if foreign::parse(session_id).is_some() {
            return Err(BridgeFailure::invalid(
                "Codex và Antigravity chưa có tab hình.",
            ));
        }
        let session = self.find(session_id)?;
        let viz = &self.extras.viz;
        let bundle = viz.build(&session, None);
        let mut specs: BTreeMap<String, wire::VizSpec> = BTreeMap::new();
        specs.insert("flow".into(), bundle.flow);
        specs.insert("diagram".into(), bundle.diagram);
        specs.insert("chart".into(), bundle.chart);
        if include_entity {
            specs.insert("entity".into(), viz.entity_spec(&session.cwd));
        }
        let tab = suggest(&session.cwd, &bundle.signals);
        Ok(BridgeViz {
            suggested_tab: tab.as_str().to_string(),
            web_url: bundle.web_url,
            specs,
            web_candidates: bundle.web_candidates,
        })
    }
}

fn not_in_transcript(error: ImageError) -> BridgeFailure {
    match error {
        ImageError::NotAllowed => BridgeFailure::invalid("Ảnh này nằm ngoài các thư mục cho phép."),
        ImageError::NotFound => BridgeFailure::not_found("Không thấy ảnh này trong transcript."),
    }
}
