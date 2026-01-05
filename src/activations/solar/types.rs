//! Solar system event types
//!
//! Domain types for the solar system activation.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Events from solar system observations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SolarEvent {
    /// Information about a celestial body
    Body {
        name: String,
        body_type: BodyType,
        mass_kg: f64,
        radius_km: f64,
        orbital_period_days: Option<f64>,
        parent: Option<String>,
    },
    /// System overview
    System {
        star: String,
        planet_count: usize,
        moon_count: usize,
        total_bodies: usize,
    },
}

/// Type of celestial body
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BodyType {
    Star,
    Planet,
    DwarfPlanet,
    Moon,
}
