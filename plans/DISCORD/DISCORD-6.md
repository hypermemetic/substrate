# DISCORD-6: Role Management

**Status:** Not started
**blocked_by:** [DISCORD-2, DISCORD-3, DISCORD-4]
**unlocks:** [DISCORD-7]

## Scope

Implement role management operations within a guild. Supports creating, listing, updating, and deleting roles. Individual roles accessed via: `discord.guilds.{guild_id}.roles.{role_id}`.

Roles control permissions and are assigned to members (DISCORD-7 will handle assignment).

## Acceptance Criteria

- [ ] `RolesActivation` hub at `discord.guilds.{guild_id}.roles`
- [ ] Create roles with name, color, permissions, and other properties
- [ ] List all roles in the guild
- [ ] Get individual role info via `roles.{role_id}.info`
- [ ] Update role properties (name, color, permissions, position, hoist, mentionable)
- [ ] Delete roles
- [ ] Implements `ChildRouter` for integration with DISCORD-4
- [ ] CLI commands functional

## Discord API Endpoints

```
POST   /guilds/{guild_id}/roles          # Create role
GET    /guilds/{guild_id}/roles          # List roles
PATCH  /guilds/{guild_id}/roles/{id}     # Update role
DELETE /guilds/{guild_id}/roles/{id}     # Delete role
PATCH  /guilds/{guild_id}/roles          # Modify role positions
```

## Implementation Details

### RolesActivation (Hub)

```rust
// guilds/roles/activation.rs

pub struct RolesActivation {
    guild_id: String,
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl RolesActivation {
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
        // Children are dynamic (individual roles)
        vec![]
    }
}

#[hub_macro::hub_methods(
    namespace = "roles",
    version = "1.0.0",
    description = "Role management for a guild",
    hub
)]
impl RolesActivation {
    /// Create a new role
    #[hub_macro::hub_method(
        description = "Create a new role in the guild",
        params(
            name = "Role name",
            color = "Role color as RGB integer (e.g., 0xFF0000 for red)",
            permissions = "Permission bitfield as string",
            hoist = "Display role separately in member list (optional)",
            mentionable = "Allow anyone to mention this role (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn create(
        &self,
        name: String,
        color: Option<u32>,
        permissions: Option<String>,
        hoist: Option<bool>,
        mentionable: Option<bool>,
        token_name: Option<String>,
    ) -> impl Stream<Item = RoleEvent> + Send + 'static {
        // Implementation: POST /guilds/{guild_id}/roles
        // Parse permissions string to u64
        // Default color to 0 (no color)
        // Return RoleEvent::Created with role data
    }

    /// List all roles in the guild
    #[hub_macro::hub_method(
        description = "List all roles in the guild",
        params(token_name = "Token to use (optional)")
    )]
    async fn list(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = RoleEvent> + Send + 'static {
        // Implementation: GET /guilds/{guild_id}/roles
        // Yield RoleEvent::Listed with Vec<RoleInfo>
    }

    /// Reorder roles
    #[hub_macro::hub_method(
        description = "Update role positions",
        params(
            positions = "JSON array of {id, position} objects",
            token_name = "Token to use (optional)"
        )
    )]
    async fn reorder(
        &self,
        positions: String,
        token_name: Option<String>,
    ) -> impl Stream<Item = RoleEvent> + Send + 'static {
        // Implementation: PATCH /guilds/{guild_id}/roles
        // Parse positions JSON
        // Yield RoleEvent::Reordered
    }
}

#[async_trait]
impl ChildRouter for RolesActivation {
    fn router_namespace(&self) -> &str {
        "roles"
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        // name is a role_id (snowflake)
        if name.chars().all(|c| c.is_numeric()) && !name.is_empty() {
            Some(Box::new(RoleActivation::new(
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

### RoleActivation (Individual Role)

```rust
// guilds/roles/role.rs

pub struct RoleActivation {
    guild_id: String,
    role_id: String,
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl RoleActivation {
    pub fn new(
        guild_id: String,
        role_id: String,
        client: Arc<DiscordClient>,
        token_manager: Arc<TokenManager>,
    ) -> Self {
        Self {
            guild_id,
            role_id,
            client,
            token_manager,
        }
    }

    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        vec![]
    }
}

