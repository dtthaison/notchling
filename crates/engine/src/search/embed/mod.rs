mod fake;
pub mod helper;
pub mod helper_fake;
mod quant;
#[cfg(feature = "static-embed")]
mod static_model;

pub use fake::{HashEmbedder, TableEmbedder};
pub use quant::{dot, dot_f32, dot_f32_i8, quantize, Q8};
#[cfg(feature = "static-embed")]
pub use static_model::StaticEmbedder;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct ModelId(pub String);

pub const PIPELINE: &str = "p1";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EmbedError {
    NotReady,
    Failed(String),
}

pub trait Embedder: Send + Sync {
    fn model(&self) -> &ModelId;
    fn dims(&self) -> usize;
    fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, EmbedError>;
}

#[cfg(test)]
mod embed_tests;
#[cfg(test)]
mod helper_tests;
#[cfg(all(test, feature = "static-embed"))]
mod static_model_tests;
