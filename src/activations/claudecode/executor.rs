use super::types::{Model, RawClaudeEvent};
use async_stream::stream;
use futures::Stream;
use serde_json::Value;
use std::path::PathBuf;
use std::pin::Pin;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// Configuration for a Claude Code session launch
#[derive(Debug, Clone)]
pub struct LaunchConfig {
    /// The query/prompt to send
    pub query: String,
    /// Resume an existing Claude session
    pub session_id: Option<String>,
    /// Fork the session instead of resuming
    pub fork_session: bool,
    /// Model to use
    pub model: Model,
    /// Working directory
    pub working_dir: String,
    /// System prompt
    pub system_prompt: Option<String>,
    /// MCP configuration (written to temp file)
    pub mcp_config: Option<Value>,
    /// Permission prompt tool name
    pub permission_prompt_tool: Option<String>,
    /// Allowed tools
    pub allowed_tools: Vec<String>,
    /// Disallowed tools
    pub disallowed_tools: Vec<String>,
    /// Max turns
    pub max_turns: Option<i32>,
    /// Enable loopback mode - routes tool permissions through Plexus for parent approval
    pub loopback_enabled: bool,
    /// Session ID for loopback correlation
    pub loopback_session_id: Option<String>,
}

impl Default for LaunchConfig {
    fn default() -> Self {
        Self {
            query: String::new(),
            session_id: None,
            fork_session: false,
            model: Model::Sonnet,
            working_dir: ".".to_string(),
            system_prompt: None,
            mcp_config: None,
            permission_prompt_tool: None,
            allowed_tools: Vec::new(),
            disallowed_tools: Vec::new(),
            max_turns: None,
            loopback_enabled: false,
            loopback_session_id: None,
        }
    }
}

/// Executor that wraps the Claude Code CLI
#[derive(Clone)]
pub struct ClaudeCodeExecutor {
    claude_path: String,
}

impl ClaudeCodeExecutor {
    pub fn new() -> Self {
        Self {
            claude_path: Self::find_claude_binary().unwrap_or_else(|| "claude".to_string()),
        }
    }

    pub fn with_path(path: String) -> Self {
        Self { claude_path: path }
    }

    /// Discover the Claude binary location
    fn find_claude_binary() -> Option<String> {
        // Check common locations
        let home = dirs::home_dir()?;

        let candidates = [
            home.join(".claude/local/claude"),
            home.join(".npm/bin/claude"),
            home.join(".bun/bin/claude"),
            home.join(".local/bin/claude"),
            PathBuf::from("/usr/local/bin/claude"),
            PathBuf::from("/opt/homebrew/bin/claude"),
        ];

        for candidate in &candidates {
            if candidate.exists() {
                return candidate.to_str().map(|s| s.to_string());
            }
        }

        // Try PATH
        which::which("claude")
            .ok()
            .and_then(|p| p.to_str().map(|s| s.to_string()))
    }

    /// Build command line arguments from config
    fn build_args(&self, config: &LaunchConfig) -> Vec<String> {
        let mut args = vec![
            "--output-format".to_string(),
            "stream-json".to_string(),
            "--include-partial-messages".to_string(),
            "--verbose".to_string(),
            "--print".to_string(),
        ];

        // Session resumption
        if let Some(ref session_id) = config.session_id {
            args.push("--resume".to_string());
            args.push(session_id.clone());

            if config.fork_session {
                args.push("--fork-session".to_string());
            }
        }

        // Model
        args.push("--model".to_string());
        args.push(config.model.as_str().to_string());

        // Max turns
        if let Some(max) = config.max_turns {
            args.push("--max-turns".to_string());
            args.push(max.to_string());
        }

        // System prompt
        if let Some(ref prompt) = config.system_prompt {
            args.push("--system-prompt".to_string());
            args.push(prompt.clone());
        }

        // Permission prompt tool - loopback takes precedence
        if config.loopback_enabled {
            args.push("--permission-prompt-tool".to_string());
            args.push("mcp__plexus__loopback_permit".to_string());
        } else if let Some(ref tool) = config.permission_prompt_tool {
            args.push("--permission-prompt-tool".to_string());
            args.push(tool.clone());
        }

        // Allowed tools
        if !config.allowed_tools.is_empty() {
            args.push("--allowedTools".to_string());
            args.push(config.allowed_tools.join(","));
        }

        // Disallowed tools
        if !config.disallowed_tools.is_empty() {
            args.push("--disallowedTools".to_string());
            args.push(config.disallowed_tools.join(","));
        }

        // Query must be last
        args.push("--".to_string());
        args.push(config.query.clone());

        args
    }