#[hub_macro::hub_methods(
    namespace_fn = "dynamic_namespace",
    version = "1.0.0",
    description = "Operations on a specific role"
)]
impl RoleActivation {
    pub fn dynamic_namespace(&self) -> String {
        self.role_id.clone()
    }

    /// Get role information
    #[hub_macro::hub_method(
        description = "Get detailed role information (fetched from guild roles list)",
        params(token_name = "Token to use (optional)")
    )]
    async fn info(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = RoleEvent> + Send + 'static {
        // Implementation: GET /guilds/{guild_id}/roles
        // Filter for this role_id
        // Yield RoleEvent::Info
    }

    /// Update role properties
    #[hub_macro::hub_method(
        description = "Update role configuration",
        params(
            name = "New role name (optional)",
            color = "New color (optional)",
            permissions = "New permissions bitfield (optional)",
            hoist = "Display separately (optional)",
            mentionable = "Allow mentioning (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn update(
        &self,
        name: Option<String>,
        color: Option<u32>,
        permissions: Option<String>,
        hoist: Option<bool>,
        mentionable: Option<bool>,
        token_name: Option<String>,
    ) -> impl Stream<Item = RoleEvent> + Send + 'static {
        // Implementation: PATCH /guilds/{guild_id}/roles/{role_id}
        // Parse permissions if provided
        // Yield RoleEvent::Updated
    }

    /// Delete the role
    #[hub_macro::hub_method(
        description = "Delete this role",
        params(token_name = "Token to use (optional)")
    )]
    async fn delete(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = RoleEvent> + Send + 'static {
        // Implementation: DELETE /guilds/{guild_id}/roles/{role_id}
        // Yield RoleEvent::Deleted
    }
}

