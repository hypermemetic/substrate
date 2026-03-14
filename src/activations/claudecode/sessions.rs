/// Session File Management Module
///
/// Provides CRUD operations for Claude Code session files stored as JSONL
/// in ~/.claude/projects/<project>/<session-id>.jsonl
///
/// Also provides integration with arbor for importing/exporting sessions.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::PathBuf;
use tokio::fs;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::activations::arbor::{ArborStorage, TreeId};
use crate::activations::claudecode::types::NodeEvent;

// ═══════════════════════════════════════════════════════════════════════════
// SESSION FILE TYPES
// ═══════════════════════════════════════════════════════════════════════════

/// Event in a session JSONL file
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum SessionEvent {
    /// User message
    #[serde(rename = "user")]
    User {
        #[serde(flatten)]
        data: UserEvent,
    },
    /// Assistant message
    #[serde(rename = "assistant")]
    Assistant {
        #[serde(flatten)]
        data: AssistantEvent,
    },
    /// System message
    #[serde(rename = "system")]
    System {
        #[serde(flatten)]
        data: SystemEvent,
    },
    /// File history snapshot
    #[serde(rename = "file-history-snapshot")]
    FileHistorySnapshot {
        timestamp: Option<String>,
    },
    /// Queue operation
    #[serde(rename = "queue-operation")]
    QueueOperation {
        operation: String,
        timestamp: String,
        #[serde(rename = "sessionId")]
        session_id: String,
    },
    /// Unknown event type
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserEvent {
    pub uuid: String,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub timestamp: String,
    pub cwd: String,
    pub message: UserMessage,
    #[serde(rename = "isSidechain")]
    pub is_sidechain: Option<bool>,
    #[serde(rename = "gitBranch")]
    pub git_branch: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssistantEvent {
    pub uuid: String,
    #[serde(rename = "parentUuid")]
    pub parent_uuid: Option<String>,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub timestamp: String,
    pub cwd: Option<String>,
    pub message: AssistantMessage,
    #[serde(rename = "requestId")]
    pub request_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AssistantMessage {
    /// Full message with content array
    Full {
        role: String,
        content: Vec<ContentBlock>,
        model: Option<String>,
        id: Option<String>,
        #[serde(rename = "stop_reason")]
        stop_reason: Option<String>,
        usage: Option<Value>,
    },
    /// Simple string (for streaming events)
    Simple(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    Text { text: String },
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },
    ToolResult {
        #[serde(rename = "tool_use_id")]
        tool_use_id: String,
        content: String,
        #[serde(rename = "is_error")]
        is_error: Option<bool>,
    },
    Thinking {
        thinking: String,
        signature: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemEvent {
    pub uuid: String,
    #[serde(rename = "sessionId")]
    pub session_id: String,
    pub timestamp: String,
    pub message: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// SESSION FILE OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════

/// Get the base directory for Claude sessions
pub fn get_sessions_base_dir() -> PathBuf {
    dirs::home_dir()
        .expect("Could not determine home directory")
        .join(".claude")
        .join("projects")
}

/// Get the path to a session file
pub fn get_session_path(project_path: &str, session_id: &str) -> PathBuf {
    get_sessions_base_dir()
        .join(project_path)
        .join(format!("{}.jsonl", session_id))
}

/// List all sessions for a project
pub async fn list_sessions(project_path: &str) -> Result<Vec<String>, String> {
    let dir = get_sessions_base_dir().join(project_path);

    if !dir.exists() {
        return Ok(vec![]);
    }

    let mut sessions = vec![];
    let mut entries = fs::read_dir(&dir)
        .await
        .map_err(|e| format!("Failed to read directory: {}", e))?;

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| format!("Failed to read entry: {}", e))?
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) == Some("jsonl") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                sessions.push(stem.to_string());
            }
        }
    }

    Ok(sessions)
}

/// Read all events from a session file
pub async fn read_session(
    project_path: &str,
    session_id: &str,
) -> Result<Vec<SessionEvent>, String> {
    let path = get_session_path(project_path, session_id);

    if !path.exists() {
        return Err(format!("Session not found: {}", session_id));
    }

    let file = fs::File::open(&path)
        .await
        .map_err(|e| format!("Failed to open session file: {}", e))?;

    let reader = BufReader::new(file);
    let mut lines = reader.lines();
    let mut events = vec![];

    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| format!("Failed to read line: {}", e))?
    {
        if line.trim().is_empty() {
            continue;
        }

        match serde_json::from_str::<SessionEvent>(&line) {
            Ok(event) => events.push(event),
            Err(e) => {
                eprintln!("Warning: Failed to parse event: {} - {}", e, &line[..line.len().min(100)]);
                // Continue reading despite parse errors
            }
        }
    }

    Ok(events)
}

