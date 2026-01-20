# DISCORD-2: Plugin Foundation & Discord API Client

**Status:** Not started
**blocked_by:** []
**unlocks:** [DISCORD-3, DISCORD-4, DISCORD-5, DISCORD-6, DISCORD-7, DISCORD-8, DISCORD-9, DISCORD-10]

## Scope

Build the foundational infrastructure for the Discord plugin:
1. HTTP client for Discord API
2. Rate limiting implementation
3. Error handling and mapping
4. Base types and structures
5. Root plugin activation

## Acceptance Criteria

- [ ] HTTP client configured with Discord base URL (`https://discord.com/api/v10`)
- [ ] User-Agent header properly set (`DiscordBot (plexus-discord, $version)`)
- [ ] Rate limiting middleware implemented (handles 429 responses)
- [ ] Discord error responses mapped to PlexusError
- [ ] Root Discord activation created with namespace "discord"
- [ ] Basic types defined (Snowflake, Permissions, Color)
- [ ] Unit tests for error handling and rate limiting

## Implementation Details

### Directory Structure

```
src/activations/discord/
├── mod.rs                    # Module exports
├── activation.rs             # Root Discord activation
├── types.rs                  # Common Discord types
├── client.rs                 # HTTP client wrapper
├── rate_limit.rs             # Rate limiting logic
├── error.rs                  # Error mapping
├── guilds/                   # Guild sub-plugin (future)
│   ├── mod.rs
│   └── activation.rs
└── README.md                 # Plugin documentation
```

### Core Types

```rust
// types.rs

/// Discord Snowflake ID
pub type Snowflake = String;

/// Discord permissions bitfield
pub type Permissions = u64;

/// RGB color value
pub type Color = u32;

/// Discord API version
pub const API_VERSION: u8 = 10;

/// Discord CDN base URL
pub const CDN_BASE: &str = "https://cdn.discordapp.com";

/// Root event enum for Discord operations
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum DiscordEvent {
    Status {
        message: String,
        connected: bool,
    },
    Error {
        message: String,
        code: Option<i32>,
    },
}
```

### HTTP Client

```rust
// client.rs

use reqwest::{Client, Response, StatusCode};
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

pub struct DiscordClient {
    client: Client,
    base_url: String,
    rate_limiter: RwLock<RateLimiter>,
}

impl DiscordClient {
    pub fn new() -> Self {
        let client = Client::builder()
            .user_agent("DiscordBot (plexus-discord, 1.0.0)")
            .timeout(Duration::from_secs(10))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            base_url: "https://discord.com/api/v10".to_string(),
            rate_limiter: RwLock::new(RateLimiter::new()),
        }
    }

    /// Make a GET request to Discord API
    pub async fn get(
        &self,
        token: &str,
        endpoint: &str,
    ) -> Result<Response, DiscordError> {
        self.request("GET", token, endpoint, None).await
    }

    /// Make a POST request to Discord API
    pub async fn post(
        &self,
        token: &str,
        endpoint: &str,
        body: serde_json::Value,
    ) -> Result<Response, DiscordError> {
        self.request("POST", token, endpoint, Some(body)).await
    }

    /// Make a PATCH request to Discord API
    pub async fn patch(
        &self,
        token: &str,
        endpoint: &str,
        body: serde_json::Value,
    ) -> Result<Response, DiscordError> {
        self.request("PATCH", token, endpoint, Some(body)).await
    }

    /// Make a DELETE request to Discord API
    pub async fn delete(
        &self,
        token: &str,
        endpoint: &str,
    ) -> Result<Response, DiscordError> {
        self.request("DELETE", token, endpoint, None).await
    }

    async fn request(
        &self,
        method: &str,
        token: &str,
        endpoint: &str,
        body: Option<serde_json::Value>,
    ) -> Result<Response, DiscordError> {
        // Wait for rate limit
        self.rate_limiter.write().await.wait().await;

        let url = format!("{}{}", self.base_url, endpoint);
        let mut req = match method {
            "GET" => self.client.get(&url),
            "POST" => self.client.post(&url),
            "PATCH" => self.client.patch(&url),
            "DELETE" => self.client.delete(&url),
            _ => return Err(DiscordError::InvalidMethod(method.to_string())),
        };

        req = req.header("Authorization", format!("Bot {}", token));

        if let Some(body) = body {
            req = req.json(&body);
        }

        let response = req.send().await?;

        // Update rate limiter from response headers
        self.rate_limiter.write().await.update_from_response(&response);

        // Handle rate limiting
        if response.status() == StatusCode::TOO_MANY_REQUESTS {
            let retry_after = response
                .headers()
                .get("Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|s| s.parse::<f64>().ok())
                .unwrap_or(1.0);

            tokio::time::sleep(Duration::from_secs_f64(retry_after)).await;

            // Retry the request
            return self.request(method, token, endpoint, body).await;
        }

        // Handle errors
        if !response.status().is_success() {
            return Err(DiscordError::from_response(response).await);
        }

        Ok(response)
    }
}
```