#[async_trait]
impl ChildRouter for RoleActivation {
    fn router_namespace(&self) -> &str {
        &self.role_id
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, _name: &str) -> Option<Box<dyn ChildRouter>> {
        None // Roles have no children
    }
}
```

### Event Types

```rust
// guilds/roles/types.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RoleEvent {
    /// Role created
    Created {
        guild_id: String,
        role_id: String,
        name: String,
        data: serde_json::Value,
    },

    /// Roles listed
    Listed {
        guild_id: String,
        roles: Vec<RoleInfo>,
    },

    /// Role info retrieved
    Info {
        role_id: String,
        data: serde_json::Value,
    },

    /// Role updated
    Updated {
        role_id: String,
        data: serde_json::Value,
    },

    /// Role deleted
    Deleted {
        role_id: String,
    },

    /// Roles reordered
    Reordered {
        guild_id: String,
        count: usize,
    },

    /// Error occurred
    Error {
        role_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct RoleInfo {
    pub id: String,
    pub name: String,
    pub color: u32,
    pub permissions: String,
    pub position: u32,
    pub hoist: bool,
    pub mentionable: bool,
}
```

## Discord Permissions

```rust
// Permission constants for reference
pub mod permissions {
    pub const CREATE_INSTANT_INVITE: u64 = 1 << 0;
    pub const KICK_MEMBERS: u64 = 1 << 1;
    pub const BAN_MEMBERS: u64 = 1 << 2;
    pub const ADMINISTRATOR: u64 = 1 << 3;
    pub const MANAGE_CHANNELS: u64 = 1 << 4;
    pub const MANAGE_GUILD: u64 = 1 << 5;
    pub const ADD_REACTIONS: u64 = 1 << 6;
    pub const VIEW_AUDIT_LOG: u64 = 1 << 7;
    pub const PRIORITY_SPEAKER: u64 = 1 << 8;
    pub const STREAM: u64 = 1 << 9;
    pub const VIEW_CHANNEL: u64 = 1 << 10;
    pub const SEND_MESSAGES: u64 = 1 << 11;
    pub const SEND_TTS_MESSAGES: u64 = 1 << 12;
    pub const MANAGE_MESSAGES: u64 = 1 << 13;
    pub const EMBED_LINKS: u64 = 1 << 14;
    pub const ATTACH_FILES: u64 = 1 << 15;
    pub const READ_MESSAGE_HISTORY: u64 = 1 << 16;
    pub const MENTION_EVERYONE: u64 = 1 << 17;
    pub const USE_EXTERNAL_EMOJIS: u64 = 1 << 18;
    pub const VIEW_GUILD_INSIGHTS: u64 = 1 << 19;
    pub const CONNECT: u64 = 1 << 20;
    pub const SPEAK: u64 = 1 << 21;
    pub const MUTE_MEMBERS: u64 = 1 << 22;
    pub const DEAFEN_MEMBERS: u64 = 1 << 23;
    pub const MOVE_MEMBERS: u64 = 1 << 24;
    pub const USE_VAD: u64 = 1 << 25;
    pub const CHANGE_NICKNAME: u64 = 1 << 26;
    pub const MANAGE_NICKNAMES: u64 = 1 << 27;
    pub const MANAGE_ROLES: u64 = 1 << 28;
    pub const MANAGE_WEBHOOKS: u64 = 1 << 29;
    pub const MANAGE_EMOJIS_AND_STICKERS: u64 = 1 << 30;
    pub const USE_APPLICATION_COMMANDS: u64 = 1 << 31;
    pub const REQUEST_TO_SPEAK: u64 = 1 << 32;
    pub const MANAGE_EVENTS: u64 = 1 << 33;
    pub const MANAGE_THREADS: u64 = 1 << 34;
    pub const CREATE_PUBLIC_THREADS: u64 = 1 << 35;
    pub const CREATE_PRIVATE_THREADS: u64 = 1 << 36;
    pub const USE_EXTERNAL_STICKERS: u64 = 1 << 37;
    pub const SEND_MESSAGES_IN_THREADS: u64 = 1 << 38;
    pub const USE_EMBEDDED_ACTIVITIES: u64 = 1 << 39;
    pub const MODERATE_MEMBERS: u64 = 1 << 40;
}

/// Helper to parse permission string to u64
pub fn parse_permissions(s: &str) -> Result<u64, String> {
    s.parse::<u64>()
        .map_err(|e| format!("Invalid permission value: {}", e))
}

/// Helper to format permissions as string
pub fn format_permissions(perms: u64) -> String {
    perms.to_string()
}
```

## CLI Usage Examples

```bash
# Create a moderator role
synapse discord guilds 123456789012345678 roles create \
  --name "Moderator" \
  --color 0xFF0000 \
  --permissions "268435463" \
  --hoist true \
  --mentionable true

# List all roles
synapse discord guilds 123456789012345678 roles list

# Get role info
synapse discord guilds 123456789012345678 roles 111111111111111111 info

# Update role
synapse discord guilds 123456789012345678 roles 111111111111111111 update \
  --name "Super Moderator" \
  --color 0x00FF00

# Delete role
synapse discord guilds 123456789012345678 roles 111111111111111111 delete

# Reorder roles
synapse discord guilds 123456789012345678 roles reorder \
  --positions '[{"id":"111111111111111111","position":5},{"id":"222222222222222222","position":4}]'
```

## Integration Requirements

### For DISCORD-4 (Guild):
Must call `RolesActivation::new(guild_id, client, token_manager)` in `get_child("roles")`

### For DISCORD-7 (Members):
- Members will reference roles by ID
- Member activation will assign/remove roles
- Role info is fetched via guild roles endpoint

## Testing Strategy

1. **Unit Tests:**
   - Permission bitfield parsing
   - Color value validation
   - Role position logic

2. **Integration Tests:**
   - Create roles with various permissions
   - List and retrieve roles
   - Update role properties
   - Delete roles
   - Reorder roles

3. **Error Cases:**
   - Invalid permission values
   - Role hierarchy violations (can't modify higher roles)
   - Missing MANAGE_ROLES permission
   - Role not found

## Notes

- Role positions matter - higher position = more power
- Cannot modify roles above your highest role
- @everyone role exists by default (ID = guild_id)
- Administrator permission grants all permissions
- Permission bitfield is stored as u64 but sent as string in JSON
- Color is 24-bit RGB integer (0xRRGGBB)