/// Append an event to a session file
pub async fn append_to_session(
    project_path: &str,
    session_id: &str,
    event: &SessionEvent,
) -> Result<(), String> {
    let path = get_session_path(project_path, session_id);

    // Ensure directory exists
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("Failed to create directory: {}", e))?;
    }

    let json = serde_json::to_string(event).map_err(|e| format!("Failed to serialize event: {}", e))?;

    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .map_err(|e| format!("Failed to open session file: {}", e))?;

    file.write_all(json.as_bytes())
        .await
        .map_err(|e| format!("Failed to write to session: {}", e))?;
    file.write_all(b"\n")
        .await
        .map_err(|e| format!("Failed to write newline: {}", e))?;

    Ok(())
}

/// Delete a session file
pub async fn delete_session(project_path: &str, session_id: &str) -> Result<(), String> {
    let path = get_session_path(project_path, session_id);

    if !path.exists() {
        return Err(format!("Session not found: {}", session_id));
    }

    fs::remove_file(&path)
        .await
        .map_err(|e| format!("Failed to delete session: {}", e))?;

    Ok(())
}

// ═══════════════════════════════════════════════════════════════════════════
// ARBOR INTEGRATION
// ═══════════════════════════════════════════════════════════════════════════

/// Import a session file into an arbor tree
///
/// Creates a tree structure matching the session conversation flow
pub async fn import_to_arbor(
    arbor: &ArborStorage,
    project_path: &str,
    session_id: &str,
    owner_id: &str,
) -> Result<TreeId, String> {
    let events = read_session(project_path, session_id).await?;

    // Create new tree
    let metadata = serde_json::json!({
        "source": "claude_session_import",
        "session_id": session_id,
        "project_path": project_path,
    });

    let tree_id = arbor
        .tree_create(Some(metadata), owner_id)
        .await
        .map_err(|e| e.to_string())?;

    let tree = arbor.tree_get(&tree_id).await.map_err(|e| e.to_string())?;
    let mut current_parent = tree.root;

    // Process each event
    for event in events {
        match event {
            SessionEvent::User { data } => {
                // Create user message node
                let node_event = NodeEvent::UserMessage {
                    content: data.message.content.clone(),
                };
                let json =
                    serde_json::to_string(&node_event).map_err(|e| format!("Serialize error: {}", e))?;

                let node_id = arbor
                    .node_create_text(&tree_id, Some(current_parent), json, None)
                    .await
                    .map_err(|e| e.to_string())?;

                current_parent = node_id;
            }
            SessionEvent::Assistant { data } => {
                // Create assistant start node
                let start_event = NodeEvent::AssistantStart;
                let json = serde_json::to_string(&start_event)
                    .map_err(|e| format!("Serialize error: {}", e))?;

                let start_node = arbor
                    .node_create_text(&tree_id, Some(current_parent), json, None)
                    .await
                    .map_err(|e| e.to_string())?;

                current_parent = start_node;

                // Process assistant message content
                if let AssistantMessage::Full { content, .. } = data.message {
                    for block in content {
                        let node_event = match block {
                            ContentBlock::Text { text } => NodeEvent::ContentText { text },
                            ContentBlock::ToolUse { id, name, input } => {
                                NodeEvent::ContentToolUse { id, name, input }
                            }
                            ContentBlock::ToolResult {
                                tool_use_id,
                                content,
                                is_error,
                            } => NodeEvent::UserToolResult {
                                tool_use_id,
                                content,
                                is_error: is_error.unwrap_or(false),
                            },
                            ContentBlock::Thinking { thinking, .. } => {
                                NodeEvent::ContentThinking { thinking }
                            }
                        };

                        let json = serde_json::to_string(&node_event)
                            .map_err(|e| format!("Serialize error: {}", e))?;

                        let node_id = arbor
                            .node_create_text(&tree_id, Some(current_parent), json, None)
                            .await
                            .map_err(|e| e.to_string())?;

                        current_parent = node_id;
                    }
                }

                // Create assistant complete node
                let complete_event = NodeEvent::AssistantComplete { usage: None };
                let json = serde_json::to_string(&complete_event)
                    .map_err(|e| format!("Serialize error: {}", e))?;

                let complete_node = arbor
                    .node_create_text(&tree_id, Some(current_parent), json, None)
                    .await
                    .map_err(|e| e.to_string())?;

                current_parent = complete_node;
            }
            _ => {
                // Skip other event types for now
            }
        }
    }

    Ok(tree_id)
}

