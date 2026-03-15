/// Arbor View System
///
/// Provides the ability to create view trees that reference ranges of nodes
/// from storage trees without copying data.
///
/// Key Concepts:
/// - Storage Tree: Immutable, granular source of truth
/// - View Tree: References nodes/ranges via external handles
/// - Range Handle: External node pointing to [start → end] in another tree
/// - Resolve Mode: Control whether to expand ranges or show placeholders

use serde::{Deserialize, Serialize};
use serde_json::Value;
use schemars::JsonSchema;
use std::time::{SystemTime, UNIX_EPOCH};
use std::collections::{HashMap, HashSet};

use super::storage::ArborStorage;
use super::types::{ArborError, NodeId, NodeType, TreeId, Tree};

// ═══════════════════════════════════════════════════════════════════════════
// VIEW TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Handle for referencing a range of nodes in another tree
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeHandle {
    /// Source tree containing the range
    pub tree_id: TreeId,
    /// First node in the range
    pub start_node: NodeId,
    /// Last node in the range (must be descendant of start)
    pub end_node: NodeId,
    /// How to collapse this range
    pub collapse_type: CollapseType,
}

impl RangeHandle {
    /// Convert to metadata Value that can be stored in arbor
    pub fn to_metadata(&self) -> Value {
        serde_json::json!({
            "range_handle": {
                "tree_id": self.tree_id,
                "start_node": self.start_node,
                "end_node": self.end_node,
                "collapse_type": self.collapse_type,
            }
        })
    }

    /// Try to parse metadata as a RangeHandle
    pub fn from_metadata(metadata: &Value) -> Option<Self> {
        use super::types::ArborId;

        let range_obj = metadata.get("range_handle")?;
        let tree_id_str = range_obj.get("tree_id")?.as_str()?;
        let start_node_str = range_obj.get("start_node")?.as_str()?;
        let end_node_str = range_obj.get("end_node")?.as_str()?;

        let tree_id = ArborId::parse_str(tree_id_str).ok()?;
        let start_node = ArborId::parse_str(start_node_str).ok()?;
        let end_node = ArborId::parse_str(end_node_str).ok()?;

        let collapse_type = serde_json::from_value(
            range_obj.get("collapse_type")?.clone()
        ).ok()?;

        Some(RangeHandle {
            tree_id,
            start_node,
            end_node,
            collapse_type,
        })
    }
}

/// Strategy for collapsing a range
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CollapseType {
    /// Merge all text content into a single concatenated string
    TextMerge,
    /// Keep structure but reference externally (placeholder)
    StructureRef,
    /// Custom collapse with user-defined strategy name
    Custom { strategy: String },
}

/// Mode for resolving range handles during rendering
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ResolveMode {
    /// Fully resolve all range handles to their content
    Full,
    /// Show ranges as placeholders (e.g., "[range: 5 nodes]")
    Placeholder,
    /// Resolve ranges up to a maximum depth
    Partial { max_depth: usize },
}

/// Specification for creating a range reference
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RangeSpec {
    /// Source tree
    pub tree_id: TreeId,
    /// Start node of range
    pub start_node: NodeId,
    /// End node of range (must be descendant of start)
    pub end_node: NodeId,
    /// Collapse strategy
    pub collapse_type: CollapseType,
}

/// Result of analyzing text runs in a tree
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextRun {
    /// First node in the run
    pub start_node: NodeId,
    /// Last node in the run
    pub end_node: NodeId,
    /// Number of nodes in the run
    pub length: usize,
    /// Total character count
    pub char_count: usize,
}

// ═══════════════════════════════════════════════════════════════════════════
// VIEW OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════

impl ArborStorage {
    /// Create a new view tree with metadata indicating it's a view
    pub async fn view_create(
        &self,
        source_tree_id: &TreeId,
        owner_id: &str,
    ) -> Result<TreeId, ArborError> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let metadata = serde_json::json!({
            "is_view": true,
            "source_tree": source_tree_id,
            "created_at": now,
        });