### Rate Limiting

```rust
// rate_limit.rs

use std::time::{Duration, Instant};
use reqwest::Response;

/// Simple token bucket rate limiter
pub struct RateLimiter {
    remaining: u32,
    reset_at: Option<Instant>,
    global_reset_at: Option<Instant>,
}

impl RateLimiter {
    pub fn new() -> Self {
        Self {
            remaining: 50, // Default conservative limit
            reset_at: None,
            global_reset_at: None,
        }
    }

    pub async fn wait(&mut self) {
        // Check global rate limit first
        if let Some(reset) = self.global_reset_at {
            if reset > Instant::now() {
                let wait_time = reset.duration_since(Instant::now());
                tokio::time::sleep(wait_time).await;
                self.global_reset_at = None;
            }
        }

        // Check bucket rate limit
        if let Some(reset) = self.reset_at {
            if self.remaining == 0 && reset > Instant::now() {
                let wait_time = reset.duration_since(Instant::now());
                tokio::time::sleep(wait_time).await;
            }
        }
    }

    pub fn update_from_response(&mut self, response: &Response) {
        let headers = response.headers();

        // Update remaining requests
        if let Some(remaining) = headers.get("X-RateLimit-Remaining") {
            if let Ok(s) = remaining.to_str() {
                if let Ok(n) = s.parse::<u32>() {
                    self.remaining = n;
                }
            }
        }

        // Update reset time
        if let Some(reset) = headers.get("X-RateLimit-Reset") {
            if let Ok(s) = reset.to_str() {
                if let Ok(timestamp) = s.parse::<f64>() {
                    let duration = Duration::from_secs_f64(timestamp - chrono::Utc::now().timestamp() as f64);
                    self.reset_at = Some(Instant::now() + duration);
                }
            }
        }

        // Check for global rate limit
        if let Some(global) = headers.get("X-RateLimit-Global") {
            if global.to_str().unwrap_or("false") == "true" {
                if let Some(retry_after) = headers.get("Retry-After") {
                    if let Ok(s) = retry_after.to_str() {
                        if let Ok(seconds) = s.parse::<f64>() {
                            self.global_reset_at = Some(Instant::now() + Duration::from_secs_f64(seconds));
                        }
                    }
                }
            }
        }
    }
}
```

### Error Handling

