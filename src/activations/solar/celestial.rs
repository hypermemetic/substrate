//! Celestial body definitions
//!
//! Each celestial body is a plugin that can have children (moons).
//! This demonstrates the coalgebraic structure: bodies are observed
//! to reveal their properties and children.

use crate::plexus::{
    Activation, ChildRouter, MethodSchema, MethodEnumSchema, PlexusError, PlexusStream,
    PluginSchema, wrap_stream,
};
use async_stream::stream;
use async_trait::async_trait;
use futures::Stream;
use jsonrpsee::core::server::Methods;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::types::{BodyType, SolarEvent};

/// A celestial body in the solar system
#[derive(Debug, Clone)]
pub struct CelestialBody {
    pub name: String,
    pub body_type: BodyType,
    pub mass_kg: f64,
    pub radius_km: f64,
    pub orbital_period_days: Option<f64>,
    pub parent: Option<String>,
    pub children: Vec<CelestialBody>,
}

impl CelestialBody {
    /// Create a new star (root of a system)
    pub fn star(name: &str, mass_kg: f64, radius_km: f64) -> Self {
        Self {
            name: name.to_string(),
            body_type: BodyType::Star,
            mass_kg,
            radius_km,
            orbital_period_days: None,
            parent: None,
            children: Vec::new(),
        }
    }

    /// Create a new planet
    pub fn planet(name: &str, mass_kg: f64, radius_km: f64, orbital_period_days: f64) -> Self {
        Self {
            name: name.to_string(),
            body_type: BodyType::Planet,
            mass_kg,
            radius_km,
            orbital_period_days: Some(orbital_period_days),
            parent: None,
            children: Vec::new(),
        }
    }

    /// Create a new moon
    pub fn moon(name: &str, mass_kg: f64, radius_km: f64, orbital_period_days: f64) -> Self {
        Self {
            name: name.to_string(),
            body_type: BodyType::Moon,
            mass_kg,
            radius_km,
            orbital_period_days: Some(orbital_period_days),
            parent: None,
            children: Vec::new(),
        }
    }

    /// Add a child body (moon to planet, planet to star)
    pub fn with_child(mut self, mut child: CelestialBody) -> Self {
        child.parent = Some(self.name.clone());
        self.children.push(child);
        self
    }

    /// Add multiple children
    pub fn with_children(mut self, children: Vec<CelestialBody>) -> Self {
        for mut child in children {
            child.parent = Some(self.name.clone());
            self.children.push(child);
        }
        self
    }

    /// Check if this body has children
    pub fn has_children(&self) -> bool {
        !self.children.is_empty()
    }

    /// Count all descendants recursively
    pub fn descendant_count(&self) -> usize {
        self.children.iter()
            .map(|c| 1 + c.descendant_count())
            .sum()
    }

    /// Generate the PluginSchema for this celestial body (coalgebra)
    ///
    /// This is the F-coalgebra structure map: CelestialBody → F(CelestialBody)
    /// It reveals one layer of structure, recursively producing child schemas.
    pub fn to_plugin_schema(&self) -> PluginSchema {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        // Create method for observing this body
        let info_desc = format!("Get information about {}", self.name);
        let mut hasher = DefaultHasher::new();
        "info".hash(&mut hasher);
        info_desc.hash(&mut hasher);
        let info_hash = format!("{:016x}", hasher.finish());

        let methods = vec![
            MethodSchema::new("info", &info_desc, info_hash),
        ];

        let namespace = self.name.to_lowercase().replace(' ', "_");
        let version = "1.0.0";
        let description = match self.body_type {
            BodyType::Star => format!("{} - the central star", self.name),
            BodyType::Planet => format!("{} - planet", self.name),
            BodyType::DwarfPlanet => format!("{} - dwarf planet", self.name),
            BodyType::Moon => format!("{} - moon of {}", self.name,
                self.parent.as_deref().unwrap_or("unknown")),
        };

        if self.has_children() {
            // Hub: recursively generate child schemas (anamorphism)
            let child_schemas: Vec<PluginSchema> = self.children
                .iter()
                .map(|c| c.to_plugin_schema())
                .collect();

            PluginSchema::hub(namespace, version, description, methods, child_schemas)
        } else {
            // Leaf: no children
            PluginSchema::leaf(namespace, version, description, methods)
        }
    }
}