        self.tree_create(Some(metadata), owner_id).await
    }

    /// Add a range reference as a text node with merged content and metadata
    pub async fn view_add_range(
        &self,
        view_tree_id: &TreeId,
        parent_node: &NodeId,
        range_spec: RangeSpec,
    ) -> Result<NodeId, ArborError> {
        let range_handle = RangeHandle {
            tree_id: range_spec.tree_id.clone(),
            start_node: range_spec.start_node.clone(),
            end_node: range_spec.end_node.clone(),
            collapse_type: range_spec.collapse_type.clone(),
        };

        // Get merged content from the range
        let range_content = self.range_get(
            &range_spec.tree_id,
            &range_spec.start_node,
            &range_spec.end_node,
            &range_spec.collapse_type,
        ).await?;

        // Create appropriate NodeEvent with merged content
        use crate::activations::claudecode::NodeEvent;
        let merged_content = match range_content {
            RangeContent::Text { content, .. } => {
                // Wrap merged text in a ContentText NodeEvent
                let node_event = NodeEvent::ContentText { text: content };
                serde_json::to_string(&node_event).unwrap_or_default()
            }
            RangeContent::Reference { .. } => {
                // For structure refs, store a JSON representation
                serde_json::to_string(&range_handle).unwrap_or_default()
            }
            RangeContent::Custom { metadata, .. } => {
                // For custom, store metadata JSON
                metadata.to_string()
            }
        };

        let metadata = range_handle.to_metadata();

        // Store merged content in the node with range metadata for provenance
        self.node_create_text(
            view_tree_id,
            Some(*parent_node),
            merged_content,  // Pre-computed merged content
            Some(metadata),  // Metadata indicates this came from a range
        )
        .await
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // TREE TRAVERSAL HELPERS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Build a map of parent_id -> Vec<child_ids> from the tree's parent pointers
    fn build_child_map(tree: &Tree) -> HashMap<NodeId, Vec<NodeId>> {
        let mut children: HashMap<NodeId, Vec<NodeId>> = HashMap::new();

        for (node_id, node) in &tree.nodes {
            if let Some(parent_id) = &node.parent {
                children.entry(*parent_id)
                    .or_insert_with(Vec::new)
                    .push(*node_id);
            }
        }

        children
    }

    /// Perform depth-first traversal starting from a node
    fn traverse_dfs_from(
        current_id: &NodeId,
        children: &HashMap<NodeId, Vec<NodeId>>,
        visited: &mut Vec<NodeId>
    ) {
        visited.push(*current_id);

        if let Some(child_ids) = children.get(current_id) {
            for child_id in child_ids {
                Self::traverse_dfs_from(child_id, children, visited);
            }
        }
    }

    /// Traverse an entire tree in depth-first order
    fn traverse_tree_dfs(tree: &Tree) -> Vec<NodeId> {
        // Find root (node with no parent)
        let root_id = tree.nodes.iter()
            .find(|(_, node)| node.parent.is_none())
            .map(|(id, _)| *id);

        let Some(root_id) = root_id else {
            // Empty tree or no root found
            return Vec::new();
        };

        let children = Self::build_child_map(tree);
        let mut visited = Vec::new();
        Self::traverse_dfs_from(&root_id, &children, &mut visited);
        visited
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // VIEW OPERATIONS
    // ═══════════════════════════════════════════════════════════════════════════

    /// Detect consecutive text nodes and return runs longer than threshold
    pub async fn view_detect_text_runs(
        &self,
        tree_id: &TreeId,
        min_length: usize,
    ) -> Result<Vec<TextRun>, ArborError> {
        let tree = self.tree_get(tree_id).await?;

        let mut runs = Vec::new();
        let mut current_run: Option<(NodeId, NodeId, usize, usize)> = None; // (start, end, count, chars)

        // Helper to check if a node is a text node
        let is_text_node = |node_id: &NodeId| -> Option<usize> {
            let node = tree.nodes.get(node_id)?;

            match &node.data {
                NodeType::Text { content } => {
                    // Try parsing as JSON first (for Claude session format)
                    if let Ok(parsed) = serde_json::from_str::<Value>(content) {
                        if parsed.get("type")?.as_str()? == "content_text" {
                            let text = parsed.get("text")?.as_str()?;
                            return Some(text.len());
                        }
                    }

                    // Otherwise treat as plain text
                    // Skip empty text nodes (like root node)
                    if content.is_empty() {
                        None
                    } else {
                        Some(content.len())
                    }
                }
                NodeType::External { .. } => None,
            }
        };

        // Traverse tree in depth-first order
        let node_ids = Self::traverse_tree_dfs(&tree);

        for node_id in &node_ids {
            if let Some(char_count) = is_text_node(node_id) {
                match &mut current_run {
                    Some((start, end, count, chars)) => {
                        // Check if this node is a child of the previous end
                        let node = tree.nodes.get(node_id).unwrap();
                        if node.parent.as_ref() == Some(end) {
                            // Extend the run
                            *end = node_id.clone();
                            *count += 1;
                            *chars += char_count;
                        } else {
                            // End current run, start new one
                            if *count >= min_length {
                                runs.push(TextRun {
                                    start_node: start.clone(),
                                    end_node: end.clone(),
                                    length: *count,
                                    char_count: *chars,
                                });
                            }
                            current_run = Some((node_id.clone(), node_id.clone(), 1, char_count));
                        }
                    }
                    None => {
                        // Start new run
                        current_run = Some((node_id.clone(), node_id.clone(), 1, char_count));
                    }
                }
            } else {
                // Not a text node - end current run
                if let Some((start, end, count, chars)) = current_run.take() {
                    if count >= min_length {
                        runs.push(TextRun {
                            start_node: start,
                            end_node: end,
                            length: count,
                            char_count: chars,
                        });
                    }
                }
            }
        }

        // Don't forget the last run
        if let Some((start, end, count, chars)) = current_run {
            if count >= min_length {
                runs.push(TextRun {
                    start_node: start,
                    end_node: end,
                    length: count,
                    char_count: chars,
                });
            }
        }

        Ok(runs)
    }

    /// Create a view tree that collapses consecutive text runs
    pub async fn view_collapse_text_runs(
        &self,
        source_tree_id: &TreeId,
        min_run_length: usize,
        owner_id: &str,
    ) -> Result<(TreeId, Vec<TextRun>), ArborError> {
        // Detect text runs
        let runs = self.view_detect_text_runs(source_tree_id, min_run_length).await?;

        // Create view tree
        let view_tree_id = self.view_create(source_tree_id, owner_id).await?;

        // Get source tree to traverse structure
        let source_tree = self.tree_get(source_tree_id).await?;

        // Build child map for traversal
        let children = Self::build_child_map(&source_tree);

        // Build sets for efficient lookup:
        // 1. All nodes that are part of collapsed runs
        // 2. Start nodes of each run (these become range references)
        let mut collapsed_nodes: HashSet<NodeId> = HashSet::new();
        let mut run_starts: HashMap<NodeId, &TextRun> = HashMap::new();

        for run in &runs {
            // Collect all nodes in this run by traversing from start to end
            let mut current = run.start_node;
            run_starts.insert(current, run);
            collapsed_nodes.insert(current);

            // Traverse children until we reach end_node
            while current != run.end_node {
                if let Some(child_ids) = children.get(&current) {
                    // In a linear text chain, there should be only one child
                    if let Some(&child_id) = child_ids.first() {
                        current = child_id;
                        collapsed_nodes.insert(current);
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
        }

        // Traverse source tree in DFS order and build view
        // Track mapping from source node ID -> view node ID for parent relationships
        let mut node_mapping: HashMap<NodeId, NodeId> = HashMap::new();
        let view_tree = self.tree_get(&view_tree_id).await?;

        // Map source root to view root
        if let Some(source_root) = source_tree.nodes.iter()
            .find(|(_, node)| node.parent.is_none())
            .map(|(id, _)| *id)
        {
            node_mapping.insert(source_root, view_tree.root);
        }

        let traversal_order = Self::traverse_tree_dfs(&source_tree);

        for source_node_id in traversal_order {
            // Skip if we've already processed this as part of a collapsed run (but not the start)
            if collapsed_nodes.contains(&source_node_id) && !run_starts.contains_key(&source_node_id) {
                continue;
            }

            let source_node = source_tree.nodes.get(&source_node_id).unwrap();

            // Find parent in view tree (map from source parent)
            let view_parent = source_node.parent.as_ref()
                .and_then(|source_parent| node_mapping.get(source_parent).copied());

            // If this is the start of a collapsed run, add range reference
            if let Some(run) = run_starts.get(&source_node_id) {
                let range_spec = RangeSpec {
                    tree_id: *source_tree_id,
                    start_node: run.start_node,
                    end_node: run.end_node,
                    collapse_type: CollapseType::TextMerge,
                };

                if let Ok(view_node_id) = self.view_add_range(&view_tree_id, &view_parent.unwrap_or(view_tree.root), range_spec).await {
                    // Map the start node to the range reference node
                    node_mapping.insert(source_node_id, view_node_id);
                }
            } else {
                // Copy the node to view tree (preserve structure for non-collapsed nodes)
                let view_node_id = match &source_node.data {
                    NodeType::Text { content } => {
                        self.node_create_text(
                            &view_tree_id,
                            view_parent,
                            content.clone(),
                            source_node.metadata.clone(),
                        ).await.ok()
                    }
                    NodeType::External { handle } => {
                        self.node_create_external(
                            &view_tree_id,
                            view_parent,
                            handle.clone(),
                            source_node.metadata.clone(),
                        ).await.ok()
                    }
                };

                if let Some(view_node_id) = view_node_id {
                    node_mapping.insert(source_node_id, view_node_id);
                }
            }
        }

        Ok((view_tree_id, runs))
    }

    /// Get merged content from a range of nodes
    pub async fn range_get(
        &self,
        tree_id: &TreeId,
        start_node: &NodeId,
        end_node: &NodeId,
        collapse_type: &CollapseType,
    ) -> Result<RangeContent, ArborError> {
        let tree = self.tree_get(tree_id).await?;

        // Get path from start to end
        let mut path_nodes = Vec::new();
        let mut current = end_node.clone();

        // Walk up from end to start (or root)
        while current != *start_node {
            path_nodes.push(current);

            let node = tree.nodes.get(&current)
                .ok_or_else(|| ArborError::NodeNotFound {
                    node_id: current.to_string(),
                    tree_id: tree_id.to_string(),
                })?;

            if let Some(parent) = &node.parent {
                current = *parent;
            } else {
                return Err(ArborError::InvalidState {
                    message: format!("end_node {} is not a descendant of start_node {} in tree {}", end_node, start_node, tree_id),
                });
            }
        }

        path_nodes.push(start_node.clone());
        path_nodes.reverse();

        match collapse_type {
            CollapseType::TextMerge => {
                // Merge all text content
                let mut merged_text = String::new();

                for node_id in &path_nodes {
                    if let Some(node) = tree.nodes.get(node_id) {
                        if let NodeType::Text { content } = &node.data {
                            // Try parsing as JSON first (for Claude session format)
                            if let Ok(parsed) = serde_json::from_str::<Value>(content) {
                                if parsed.get("type").and_then(|t| t.as_str()) == Some("content_text") {
                                    if let Some(text) = parsed.get("text").and_then(|t| t.as_str()) {
                                        merged_text.push_str(text);
                                    }
                                    continue;
                                }
                            }

                            // Otherwise treat as plain text (skip empty content)
                            if !content.is_empty() {
                                merged_text.push_str(content);
                            }
                        }
                    }
                }

                Ok(RangeContent::Text {
                    content: merged_text,
                    node_count: path_nodes.len(),
                    node_ids: path_nodes,
                })
            }
            CollapseType::StructureRef => {
                Ok(RangeContent::Reference {
                    tree_id: tree_id.clone(),
                    start_node: start_node.clone(),
                    end_node: end_node.clone(),
                    node_count: path_nodes.len(),
                })
            }
            CollapseType::Custom { strategy } => {
                Ok(RangeContent::Custom {
                    strategy: strategy.clone(),
                    node_count: path_nodes.len(),
                    metadata: serde_json::json!({
                        "tree_id": tree_id,
                        "start": start_node,
                        "end": end_node,
                    }),
                })
            }
        }
    }
}

/// Result of resolving a range
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RangeContent {
    /// Merged text content
    Text {
        content: String,
        node_count: usize,
        node_ids: Vec<NodeId>,
    },
    /// Structure reference (placeholder)
    Reference {
        tree_id: TreeId,
        start_node: NodeId,
        end_node: NodeId,
        node_count: usize,
    },
    /// Custom collapse strategy
    Custom {
        strategy: String,
        node_count: usize,
        metadata: Value,
    },
}