```rust
// error.rs

use serde::{Deserialize, Serialize};
use reqwest::Response;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordApiError {
    pub code: i32,
    pub message: String,
    pub errors: Option<serde_json::Value>,
}

#[derive(Debug, thiserror::Error)]
pub enum DiscordError {
    #[error("Discord API error {0}: {1}")]
    ApiError(i32, String),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("Invalid method: {0}")]
    InvalidMethod(String),

    #[error("Unauthorized: invalid bot token")]
    Unauthorized,

    #[error("Forbidden: missing permissions")]
    Forbidden,

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Rate limited")]
    RateLimited,
}

impl DiscordError {
    pub async fn from_response(response: Response) -> Self {
        let status = response.status();

        match status.as_u16() {
            401 => Self::Unauthorized,
            403 => Self::Forbidden,
            404 => Self::NotFound("Resource not found".to_string()),
            429 => Self::RateLimited,
            _ => {
                if let Ok(api_error) = response.json::<DiscordApiError>().await {
                    Self::ApiError(api_error.code, api_error.message)
                } else {
                    Self::ApiError(status.as_u16() as i32, status.to_string())
                }
            }
        }
    }

    pub fn to_plexus_error(self) -> crate::plexus::PlexusError {
        use crate::plexus::PlexusError;

        match self {
            Self::Unauthorized => PlexusError::ExecutionError(
                "Invalid Discord bot token. Please set a valid token in hyperforge.".into()
            ),
            Self::Forbidden => PlexusError::ExecutionError(
                "Missing permissions. Ensure your bot has the required permissions.".into()
            ),
            Self::NotFound(msg) => PlexusError::ExecutionError(msg),
            Self::ApiError(code, msg) => PlexusError::ExecutionError(
                format!("Discord API error {}: {}", code, msg)
            ),
            Self::RateLimited => PlexusError::ExecutionError(
                "Rate limited by Discord API".into()
            ),
            Self::Http(e) => PlexusError::ExecutionError(
                format!("HTTP error: {}", e)
            ),
            Self::InvalidMethod(m) => PlexusError::ExecutionError(
                format!("Invalid HTTP method: {}", m)
            ),
        }
    }
}
```

### Root Activation

```rust
// activation.rs

use super::types::DiscordEvent;
use async_stream::stream;
use futures::Stream;

#[derive(Clone)]
pub struct Discord {
    client: Arc<DiscordClient>,
}

impl Discord {
    pub fn new() -> Self {
        Self {
            client: Arc::new(DiscordClient::new()),
        }
    }

    pub fn client(&self) -> &DiscordClient {
        &self.client
    }
}

impl Default for Discord {
    fn default() -> Self {
        Self::new()
    }
}

#[hub_macro::hub_methods(
    namespace = "discord",
    version = "1.0.0",
    description = "Discord server management and API access",
    hub  // This is a hub with guild children
)]
impl Discord {
    /// Get plugin status and connection info
    #[hub_macro::hub_method(
        description = "Get Discord plugin status"
    )]
    async fn status(&self) -> impl Stream<Item = DiscordEvent> + Send + 'static {
        stream! {
            yield DiscordEvent::Status {
                message: "Discord plugin ready".to_string(),
                connected: true,
            };
        }
    }

    /// Get child plugin summaries (guilds)
    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        // Initially empty - guilds are dynamically accessed
        // Future: could list guilds from a configured bot token
        vec![]
    }
}
```

## Testing Strategy

1. **Unit Tests:**
   - Rate limiter behavior
   - Error mapping
   - Type serialization

2. **Integration Tests (manual):**
   - Client connection to Discord API
   - Error handling with invalid tokens
   - Rate limit handling

3. **Mock Tests:**
   - Use `wiremock` to simulate Discord API responses
   - Test all error codes
   - Test rate limiting scenarios

## Dependencies

Add to `Cargo.toml`:
```toml
[dependencies]
reqwest = { version = "0.11", features = ["json"] }
tokio = { version = "1", features = ["sync", "time"] }
thiserror = "1.0"
chrono = "0.4"

[dev-dependencies]
wiremock = "0.5"
```

## Notes

- This ticket focuses on infrastructure only
- No actual Discord operations yet (those come in DISCORD-4+)
- Rate limiting is conservative to avoid issues
- Client is designed to be shared across guild activations
