# MCP-9: Stream Buffering Utilities

## Metadata
- **blocked_by:** [MCP-2]
- **unlocks:** [MCP-10]
- **priority:** High (can start early, parallel with lifecycle)

## Scope

Create utilities to buffer Plexus streaming events into MCP's single-response format.

## Implementation

```rust
// src/mcp/buffer.rs

use futures::StreamExt;

#[derive(Debug, Serialize)]
pub struct McpContent {
    #[serde(rename = "type")]
    pub content_type: String,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct ToolCallResult {
    pub content: Vec<McpContent>,
    #[serde(rename = "isError")]
    pub is_error: bool,
}

/// Buffer a Plexus stream into an MCP tools/call result
pub async fn buffer_plexus_stream<S, E>(stream: S) -> ToolCallResult
where
    S: Stream<Item = E> + Unpin,
    E: PlexusEvent,
{
    let mut contents = Vec::new();
    let mut is_error = false;
    let mut text_buffer = String::new();

    pin_mut!(stream);
    while let Some(event) = stream.next().await {
        match event.event_type() {
            // Accumulate text content
            EventType::Content => {
                if let Some(text) = event.text() {
                    text_buffer.push_str(&text);
                }
            }

            // Capture tool invocations as text
            EventType::ToolUse => {
                if let Some(tool_info) = event.tool_info() {
                    contents.push(McpContent {
                        content_type: "text".into(),
                        text: format!("[Tool: {}] {}", tool_info.name, tool_info.input),
                    });
                }
            }

            // Capture errors
            EventType::Error => {
                is_error = true;
                if let Some(msg) = event.error_message() {
                    contents.push(McpContent {
                        content_type: "text".into(),
                        text: msg,
                    });
                }
            }

            // Terminal events
            EventType::Complete | EventType::Done => break,

            // Skip lifecycle events
            _ => {}
        }
    }

    // Prepend accumulated text as first content block
    if !text_buffer.is_empty() {
        contents.insert(0, McpContent {
            content_type: "text".into(),
            text: text_buffer,
        });
    }

    // Ensure at least one content block
    if contents.is_empty() {
        contents.push(McpContent {
            content_type: "text".into(),
            text: "".into(),
        });
    }

    ToolCallResult { content: contents, is_error }
}

/// Trait for extracting info from various Plexus event types
pub trait PlexusEvent {
    fn event_type(&self) -> EventType;
    fn text(&self) -> Option<String>;
    fn error_message(&self) -> Option<String>;
    fn tool_info(&self) -> Option<ToolInfo>;
}

pub enum EventType {
    Start,
    Content,
    ToolUse,
    ToolResult,
    Error,
    Complete,
    Done,
    Unknown,
}
```

## Files to Create/Modify

- Create `src/mcp/buffer.rs`
- Update `src/mcp/mod.rs`

## Acceptance Criteria

- [ ] Buffers stream events into single `ToolCallResult`
- [ ] Accumulates text content into single block
- [ ] Captures tool usage as text descriptions
- [ ] Sets `isError: true` on error events
- [ ] Works with any Plexus activation's event stream
- [ ] Unit tests with mock streams
