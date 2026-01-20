# DISCORD-5: Channel Management

**Status:** Not started
**blocked_by:** [DISCORD-2, DISCORD-3, DISCORD-4]
**unlocks:** [DISCORD-9]

## Scope

Implement channel management operations within a guild. Supports creating, listing, updating, and deleting channels. Individual channels are accessed via nested routing: `discord.guilds.{guild_id}.channels.{channel_id}`.

## Acceptance Criteria

- [ ] `ChannelsActivation` hub at `discord.guilds.{guild_id}.channels`
- [ ] Create text, voice, announcement, and category channels
- [ ] List all channels in the guild
- [ ] Get individual channel info via `channels.{channel_id}.info`
- [ ] Update channel properties (name, topic, position, permissions)
- [ ] Delete channels
- [ ] Implements `ChildRouter` for integration with DISCORD-4
- [ ] CLI commands functional

## Discord API Endpoints

```
POST   /guilds/{guild_id}/channels              # Create channel
GET    /guilds/{guild_id}/channels              # List channels
GET    /channels/{channel_id}                   # Get channel
PATCH  /channels/{channel_id}                   # Update channel
DELETE /channels/{channel_id}                   # Delete channel
PUT    /channels/{channel_id}/permissions/{id}  # Edit permissions
DELETE /channels/{channel_id}/permissions/{id}  # Delete permissions
```

## Implementation Details

### ChannelsActivation (Hub)

```rust
// guilds/channels/activation.rs

pub struct ChannelsActivation {
    guild_id: String,
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl ChannelsActivation {
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

    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        // Children are dynamic (individual channels)
        vec![]
    }
}

#[hub_macro::hub_methods(
    namespace = "channels",
    version = "1.0.0",
    description = "Channel management for a guild",
    hub
)]
impl ChannelsActivation {
    /// Create a new channel
    #[hub_macro::hub_method(
        description = "Create a new channel in the guild",
        params(
            name = "Channel name",
            channel_type = "Channel type: text (0), voice (2), category (4), announcement (5)",
            topic = "Channel topic (optional, text channels only)",
            parent_id = "Category ID to place channel under (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn create(
        &self,
        name: String,
        channel_type: Option<u8>,
        topic: Option<String>,
        parent_id: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = ChannelEvent> + Send + 'static {
        // Implementation: POST /guilds/{guild_id}/channels
        // Default channel_type to 0 (text)
        // Return ChannelEvent::Created with channel data
    }

    /// List all channels in the guild
    #[hub_macro::hub_method(
        description = "List all channels in the guild",
        params(token_name = "Token to use (optional)")
    )]
    async fn list(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = ChannelEvent> + Send + 'static {
        // Implementation: GET /guilds/{guild_id}/channels
        // Yield ChannelEvent::Listed with Vec<ChannelInfo>
    }
}

#[async_trait]
impl ChildRouter for ChannelsActivation {
    fn router_namespace(&self) -> &str {
        "channels"
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        // name is a channel_id (snowflake)
        if name.chars().all(|c| c.is_numeric()) && !name.is_empty() {
            Some(Box::new(ChannelActivation::new(
                self.guild_id.clone(),
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

### ChannelActivation (Individual Channel)

```rust
// guilds/channels/channel.rs

pub struct ChannelActivation {
    guild_id: String,
    channel_id: String,
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl ChannelActivation {
    pub fn new(
        guild_id: String,
        channel_id: String,
        client: Arc<DiscordClient>,
        token_manager: Arc<TokenManager>,
    ) -> Self {
        Self {
            guild_id,
            channel_id,
            client,
            token_manager,
        }
    }

    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        vec![
            ChildSummary {
                namespace: "messages".into(),
                description: "Message operations (DISCORD-9)".into(),
                hash: "messages".into(),
            },
            ChildSummary {
                namespace: "permissions".into(),
                description: "Permission overrides".into(),
                hash: "permissions".into(),
            },
        ]
    }
}

#[hub_macro::hub_methods(
    namespace_fn = "dynamic_namespace",
    version = "1.0.0",
    description = "Operations on a specific channel",
    hub
)]
impl ChannelActivation {
    pub fn dynamic_namespace(&self) -> String {
        self.channel_id.clone()
    }

