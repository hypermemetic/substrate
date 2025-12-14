use serde::{Deserialize, Serialize};

/// Tracks provenance through nested behavior calls
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Provenance {
    /// Ordered list of behavior names in call chain
    /// Example: ["health"], ["agent", "loom"]
    segments: Vec<String>,
}

impl Provenance {
    pub fn root(behavior_name: impl Into<String>) -> Self {
        Self {
            segments: vec![behavior_name.into()],
        }
    }

    pub fn extend(&self, behavior_name: impl Into<String>) -> Self {
        let mut new_path = self.clone();
        new_path.segments.push(behavior_name.into());
        new_path
    }

    pub fn depth(&self) -> usize {
        self.segments.len()
    }

    pub fn segments(&self) -> &[String] {
        &self.segments
    }

    pub fn parent(&self) -> Option<Self> {
        if self.segments.len() <= 1 {
            None
        } else {
            Some(Self {
                segments: self.segments[..self.segments.len() - 1].to_vec(),
            })
        }
    }
}

impl std::fmt::Display for Provenance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.segments.join("."))
    }
}
