# DISCORD-4: Guild Activation (Dynamic Hub)

**Status:** Not started
**blocked_by:** [DISCORD-2, DISCORD-3]
**unlocks:** [DISCORD-5, DISCORD-6, DISCORD-7]

## Scope

Implement dynamic guild activations following the solar system pattern. Each guild is accessed via `discord.guilds.{guild_id}` and acts as a hub for channels, roles, members, etc.

This is the **critical integration point** - guild activation must support nested routing to all child resources.

## Acceptance Criteria

- [ ] Guild accessed dynamically via snowflake ID: `discord.guilds.{guild_id}`
- [ ] Guild implements `ChildRouter` for nested resources
- [ ] Methods to get guild information and configuration
- [ ] Methods to update guild settings (name, icon, features)
- [ ] Guild has child summaries for: channels, roles, members, webhooks, emojis, events
- [ ] Token retrieved from hyperforge via DISCORD-3 auth system
- [ ] Error handling for invalid guild ID or missing bot permissions
- [ ] CLI: `synapse discord guilds <guild_id> info`

## Implementation Details

### Guild Activation Structure

```rust
// guilds/activation.rs

use super::types::GuildEvent;
use crate::client::DiscordClient;
use crate::auth::TokenManager;

/// Dynamic activation for a specific guild
pub struct GuildActivation {
    guild_id: String,
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl GuildActivation {
    pub fn new(
        guild_id: String,
        client: Arc<DiscordClient>,
        token_manager: Arc<TokenManager>,
    ) -> Self {
        Self {
            guild_id,
            client,
            token_manager,
        }
    }

    /// Get bot token (from default or specified token name)
    async fn get_token(&self, token_name: Option<&str>) -> Result<String, PlexusError> {
        self.token_manager.get_token(token_name).await
    }

    /// Child summaries for schema generation
    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        vec![
            ChildSummary {
                namespace: "channels".into(),
                description: "Channel management".into(),
                hash: "channels".into(),
            },
            ChildSummary {
                namespace: "roles".into(),
                description: "Role management".into(),
                hash: "roles".into(),
            },
            ChildSummary {
                namespace: "members".into(),
                description: "Member management".into(),
                hash: "members".into(),
            },
            ChildSummary {
                namespace: "webhooks".into(),
                description: "Webhook management".into(),
                hash: "webhooks".into(),
            },
            ChildSummary {
                namespace: "emojis".into(),
                description: "Custom emoji management".into(),
                hash: "emojis".into(),
            },
            ChildSummary {
                namespace: "events".into(),
                description: "Scheduled events".into(),
                hash: "events".into(),
            },
        ]
    }
}
```

### Guild Methods

```rust
#[hub_macro::hub_methods(
    namespace_fn = "dynamic_namespace",  // Use guild_id as namespace
    version = "1.0.0",
    description = "Guild-specific operations",
    hub  // This is a hub with child resources
)]
impl GuildActivation {
    /// Dynamic namespace based on guild_id
    pub fn dynamic_namespace(&self) -> String {
        self.guild_id.clone()
    }

    /// Get guild information
    #[hub_macro::hub_method(
        description = "Get detailed guild information",
        params(token_name = "Token to use (optional, uses default)")
    )]
    async fn info(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = GuildEvent> + Send + 'static {
        let guild_id = self.guild_id.clone();
        let client = self.client.clone();
        let token_manager = self.token_manager.clone();

        stream! {
            match token_manager.get_token(token_name.as_deref()).await {
                Ok(token) => {
                    let endpoint = format!("/guilds/{}", guild_id);
                    match client.get(&token, &endpoint).await {
                        Ok(response) => {
                            match response.json::<serde_json::Value>().await {
                                Ok(guild_data) => {
                                    yield GuildEvent::Info {
                                        id: guild_id,
                                        data: guild_data,
                                    };
                                }
                                Err(e) => {
                                    yield GuildEvent::Error {
                                        guild_id,
                                        message: format!("Failed to parse guild data: {}", e),
                                    };
                                }
                            }
                        }
                        Err(e) => {
                            yield GuildEvent::Error {
                                guild_id,
                                message: e.to_string(),
                            };
                        }
                    }
                }
                Err(e) => {
                    yield GuildEvent::Error {
                        guild_id,
                        message: e.to_string(),
                    };
                }
            }
        }
    }

    /// Update guild settings
    #[hub_macro::hub_method(
        description = "Update guild configuration",
        params(
            name = "New guild name (optional)",
            icon = "New icon URL or base64 data (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn update(
        &self,
        name: Option<String>,
        icon: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = GuildEvent> + Send + 'static {
        let guild_id = self.guild_id.clone();
        let client = self.client.clone();
        let token_manager = self.token_manager.clone();

        stream! {
            match token_manager.get_token(token_name.as_deref()).await {
                Ok(token) => {
                    let mut body = serde_json::Map::new();
                    if let Some(n) = name {
                        body.insert("name".into(), serde_json::Value::String(n));
                    }
                    if let Some(i) = icon {
                        body.insert("icon".into(), serde_json::Value::String(i));
                    }

                    let endpoint = format!("/guilds/{}", guild_id);
                    match client.patch(&token, &endpoint, serde_json::Value::Object(body)).await {
                        Ok(response) => {
                            match response.json::<serde_json::Value>().await {
                                Ok(updated_guild) => {
                                    yield GuildEvent::Updated {
                                        guild_id,
                                        data: updated_guild,
                                    };
                                }
                                Err(e) => {
                                    yield GuildEvent::Error {
                                        guild_id,
                                        message: format!("Failed to parse response: {}", e),
                                    };
                                }
                            }
                        }
                        Err(e) => {
                            yield GuildEvent::Error {
                                guild_id,
                                message: e.to_string(),
                            };
                        }
                    }
                }
                Err(e) => {
                    yield GuildEvent::Error {
                        guild_id,
                        message: e.to_string(),
                    };
                }
            }
        }
    }
}
```

