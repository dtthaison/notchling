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

const PICTURE_LOOKUP_MESSAGES: usize = chat::DEFAULT_LIMIT * 2;

pub struct Extras {
    pub viz: VizEngine,
    pub snapshots: WebSnapshots,
    pub streamer: Box<dyn Streamer>,
    pub imaging: Box<dyn Imaging>,
    pub home: PathBuf,
}

impl Extras {
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
    pub(super) fn picture(
        &self,
        session_id: &str,
        image_id: &str,
        max_width: i64,
    ) -> Result<Vec<u8>, BridgeFailure> {
        if foreign::parse(session_id).is_some() {
            return Err(BridgeFailure::not_found("Image not found."));
        }
        let session = self.find(session_id)?;
        let path = session
            .transcript_path
            .ok_or_else(|| BridgeFailure::not_found("Transcript not found yet."))?;
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
                ImagingError::Unreadable => BridgeFailure::not_found("Can't read the image."),
                ImagingError::Unsupported(why) => BridgeFailure::invalid(why),
            })
    }

    pub(super) fn viz_bundle(
        &self,
        session_id: &str,
        include_entity: bool,
    ) -> Result<BridgeViz, BridgeFailure> {
        if foreign::parse(session_id).is_some() {
            return Err(BridgeFailure::invalid(
                "Codex and Antigravity have no Images tab yet.",
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
        ImageError::NotAllowed => BridgeFailure::invalid("This image is outside the allowed folders."),
        ImageError::NotFound => BridgeFailure::not_found("Image not found in the transcript."),
        ImageError::TooLarge => BridgeFailure::invalid("This image is too large (over 20 MB)."),
    }
}