/// Export an arbor tree to a session JSONL file
///
/// Converts arbor node structure back to claude session format
pub async fn export_from_arbor(
    arbor: &ArborStorage,
    tree_id: &TreeId,
    project_path: &str,
    session_id: &str,
) -> Result<(), String> {
    use crate::activations::arbor::NodeType;
    use crate::activations::claudecode::types::NodeEvent;

    let tree = arbor.tree_get(tree_id).await.map_err(|e| e.to_string())?;

    // Helper to traverse tree in DFS order
    let traverse_dfs = |tree: &crate::activations::arbor::Tree| -> Vec<TreeId> {
        use std::collections::HashMap;

        // Build child map
        let mut children: HashMap<TreeId, Vec<TreeId>> = HashMap::new();
        for (node_id, node) in &tree.nodes {
            if let Some(parent_id) = &node.parent {
                children.entry(*parent_id)
                    .or_insert_with(Vec::new)
                    .push(*node_id);
            }
        }

        // DFS traversal
        let mut visited = Vec::new();
        let mut stack = vec![tree.root];

        while let Some(current) = stack.pop() {
            visited.push(current);
            if let Some(child_ids) = children.get(&current) {
                // Reverse to maintain left-to-right order
                for child_id in child_ids.iter().rev() {
                    stack.push(*child_id);
                }
            }
        }

        visited
    };

    let node_ids = traverse_dfs(&tree);

    // Parse NodeEvents and aggregate into SessionEvents
    let mut session_events = Vec::new();
    let mut current_assistant_blocks: Vec<ContentBlock> = Vec::new();
    let mut in_assistant = false;

    for node_id in node_ids {
        let node = tree.nodes.get(&node_id).unwrap();

        if let NodeType::Text { content } = &node.data {
            // Skip empty content (like root node)
            if content.is_empty() {
                continue;
            }

            // Try to parse as NodeEvent
            let node_event: NodeEvent = match serde_json::from_str(content) {
                Ok(e) => e,
                Err(_) => continue, // Skip nodes that aren't NodeEvents
            };

            match node_event {
                NodeEvent::UserMessage { content } => {
                    // Complete any pending assistant message
                    if in_assistant && !current_assistant_blocks.is_empty() {
                        session_events.push(build_assistant_event(
                            std::mem::take(&mut current_assistant_blocks),
                            session_id,
                        ));
                        in_assistant = false;
                    }

                    // Create user event
                    session_events.push(SessionEvent::User {
                        data: UserEvent {
                            uuid: uuid::Uuid::new_v4().to_string(),
                            parent_uuid: None,
                            session_id: session_id.to_string(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            cwd: std::env::current_dir()
                                .ok()
                                .and_then(|p| p.to_str().map(String::from))
                                .unwrap_or_default(),
                            message: UserMessage {
                                role: "user".to_string(),
                                content,
                            },
                            is_sidechain: None,
                            git_branch: None,
                        },
                    });
                }

                NodeEvent::AssistantStart => {
                    in_assistant = true;
                    current_assistant_blocks.clear();
                }

                NodeEvent::ContentText { text } => {
                    if in_assistant {
                        current_assistant_blocks.push(ContentBlock::Text { text });
                    }
                }

                NodeEvent::ContentToolUse { id, name, input } => {
                    if in_assistant {
                        current_assistant_blocks.push(ContentBlock::ToolUse { id, name, input });
                    }
                }

                NodeEvent::ContentThinking { thinking } => {
                    if in_assistant {
                        current_assistant_blocks.push(ContentBlock::Thinking {
                            thinking,
                            signature: None,
                        });
                    }
                }

                NodeEvent::UserToolResult {
                    tool_use_id,
                    content,
                    is_error,
                } => {
                    // Complete any pending assistant message
                    if in_assistant && !current_assistant_blocks.is_empty() {
                        session_events.push(build_assistant_event(
                            std::mem::take(&mut current_assistant_blocks),
                            session_id,
                        ));
                        in_assistant = false;
                    }

                    // Tool results become user messages in Claude API
                    let content_str = serde_json::to_string(&vec![ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error: Some(is_error),
                    }])
                    .unwrap_or_default();

                    session_events.push(SessionEvent::User {
                        data: UserEvent {
                            uuid: uuid::Uuid::new_v4().to_string(),
                            parent_uuid: None,
                            session_id: session_id.to_string(),
                            timestamp: chrono::Utc::now().to_rfc3339(),
                            cwd: std::env::current_dir()
                                .ok()
                                .and_then(|p| p.to_str().map(String::from))
                                .unwrap_or_default(),
                            message: UserMessage {
                                role: "user".to_string(),
                                content: content_str,
                            },
                            is_sidechain: None,
                            git_branch: None,
                        },
                    });
                }

                NodeEvent::AssistantComplete { .. } => {
                    if in_assistant && !current_assistant_blocks.is_empty() {
                        session_events.push(build_assistant_event(
                            std::mem::take(&mut current_assistant_blocks),
                            session_id,
                        ));
                        in_assistant = false;
                    }
                }

                // Debug/observability nodes — not part of conversation history
                NodeEvent::LaunchCommand { .. } | NodeEvent::ClaudeStderr { .. } => {}
            }
        }
    }

    // Complete any pending assistant message
    if in_assistant && !current_assistant_blocks.is_empty() {
        session_events.push(build_assistant_event(current_assistant_blocks, session_id));
    }

    // Write to session file
    for event in session_events {
        append_to_session(project_path, session_id, &event).await?;
    }

    Ok(())
}

/// Helper to build AssistantEvent from content blocks
fn build_assistant_event(blocks: Vec<ContentBlock>, session_id: &str) -> SessionEvent {
    SessionEvent::Assistant {
        data: AssistantEvent {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            session_id: session_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            cwd: std::env::current_dir()
                .ok()
                .and_then(|p| p.to_str().map(String::from)),
            message: AssistantMessage::Full {
                role: "assistant".to_string(),
                content: blocks,
                model: None,
                id: None,
                stop_reason: None,
                usage: None,
            },
            request_id: None,
        },
    }
}
