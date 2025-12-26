//! Solar system activation module
//!
//! Demonstrates nested plugin hierarchy via the coalgebraic structure.

mod activation;
mod celestial;
mod types;

pub use activation::Solar;
pub use types::{BodyType, SolarEvent};
