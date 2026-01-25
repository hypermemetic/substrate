//! Solar system activation - demonstrates nested plugin hierarchy
//!
//! This activation shows the coalgebraic plugin structure where plugins
//! can have children. The solar system is a natural hierarchy:
//! - Sol (star) contains planets
//! - Planets contain moons
//!
//! Each level implements the F-coalgebra structure map via `plugin_schema()`.

use super::celestial::{build_solar_system, CelestialBody, CelestialBodyActivation};
use super::types::{BodyType, SolarEvent};
use crate::plexus::{Activation, ChildRouter, ChildSummary};
use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;

/// Solar system activation - demonstrates nested plugin children
#[derive(Clone)]
pub struct Solar {
    system: CelestialBody,
}

impl Solar {
    pub fn new() -> Self {
        Self {
            system: build_solar_system(),
        }
    }

    /// Find a body by path (e.g., "earth" or "jupiter.io")
    fn find_body(&self, path: &str) -> Option<&CelestialBody> {
        let parts: Vec<&str> = path.split('.').collect();
        let mut current = &self.system;

        for part in parts {
            let normalized = part.to_lowercase();
            if current.name.to_lowercase() == normalized {
                continue;
            }
            current = current.children.iter()
                .find(|c| c.name.to_lowercase() == normalized)?;
        }
        Some(current)
    }

    /// Count all moons in the system
    fn moon_count(&self) -> usize {
        fn count_moons(body: &CelestialBody) -> usize {
            let mine: usize = body.children.iter()
                .filter(|c| c.body_type == BodyType::Moon)
                .count();
            let nested: usize = body.children.iter()
                .map(count_moons)
                .sum();
            mine + nested
        }
        count_moons(&self.system)
    }
}

impl Default for Solar {
    fn default() -> Self {
        Self::new()
    }
}

#[hub_macro::hub_methods(
    namespace = "solar",
    version = "1.0.0",
    description = "Solar system model - demonstrates nested plugin hierarchy",
    hub
)]
impl Solar {
    /// Observe the entire solar system
    #[hub_macro::hub_method(
        description = "Get an overview of the solar system"
    )]
    async fn observe(&self) -> impl Stream<Item = SolarEvent> + Send + 'static {
        let star = self.system.name.clone();
        let planet_count = self.system.children.len();
        let moon_count = self.moon_count();
        let total_bodies = 1 + self.system.descendant_count();

        stream! {
            yield SolarEvent::System {
                star,
                planet_count,
                moon_count,
                total_bodies,
            };
        }
    }

    /// Get information about a specific celestial body
    #[hub_macro::hub_method(
        description = "Get detailed information about a celestial body",
        params(path = "Path to the body (e.g., 'earth', 'jupiter.io', 'saturn.titan')")
    )]
    async fn info(
        &self,
        path: String,
    ) -> impl Stream<Item = SolarEvent> + Send + 'static {
        let body = self.find_body(&path).cloned();

        stream! {
            if let Some(b) = body {
                yield SolarEvent::Body {
                    name: b.name,
                    body_type: b.body_type,
                    mass_kg: b.mass_kg,
                    radius_km: b.radius_km,
                    orbital_period_days: b.orbital_period_days,
                    parent: b.parent,
                };
            }
        }
    }

    /// Get child plugin summaries (planets as children of solar system)
    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        self.system.children.iter()
            .map(|planet| planet.to_child_summary())
            .collect()
    }
}

/// ChildRouter implementation for nested method routing
///
/// This enables calls like `solar.mercury.info` to route through
/// Solar → Mercury → info method.
#[async_trait]
impl ChildRouter for Solar {
    fn router_namespace(&self) -> &str {
        "solar"
    }

    async fn router_call(&self, method: &str, params: serde_json::Value) -> Result<crate::plexus::PlexusStream, crate::plexus::PlexusError> {
        // Delegate to Activation::call which handles local methods + nested routing
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        let normalized = name.to_lowercase();
        self.system.children.iter()
            .find(|c| c.name.to_lowercase() == normalized)
            .map(|c| Box::new(CelestialBodyActivation::new(c.clone())) as Box<dyn ChildRouter>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plexus::{Activation, DynamicHub};

    #[test]
    fn solar_is_hub_with_planets() {
        let solar = Solar::new();
        let schema = solar.plugin_schema();

        assert!(schema.is_hub(), "solar should be a hub");
        let children = schema.children.as_ref().expect("solar should have children");
        assert_eq!(children.len(), 8, "solar should have 8 planets");

        // Children are summaries - check namespace and description
        let jupiter = children.iter().find(|c| c.namespace == "jupiter").unwrap();
        assert!(jupiter.description.contains("planet"));
        assert!(!jupiter.hash.is_empty());
    }

    #[test]
    fn solar_registered_with_dynamic_hub() {
        let hub = DynamicHub::new("plexus").register(Solar::new());
        let schema = hub.plugin_schema();

        // DynamicHub is a hub
        assert!(schema.is_hub());
        let children = schema.children.as_ref().unwrap();

        // Solar should be one of the children (as a summary)
        let solar = children.iter().find(|c| c.namespace == "solar").unwrap();
        assert!(solar.description.contains("Solar system"));
        assert!(!solar.hash.is_empty());
    }

    #[test]
    fn solar_hash_changes_with_structure() {
        let solar1 = Solar::new();
        let solar2 = Solar::new();

        // Same structure = same hash
        assert_eq!(
            solar1.plugin_schema().hash,
            solar2.plugin_schema().hash
        );
    }

    #[test]
    fn print_solar_schema() {
        let solar = Solar::new();
        let schema = solar.plugin_schema();
        let json = serde_json::to_string_pretty(&schema).unwrap();
        println!("Solar system schema:\n{}", json);
    }

    #[tokio::test]
    async fn test_nested_routing_mercury() {
        let solar = Solar::new();
        let result = Activation::call(&solar, "mercury.info", serde_json::json!({})).await;
        assert!(result.is_ok(), "mercury.info should be callable: {:?}", result.err());
    }

    #[tokio::test]
    async fn test_nested_routing_jupiter_io() {
        let solar = Solar::new();

        // Call solar.call("jupiter.io.info", {}) - should route jupiter → io
        let result = Activation::call(&solar, "jupiter.io.info", serde_json::json!({})).await;
        assert!(result.is_ok(), "jupiter.io.info should be callable");
    }

    #[tokio::test]
    async fn test_nested_routing_earth_luna() {
        let solar = Solar::new();

        // Call solar.call("earth.luna.info", {}) - should route earth → luna
        let result = Activation::call(&solar, "earth.luna.info", serde_json::json!({})).await;
        assert!(result.is_ok(), "earth.luna.info should be callable");
    }

    #[tokio::test]
    async fn test_nested_routing_invalid_child() {
        let solar = Solar::new();

        // Call with invalid child
        let result = Activation::call(&solar, "pluto.info", serde_json::json!({})).await;
        assert!(result.is_err(), "pluto.info should fail - not a planet");
    }
}