    /// Get channel information
    #[hub_macro::hub_method(
        description = "Get detailed channel information",
        params(token_name = "Token to use (optional)")
    )]
    async fn info(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = ChannelEvent> + Send + 'static {
        // Implementation: GET /channels/{channel_id}
        // Yield ChannelEvent::Info
    }

    /// Update channel properties
    #[hub_macro::hub_method(
        description = "Update channel configuration",
        params(
            name = "New channel name (optional)",
            topic = "New topic (optional)",
            position = "New position (optional)",
            nsfw = "Mark as NSFW (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn update(
        &self,
        name: Option<String>,
        topic: Option<String>,
        position: Option<u32>,
        nsfw: Option<bool>,
        token_name: Option<String>,
    ) -> impl Stream<Item = ChannelEvent> + Send + 'static {
        // Implementation: PATCH /channels/{channel_id}
        // Yield ChannelEvent::Updated
    }

    /// Delete the channel
    #[hub_macro::hub_method(
        description = "Delete this channel",
        params(token_name = "Token to use (optional)")
    )]
    async fn delete(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = ChannelEvent> + Send + 'static {
        // Implementation: DELETE /channels/{channel_id}
        // Yield ChannelEvent::Deleted
    }

    /// Set permission override for a role or user
    #[hub_macro::hub_method(
        description = "Set permission override",
        params(
            target_id = "Role or user ID to override permissions for",
            target_type = "Type: 'role' or 'member'",
            allow = "Permissions to allow (bitfield)",
            deny = "Permissions to deny (bitfield)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn set_permission(
        &self,
        target_id: String,
        target_type: String,
        allow: Option<String>,
        deny: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = ChannelEvent> + Send + 'static {
        // Implementation: PUT /channels/{channel_id}/permissions/{target_id}
        // Yield ChannelEvent::PermissionSet
    }
}

#[async_trait]
impl ChildRouter for ChannelActivation {
    fn router_namespace(&self) -> &str {
        &self.channel_id
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        match name {
            "messages" => {
                // DISCORD-9 will implement this
                Some(Box::new(MessagesActivation::new(
                    self.channel_id.clone(),
                    self.client.clone(),
                    self.token_manager.clone(),
                )))
            }
            "permissions" => {
                // Could be implemented here or in future ticket
                None
            }
            _ => None,
        }
    }
}
```

### Event Types

```rust
// guilds/channels/types.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChannelEvent {
    /// Channel created
    Created {
        guild_id: String,
        channel_id: String,
        name: String,
        channel_type: u8,
        data: serde_json::Value,
    },

    /// Channels listed
    Listed {
        guild_id: String,
        channels: Vec<ChannelInfo>,
    },

    /// Channel info retrieved
    Info {
        channel_id: String,
        data: serde_json::Value,
    },

    /// Channel updated
    Updated {
        channel_id: String,
        data: serde_json::Value,
    },

    /// Channel deleted
    Deleted {
        channel_id: String,
    },

    /// Permission override set
    PermissionSet {
        channel_id: String,
        target_id: String,
    },

    /// Error occurred
    Error {
        channel_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ChannelInfo {
    pub id: String,
    pub name: String,
    pub r#type: u8,
    pub position: u32,
    pub parent_id: Option<String>,
    pub topic: Option<String>,
}
```

## Discord Channel Types

```rust
// Channel type constants for reference
pub mod channel_types {
    pub const TEXT: u8 = 0;
    pub const VOICE: u8 = 2;
    pub const CATEGORY: u8 = 4;
    pub const ANNOUNCEMENT: u8 = 5;
    pub const ANNOUNCEMENT_THREAD: u8 = 10;
    pub const PUBLIC_THREAD: u8 = 11;
    pub const PRIVATE_THREAD: u8 = 12;
    pub const STAGE_VOICE: u8 = 13;
    pub const FORUM: u8 = 15;
}
```

## CLI Usage Examples

```bash
# Create a text channel
synapse discord guilds 123456789012345678 channels create \
  --name "general" \
  --channel-type 0

# Create a voice channel
synapse discord guilds 123456789012345678 channels create \
  --name "Voice Chat" \
  --channel-type 2

# List all channels
synapse discord guilds 123456789012345678 channels list

# Get channel info
synapse discord guilds 123456789012345678 channels 987654321098765432 info

# Update channel
synapse discord guilds 123456789012345678 channels 987654321098765432 update \
  --name "new-channel-name" \
  --topic "A new topic"

# Delete channel
synapse discord guilds 123456789012345678 channels 987654321098765432 delete

# Set permission override
synapse discord guilds 123456789012345678 channels 987654321098765432 set-permission \
  --target-id 111111111111111111 \
  --target-type role \
  --allow "2048" \
  --deny "8"
```

## Integration Requirements

### For DISCORD-4 (Guild):
Must call `ChannelsActivation::new(guild_id, client, token_manager)` in `get_child("channels")`

### For DISCORD-9 (Messages):
- `MessagesActivation::new(channel_id, client, token_manager)` constructor
- Implements `ChildRouter`
- Handles message CRUD operations

## Testing Strategy

1. **Unit Tests:**
   - Channel type validation
   - Permission bitfield parsing
   - Event serialization

2. **Integration Tests:**
   - Create all channel types
   - List and retrieve channels
   - Update channel properties
   - Delete channels
   - Permission overrides

3. **Error Cases:**
   - Invalid channel type
   - Missing permissions
   - Channel not found
   - Invalid permission values

## Notes

- Channels can be nested in categories (parent_id)
- Permission overrides are complex - start with basic allow/deny
- Thread channels (types 10-12) may need special handling
- Forum channels (type 15) have different structure