    /// Write MCP config to a temp file and return the path
    async fn write_mcp_config(&self, config: &Value) -> Result<String, String> {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("mcp-config-{}.json", uuid::Uuid::new_v4()));

        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize MCP config: {}", e))?;

        tokio::fs::write(&temp_path, json)
            .await
            .map_err(|e| format!("Failed to write MCP config: {}", e))?;

        Ok(temp_path.to_string_lossy().to_string())
    }

    /// Launch a Claude Code session and stream raw events
    pub async fn launch(
        &self,
        config: LaunchConfig,
    ) -> Pin<Box<dyn Stream<Item = RawClaudeEvent> + Send + 'static>> {
        let mut args = self.build_args(&config);
        let claude_path = self.claude_path.clone();
        let working_dir = config.working_dir.clone();
        let loopback_enabled = config.loopback_enabled;
        let loopback_session_id = config.loopback_session_id.clone();

        // Build MCP config - merge loopback config if enabled
        let mcp_config = if loopback_enabled {
            let plexus_url = std::env::var("PLEXUS_MCP_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:4445/mcp".to_string());

            let loopback_mcp = serde_json::json!({
                "mcpServers": {
                    "plexus": {
                        "type": "http",
                        "url": plexus_url
                    }
                }
            });

            // Merge with existing config if present
            match config.mcp_config {
                Some(existing) => {
                    // Merge mcpServers from both
                    let mut merged = existing.clone();
                    if let (Some(existing_servers), Some(loopback_servers)) = (
                        merged.get_mut("mcpServers"),
                        loopback_mcp.get("mcpServers")
                    ) {
                        if let (Some(existing_obj), Some(loopback_obj)) = (
                            existing_servers.as_object_mut(),
                            loopback_servers.as_object()
                        ) {
                            for (k, v) in loopback_obj {
                                existing_obj.insert(k.clone(), v.clone());
                            }
                        }
                    } else {
                        merged["mcpServers"] = loopback_mcp["mcpServers"].clone();
                    }
                    Some(merged)
                }
                None => Some(loopback_mcp)
            }
        } else {
            config.mcp_config.clone()
        };

        Box::pin(stream! {
            // Handle MCP config if present
            let mcp_path = if let Some(ref mcp) = mcp_config {
                match Self::write_mcp_config_sync(mcp) {
                    Ok(path) => {
                        // Insert MCP config args before the "--" separator
                        if let Some(pos) = args.iter().position(|a| a == "--") {
                            args.insert(pos, path.clone());
                            args.insert(pos, "--mcp-config".to_string());
                        }
                        Some(path)
                    }
                    Err(e) => {
                        yield RawClaudeEvent::Result {
                            subtype: Some("error".to_string()),
                            session_id: None,
                            cost_usd: None,
                            is_error: Some(true),
                            duration_ms: None,
                            num_turns: None,
                            result: None,
                            error: Some(e),
                        };
                        return;
                    }
                }
            } else {
                None
            };

            // Spawn Claude process via shell to ensure clean process context
            // This avoids any potential issues with nested Claude sessions
            fn shell_escape(s: &str) -> String {
                // Escape by wrapping in single quotes and escaping any single quotes
                format!("'{}'", s.replace("'", "'\\''"))
            }

            let shell_cmd = format!(
                "{} {}",
                shell_escape(&claude_path),
                args.iter()
                    .map(|a| shell_escape(a))
                    .collect::<Vec<_>>()
                    .join(" ")
            );

            // Debug: log the command being executed
            tracing::debug!(cmd = %shell_cmd, "Launching Claude Code");
            eprintln!("[DEBUG] Claude command: {}", shell_cmd);

            let mut cmd = Command::new("bash");
            cmd.args(&["-c", &shell_cmd])
                .current_dir(&working_dir)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(Stdio::null());

            // Set loopback session ID env var if loopback is enabled
            if loopback_enabled {
                if let Some(ref session_id) = loopback_session_id {
                    cmd.env("LOOPBACK_SESSION_ID", session_id);
                }
            }

            let mut child = match cmd.spawn() {
                Ok(c) => c,
                Err(e) => {
                    yield RawClaudeEvent::Result {
                        subtype: Some("error".to_string()),
                        session_id: None,
                        cost_usd: None,
                        is_error: Some(true),
                        duration_ms: None,
                        num_turns: None,
                        result: None,
                        error: Some(format!("Failed to spawn claude: {}", e)),
                    };
                    return;
                }
            };

            let stdout = child.stdout.take().expect("stdout");
            let mut reader = BufReader::with_capacity(10 * 1024 * 1024, stdout).lines(); // 10MB buffer

            // Stream events from stdout
            while let Ok(Some(line)) = reader.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }

                match serde_json::from_str::<RawClaudeEvent>(&line) {
                    Ok(event) => {
                        let is_result = matches!(event, RawClaudeEvent::Result { .. });
                        yield event;
                        if is_result {
                            break;
                        }
                    }
                    Err(_) => {
                        // Try to parse as generic JSON and wrap as Unknown event
                        if let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) {
                            let event_type = value.get("type")
                                .and_then(|t| t.as_str())
                                .unwrap_or("unknown_json")
                                .to_string();
                            yield RawClaudeEvent::Unknown {
                                event_type,
                                data: value,
                            };
                        } else {
                            // Non-JSON output (raw text, errors, etc.)
                            yield RawClaudeEvent::Unknown {
                                event_type: "raw_output".to_string(),
                                data: serde_json::Value::String(line),
                            };
                        }
                    }
                }
            }

            // Cleanup
            let _ = child.wait().await;
            if let Some(path) = mcp_path {
                let _ = tokio::fs::remove_file(path).await;
            }
        })
    }

    /// Sync version of write_mcp_config for use in async stream
    fn write_mcp_config_sync(config: &Value) -> Result<String, String> {
        let temp_dir = std::env::temp_dir();
        let temp_path = temp_dir.join(format!("mcp-config-{}.json", uuid::Uuid::new_v4()));

        let json = serde_json::to_string_pretty(config)
            .map_err(|e| format!("Failed to serialize MCP config: {}", e))?;

        std::fs::write(&temp_path, json)
            .map_err(|e| format!("Failed to write MCP config: {}", e))?;

        Ok(temp_path.to_string_lossy().to_string())
    }
}