### ChildRouter Implementation

```rust
#[async_trait]
impl ChildRouter for GuildActivation {
    fn router_namespace(&self) -> &str {
        &self.guild_id
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        // Delegate to Activation::call for local methods + nested routing
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        match name {
            "channels" => {
                // DISCORD-5 will implement this
                Some(Box::new(ChannelsActivation::new(
                    self.guild_id.clone(),
                    self.client.clone(),
                    self.token_manager.clone(),
                )))
            }
            "roles" => {
                // DISCORD-6 will implement this
                Some(Box::new(RolesActivation::new(
                    self.guild_id.clone(),
                    self.client.clone(),
                    self.token_manager.clone(),
                )))
            }
            "members" => {
                // DISCORD-7 will implement this
                Some(Box::new(MembersActivation::new(
                    self.guild_id.clone(),
                    self.client.clone(),
                    self.token_manager.clone(),
                )))
            }
            "webhooks" => {
                // DISCORD-8 will implement this
                Some(Box::new(WebhooksActivation::new(
                    self.guild_id.clone(),
                    self.client.clone(),
                    self.token_manager.clone(),
                )))
            }
            "emojis" => {
                // DISCORD-10 will implement this
                Some(Box::new(EmojisActivation::new(
                    self.guild_id.clone(),
                    self.client.clone(),
                    self.token_manager.clone(),
                )))
            }
            "events" => {
                // Future implementation
                None
            }
            _ => None,
        }
    }
}
```

### Root Discord Activation Updates

Update `discord/activation.rs` to support guild routing:

```rust
#[async_trait]
impl ChildRouter for Discord {
    fn router_namespace(&self) -> &str {
        "discord"
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        // Check if this is "guilds" to access guild sub-hub
        if name == "guilds" {
            Some(Box::new(GuildsHub::new(
                self.client.clone(),
                self.token_manager.clone(),
            )))
        } else {
            None
        }
    }
}
```

### Guilds Hub (Dynamic Router)

```rust
// guilds/hub.rs

/// Hub for dynamically accessing guilds by ID
pub struct GuildsHub {
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl GuildsHub {
    pub fn new(client: Arc<DiscordClient>, token_manager: Arc<TokenManager>) -> Self {
        Self { client, token_manager }
    }
}

#[async_trait]
impl ChildRouter for GuildsHub {
    fn router_namespace(&self) -> &str {
        "guilds"
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        // guilds hub has no methods of its own, only routes to guild IDs
        Err(PlexusError::MethodNotFound {
            activation: "guilds".to_string(),
            method: method.to_string(),
        })
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        // name is a guild_id (snowflake)
        // Validate it's a valid snowflake format (numeric string)
        if name.chars().all(|c| c.is_numeric()) && !name.is_empty() {
            Some(Box::new(GuildActivation::new(
                name.to_string(),
                self.client.clone(),
                self.token_manager.clone(),
            )))
        } else {
            None
        }
    }
}
```

### Event Types

```rust
// guilds/types.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum GuildEvent {
    /// Guild information retrieved
    Info {
        id: String,
        data: serde_json::Value,
    },

    /// Guild updated successfully
    Updated {
        guild_id: String,
        data: serde_json::Value,
    },

    /// Error occurred
    Error {
        guild_id: String,
        message: String,
    },
}
```

## CLI Usage Examples

```bash
# Get guild information
synapse discord guilds 123456789012345678 info

# Update guild name
synapse discord guilds 123456789012345678 update --name "New Server Name"

# Access nested resources (implemented in later tickets)
synapse discord guilds 123456789012345678 channels list
synapse discord guilds 123456789012345678 roles create --name "Moderator"
synapse discord guilds 123456789012345678 members 987654321098765432 info
```

## Integration Requirements

### For DISCORD-5 (Channels):
- `ChannelsActivation::new(guild_id, client, token_manager)` constructor
- Implements `ChildRouter`

### For DISCORD-6 (Roles):
- `RolesActivation::new(guild_id, client, token_manager)` constructor
- Implements `ChildRouter`

### For DISCORD-7 (Members):
- `MembersActivation::new(guild_id, client, token_manager)` constructor
- Implements `ChildRouter`

### For DISCORD-8 (Webhooks):
- `WebhooksActivation::new(guild_id, client, token_manager)` constructor
- Implements `ChildRouter`

### For DISCORD-10 (Emojis):
- `EmojisActivation::new(guild_id, client, token_manager)` constructor
- Implements `ChildRouter`

## Testing Strategy

1. **Unit Tests:**
   - Guild ID validation (numeric snowflakes only)
   - Dynamic namespace generation
   - Child router resolution

2. **Integration Tests:**
   - Get guild info with real Discord API
   - Update guild settings
   - Child routing to channels/roles/members

3. **Error Cases:**
   - Invalid guild ID format
   - Guild not found (bot not in server)
   - Missing permissions

## Notes

- Guild activation is the **hub point** for all server operations
- Must be generic enough to support all child resources
- Token management is centralized - children inherit from parent
- Guild ID is used as the dynamic namespace for routing
- This ticket should compile and work standalone, with stubs for child activations