// ============================================================================
// CelestialBodyActivation - makes CelestialBody callable
// ============================================================================

/// Method enum for celestial body - just "info"
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum CelestialBodyMethod {
    Info,
}

impl MethodEnumSchema for CelestialBodyMethod {
    fn method_names() -> &'static [&'static str] {
        &["info"]
    }

    fn schema_with_consts() -> Value {
        serde_json::to_value(schemars::schema_for!(CelestialBodyMethod))
            .expect("Schema should serialize")
    }
}

/// Activation wrapper for CelestialBody
///
/// This makes a celestial body callable as a plugin.
/// It implements both Activation (for method dispatch) and
/// ChildRouter (for nested routing to moons).
#[derive(Clone)]
pub struct CelestialBodyActivation {
    body: CelestialBody,
    namespace: String,
}

impl CelestialBodyActivation {
    pub fn new(body: CelestialBody) -> Self {
        let namespace = body.name.to_lowercase().replace(' ', "_");
        Self { body, namespace }
    }

    fn info_stream(&self) -> impl Stream<Item = SolarEvent> + Send + 'static {
        let body = self.body.clone();
        stream! {
            yield SolarEvent::Body {
                name: body.name,
                body_type: body.body_type,
                mass_kg: body.mass_kg,
                radius_km: body.radius_km,
                orbital_period_days: body.orbital_period_days,
                parent: body.parent,
            };
        }
    }
}

#[async_trait]
impl Activation for CelestialBodyActivation {
    type Methods = CelestialBodyMethod;

    fn namespace(&self) -> &str {
        &self.namespace
    }

    fn version(&self) -> &str {
        "1.0.0"
    }

    fn description(&self) -> &str {
        // Can't return dynamic string without lifetime issues
        // The schema has the full description
        "Celestial body"
    }

    fn methods(&self) -> Vec<&str> {
        vec!["info", "schema"]
    }

    fn method_help(&self, method: &str) -> Option<String> {
        match method {
            "info" => Some(format!("Get information about {}", self.body.name)),
            "schema" => Some("Get this plugin's schema (shallow - children as summaries only)".to_string()),
            _ => None,
        }
    }

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        match method {
            "info" => {
                let stream = self.info_stream();
                // Use static content type to avoid lifetime issues
                Ok(wrap_stream(stream, "celestial.info", vec![self.namespace.clone()]))
            }
            "schema" => {
                let schema = self.plugin_schema().shallow();
                let ns = self.namespace.clone();
                Ok(wrap_stream(
                    futures::stream::once(async move { schema }),
                    "celestial.schema",
                    vec![ns]
                ))
            }
            _ => {
                // Try routing to child
                crate::plexus::route_to_child(self, method, params).await
            }
        }
    }

    fn into_rpc_methods(self) -> Methods {
        // Celestial bodies don't register their own RPC methods
        // They're called through the parent (Solar) routing
        Methods::new()
    }

    fn plugin_schema(&self) -> PluginSchema {
        self.body.to_plugin_schema()
    }
}

#[async_trait]
impl ChildRouter for CelestialBodyActivation {
    fn router_namespace(&self) -> &str {
        &self.namespace
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        // Delegate to Activation::call which handles local methods + nested routing
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        let normalized = name.to_lowercase();
        self.body.children.iter()
            .find(|c| c.name.to_lowercase() == normalized)
            .map(|c| Box::new(CelestialBodyActivation::new(c.clone())) as Box<dyn ChildRouter>)
    }
}

// ============================================================================
// Solar System Data
// ============================================================================