impl Default for ClaudeCodeExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_args_basic() {
        let executor = ClaudeCodeExecutor::with_path("/usr/bin/claude".to_string());
        let config = LaunchConfig {
            query: "hello".to_string(),
            model: Model::Sonnet,
            working_dir: "/tmp".to_string(),
            ..Default::default()
        };

        let args = executor.build_args(&config);

        assert!(args.contains(&"--output-format".to_string()));
        assert!(args.contains(&"stream-json".to_string()));
        assert!(args.contains(&"--model".to_string()));
        assert!(args.contains(&"sonnet".to_string()));
        assert!(args.contains(&"--".to_string()));
        assert!(args.contains(&"hello".to_string()));
    }

    #[test]
    fn test_build_args_with_resume() {
        let executor = ClaudeCodeExecutor::with_path("/usr/bin/claude".to_string());
        let config = LaunchConfig {
            query: "continue".to_string(),
            session_id: Some("sess_123".to_string()),
            model: Model::Haiku,
            working_dir: "/tmp".to_string(),
            ..Default::default()
        };

        let args = executor.build_args(&config);

        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"sess_123".to_string()));
        assert!(args.contains(&"haiku".to_string()));
    }

    #[test]
    fn test_build_args_with_fork() {
        let executor = ClaudeCodeExecutor::with_path("/usr/bin/claude".to_string());
        let config = LaunchConfig {
            query: "branch".to_string(),
            session_id: Some("sess_123".to_string()),
            fork_session: true,
            model: Model::Opus,
            working_dir: "/tmp".to_string(),
            ..Default::default()
        };

        let args = executor.build_args(&config);

        assert!(args.contains(&"--resume".to_string()));
        assert!(args.contains(&"--fork-session".to_string()));
        assert!(args.contains(&"opus".to_string()));
    }
}
