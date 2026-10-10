//! Deterministic CPU reference engine. All pixel math happens here; the CLI and
//! (later) the desktop app and Photoshop bridge call into this crate.

pub mod buffer;
pub mod color;
pub mod exposure;

pub use buffer::{EngineError, LinearImage};
pub use exposure::apply_exposure;