/// Build the real solar system with accurate data
pub fn build_solar_system() -> CelestialBody {
    CelestialBody::star("Sol", 1.989e30, 696_340.0)
        .with_children(vec![
            // Mercury - no moons
            CelestialBody::planet("Mercury", 3.285e23, 2_439.7, 87.97),

            // Venus - no moons
            CelestialBody::planet("Venus", 4.867e24, 6_051.8, 224.7),

            // Earth with Moon
            CelestialBody::planet("Earth", 5.972e24, 6_371.0, 365.25)
                .with_child(
                    CelestialBody::moon("Luna", 7.342e22, 1_737.4, 27.32)
                ),

            // Mars with moons
            CelestialBody::planet("Mars", 6.39e23, 3_389.5, 687.0)
                .with_children(vec![
                    CelestialBody::moon("Phobos", 1.0659e16, 11.267, 0.319),
                    CelestialBody::moon("Deimos", 1.4762e15, 6.2, 1.263),
                ]),

            // Jupiter with major moons (Galilean)
            CelestialBody::planet("Jupiter", 1.898e27, 69_911.0, 4_333.0)
                .with_children(vec![
                    CelestialBody::moon("Io", 8.932e22, 1_821.6, 1.769),
                    CelestialBody::moon("Europa", 4.8e22, 1_560.8, 3.551),
                    CelestialBody::moon("Ganymede", 1.4819e23, 2_634.1, 7.155),
                    CelestialBody::moon("Callisto", 1.0759e23, 2_410.3, 16.689),
                ]),

            // Saturn with major moons
            CelestialBody::planet("Saturn", 5.683e26, 58_232.0, 10_759.0)
                .with_children(vec![
                    CelestialBody::moon("Titan", 1.3452e23, 2_574.7, 15.945),
                    CelestialBody::moon("Enceladus", 1.08e20, 252.1, 1.370),
                    CelestialBody::moon("Mimas", 3.749e19, 198.2, 0.942),
                ]),

            // Uranus with major moons
            CelestialBody::planet("Uranus", 8.681e25, 25_362.0, 30_687.0)
                .with_children(vec![
                    CelestialBody::moon("Titania", 3.527e21, 788.9, 8.706),
                    CelestialBody::moon("Oberon", 3.014e21, 761.4, 13.463),
                    CelestialBody::moon("Miranda", 6.59e19, 235.8, 1.413),
                ]),

            // Neptune with Triton
            CelestialBody::planet("Neptune", 1.024e26, 24_622.0, 60_190.0)
                .with_child(
                    CelestialBody::moon("Triton", 2.14e22, 1_353.4, 5.877)
                ),
        ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_solar_system() {
        let sol = build_solar_system();
        assert_eq!(sol.name, "Sol");
        assert_eq!(sol.body_type, BodyType::Star);
        assert_eq!(sol.children.len(), 8); // 8 planets
    }

    #[test]
    fn test_earth_has_moon() {
        let sol = build_solar_system();
        let earth = sol.children.iter().find(|p| p.name == "Earth").unwrap();
        assert_eq!(earth.children.len(), 1);
        assert_eq!(earth.children[0].name, "Luna");
    }

    #[test]
    fn test_descendant_count() {
        let sol = build_solar_system();
        let total = sol.descendant_count();
        // 8 planets + 1 + 2 + 4 + 3 + 3 + 1 = 8 + 14 = 22 bodies (excluding Sol)
        assert_eq!(total, 22);
    }

    #[test]
    fn test_plugin_schema_hierarchy() {
        let sol = build_solar_system();
        let schema = sol.to_plugin_schema();

        assert!(schema.is_hub());
        let children = schema.children.as_ref().unwrap();
        assert_eq!(children.len(), 8); // 8 planets

        // Mercury should be a leaf (no moons)
        let mercury = children.iter().find(|c| c.namespace == "mercury").unwrap();
        assert!(mercury.is_leaf());

        // Earth should be a hub (has Luna)
        let earth = children.iter().find(|c| c.namespace == "earth").unwrap();
        assert!(earth.is_hub());
        let earth_children = earth.children.as_ref().unwrap();
        assert_eq!(earth_children.len(), 1);
        assert_eq!(earth_children[0].namespace, "luna");
    }
}
