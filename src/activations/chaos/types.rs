use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A running node found across all active graphs
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RunningNode {
    pub graph_id: String,
    pub node_id: String,
    pub spec_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListRunningResult {
    #[serde(rename = "node")]
    Node(RunningNode),
    #[serde(rename = "done")]
    Done { count: usize },
    #[serde(rename = "error")]
    Err { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum InjectResult {
    #[serde(rename = "ok")]
    Ok { graph_id: String, node_id: String, action: String },
    #[serde(rename = "skipped")]
    Skipped { reason: String },
    #[serde(rename = "error")]
    Err { message: String },
}

/// A process found on the system
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProcessInfo {
    pub pid: u32,
    pub cmdline: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ListProcessesResult {
    #[serde(rename = "process")]
    Process(ProcessInfo),
    #[serde(rename = "done")]
    Done { count: usize },
    #[serde(rename = "error")]
    Err { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum KillProcessResult {
    #[serde(rename = "killed")]
    Killed { pid: u32 },
    #[serde(rename = "not_found")]
    NotFound,
    #[serde(rename = "error")]
    Err { message: String },
}

/// Per-node status snapshot
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct NodeSnapshot {
    pub node_id: String,
    pub status: String,
    pub spec_type: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GraphSnapshotResult {
    #[serde(rename = "node")]
    Node(NodeSnapshot),
    #[serde(rename = "summary")]
    Summary {
        graph_id: String,
        graph_status: String,
        total: usize,
        pending: usize,
        ready: usize,
        running: usize,
        complete: usize,
        failed: usize,
    },
    #[serde(rename = "error")]
    Err { message: String },
}
