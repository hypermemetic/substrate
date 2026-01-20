# DISCORD-7: Member Management

**Status:** Not started
**blocked_by:** [DISCORD-4, DISCORD-6]
**unlocks:** []

## Scope

Implement member (user) management operations within a guild. Supports listing members, getting member info, updating member properties, assigning/removing roles, kicking, and banning. Individual members accessed via: `discord.guilds.{guild_id}.members.{user_id}`.

## Acceptance Criteria

- [ ] `MembersActivation` hub at `discord.guilds.{guild_id}.members`
- [ ] List guild members (with pagination support)
- [ ] Search members by username
- [ ] Get individual member info via `members.{user_id}.info`
- [ ] Update member properties (nickname, mute, deafen)
- [ ] Add/remove roles from members
- [ ] Kick members from guild
- [ ] Ban/unban members
- [ ] Implements `ChildRouter` for integration with DISCORD-4
- [ ] CLI commands functional

## Discord API Endpoints

```
GET    /guilds/{guild_id}/members                    # List members
GET    /guilds/{guild_id}/members/search             # Search members
GET    /guilds/{guild_id}/members/{user_id}          # Get member
PATCH  /guilds/{guild_id}/members/{user_id}          # Update member
PUT    /guilds/{guild_id}/members/{user_id}/roles/{role_id}  # Add role
DELETE /guilds/{guild_id}/members/{user_id}/roles/{role_id}  # Remove role
DELETE /guilds/{guild_id}/members/{user_id}          # Kick member
PUT    /guilds/{guild_id}/bans/{user_id}             # Ban member
DELETE /guilds/{guild_id}/bans/{user_id}             # Unban member
GET    /guilds/{guild_id}/bans                       # List bans
```

## Implementation Details

### MembersActivation (Hub)

```rust
// guilds/members/activation.rs

pub struct MembersActivation {
    guild_id: String,
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl MembersActivation {
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
        // Children are dynamic (individual members)
        vec![]
    }
}

#[hub_macro::hub_methods(
    namespace = "members",
    version = "1.0.0",
    description = "Member management for a guild",
    hub
)]
impl MembersActivation {
    /// List guild members
    #[hub_macro::hub_method(
        description = "List guild members",
        params(
            limit = "Max members to return (1-1000, default 100)",
            after = "User ID to paginate after (optional)",
            token_name = "Token to use (optional)"
        ),
        streaming  // May yield multiple events for pagination
    )]
    async fn list(
        &self,
        limit: Option<u32>,
        after: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: GET /guilds/{guild_id}/members?limit={limit}&after={after}
        // Support pagination via 'after' parameter
        // Yield MemberEvent::Listed with Vec<MemberInfo>
    }

    /// Search members by username or nickname
    #[hub_macro::hub_method(
        description = "Search members by username or nickname",
        params(
            query = "Search query string",
            limit = "Max results (1-1000, default 100)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn search(
        &self,
        query: String,
        limit: Option<u32>,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: GET /guilds/{guild_id}/members/search?query={query}&limit={limit}
        // Yield MemberEvent::SearchResults
    }

    /// List banned members
    #[hub_macro::hub_method(
        description = "List all banned members",
        params(token_name = "Token to use (optional)")
    )]
    async fn list_bans(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: GET /guilds/{guild_id}/bans
        // Yield MemberEvent::BanList
    }
}

#[async_trait]
impl ChildRouter for MembersActivation {
    fn router_namespace(&self) -> &str {
        "members"
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        // name is a user_id (snowflake)
        if name.chars().all(|c| c.is_numeric()) && !name.is_empty() {
            Some(Box::new(MemberActivation::new(
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

### MemberActivation (Individual Member)

```rust
// guilds/members/member.rs

pub struct MemberActivation {
    guild_id: String,
    user_id: String,
    client: Arc<DiscordClient>,
    token_manager: Arc<TokenManager>,
}

impl MemberActivation {
    pub fn new(
        guild_id: String,
        user_id: String,
        client: Arc<DiscordClient>,
        token_manager: Arc<TokenManager>,
    ) -> Self {
        Self {
            guild_id,
            user_id,
            client,
            token_manager,
        }
    }

    pub fn plugin_children(&self) -> Vec<ChildSummary> {
        vec![
            ChildSummary {
                namespace: "roles".into(),
                description: "Member role management".into(),
                hash: "roles".into(),
            },
        ]
    }
}

#[hub_macro::hub_methods(
    namespace_fn = "dynamic_namespace",
    version = "1.0.0",
    description = "Operations on a specific member",
    hub
)]
impl MemberActivation {
    pub fn dynamic_namespace(&self) -> String {
        self.user_id.clone()
    }

    /// Get member information
    #[hub_macro::hub_method(
        description = "Get detailed member information",
        params(token_name = "Token to use (optional)")
    )]
    async fn info(
        &self,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: GET /guilds/{guild_id}/members/{user_id}
        // Yield MemberEvent::Info
    }

    /// Update member properties
    #[hub_macro::hub_method(
        description = "Update member configuration",
        params(
            nickname = "New nickname (optional, empty string to remove)",
            mute = "Server mute status (optional)",
            deaf = "Server deafen status (optional)",
            channel_id = "Voice channel to move member to (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn update(
        &self,
        nickname: Option<String>,
        mute: Option<bool>,
        deaf: Option<bool>,
        channel_id: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: PATCH /guilds/{guild_id}/members/{user_id}
        // Yield MemberEvent::Updated
    }

    /// Add a role to the member
    #[hub_macro::hub_method(
        description = "Add a role to this member",
        params(
            role_id = "ID of the role to add",
            token_name = "Token to use (optional)"
        )
    )]
    async fn add_role(
        &self,
        role_id: String,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: PUT /guilds/{guild_id}/members/{user_id}/roles/{role_id}
        // Yield MemberEvent::RoleAdded
    }

    /// Remove a role from the member
    #[hub_macro::hub_method(
        description = "Remove a role from this member",
        params(
            role_id = "ID of the role to remove",
            token_name = "Token to use (optional)"
        )
    )]
    async fn remove_role(
        &self,
        role_id: String,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: DELETE /guilds/{guild_id}/members/{user_id}/roles/{role_id}
        // Yield MemberEvent::RoleRemoved
    }

    /// Kick the member from the guild
    #[hub_macro::hub_method(
        description = "Kick this member from the guild",
        params(
            reason = "Reason for kicking (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn kick(
        &self,
        reason: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: DELETE /guilds/{guild_id}/members/{user_id}
        // Set X-Audit-Log-Reason header if reason provided
        // Yield MemberEvent::Kicked
    }

    /// Ban the member from the guild
    #[hub_macro::hub_method(
        description = "Ban this member from the guild",
        params(
            delete_message_days = "Number of days of messages to delete (0-7, default 0)",
            reason = "Reason for banning (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn ban(
        &self,
        delete_message_days: Option<u8>,
        reason: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: PUT /guilds/{guild_id}/bans/{user_id}
        // Body: {"delete_message_days": N}
        // Set X-Audit-Log-Reason header if reason provided
        // Yield MemberEvent::Banned
    }

    /// Unban the member
    #[hub_macro::hub_method(
        description = "Remove ban for this member",
        params(
            reason = "Reason for unbanning (optional)",
            token_name = "Token to use (optional)"
        )
    )]
    async fn unban(
        &self,
        reason: Option<String>,
        token_name: Option<String>,
    ) -> impl Stream<Item = MemberEvent> + Send + 'static {
        // Implementation: DELETE /guilds/{guild_id}/bans/{user_id}
        // Set X-Audit-Log-Reason header if reason provided
        // Yield MemberEvent::Unbanned
    }
}

#[async_trait]
impl ChildRouter for MemberActivation {
    fn router_namespace(&self) -> &str {
        &self.user_id
    }

    async fn router_call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        Activation::call(self, method, params).await
    }

    async fn get_child(&self, name: &str) -> Option<Box<dyn ChildRouter>> {
        match name {
            "roles" => {
                // Future: could provide a roles sub-activation for listing member's roles
                None
            }
            _ => None,
        }
    }
}
```

### Event Types

```rust
// guilds/members/types.rs

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum MemberEvent {
    /// Members listed
    Listed {
        guild_id: String,
        members: Vec<MemberInfo>,
        has_more: bool,
    },

    /// Search results
    SearchResults {
        guild_id: String,
        query: String,
        members: Vec<MemberInfo>,
    },

    /// Member info retrieved
    Info {
        user_id: String,
        data: serde_json::Value,
    },

    /// Member updated
    Updated {
        user_id: String,
        data: serde_json::Value,
    },

    /// Role added to member
    RoleAdded {
        user_id: String,
        role_id: String,
    },

    /// Role removed from member
    RoleRemoved {
        user_id: String,
        role_id: String,
    },

    /// Member kicked
    Kicked {
        user_id: String,
        reason: Option<String>,
    },

    /// Member banned
    Banned {
        user_id: String,
        reason: Option<String>,
    },

    /// Member unbanned
    Unbanned {
        user_id: String,
    },

    /// Ban list retrieved
    BanList {
        guild_id: String,
        bans: Vec<BanInfo>,
    },

    /// Error occurred
    Error {
        user_id: Option<String>,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MemberInfo {
    pub user_id: String,
    pub username: String,
    pub nickname: Option<String>,
    pub roles: Vec<String>,
    pub joined_at: String,
    pub premium_since: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BanInfo {
    pub user_id: String,
    pub username: String,
    pub reason: Option<String>,
}
```

## CLI Usage Examples

```bash
# List members (first page)
synapse discord guilds 123456789012345678 members list --limit 50

# List members (next page)
synapse discord guilds 123456789012345678 members list \
  --limit 50 \
  --after 987654321098765432

# Search for members
synapse discord guilds 123456789012345678 members search --query "alice"

# Get member info
synapse discord guilds 123456789012345678 members 987654321098765432 info

# Update member nickname
synapse discord guilds 123456789012345678 members 987654321098765432 update \
  --nickname "Alice the Great"

# Server mute a member
synapse discord guilds 123456789012345678 members 987654321098765432 update \
  --mute true

# Add role to member
synapse discord guilds 123456789012345678 members 987654321098765432 add-role \
  --role-id 111111111111111111

# Remove role from member
synapse discord guilds 123456789012345678 members 987654321098765432 remove-role \
  --role-id 111111111111111111

# Kick member
synapse discord guilds 123456789012345678 members 987654321098765432 kick \
  --reason "Spamming"

# Ban member
synapse discord guilds 123456789012345678 members 987654321098765432 ban \
  --delete-message-days 7 \
  --reason "Terms of service violation"

# Unban member
synapse discord guilds 123456789012345678 members 987654321098765432 unban

# List banned members
synapse discord guilds 123456789012345678 members list-bans
```

## Integration Requirements

### For DISCORD-4 (Guild):
Must call `MembersActivation::new(guild_id, client, token_manager)` in `get_child("members")`

### For DISCORD-6 (Roles):
- Member operations reference role IDs
- Role data comes from DISCORD-6 endpoints
- Cannot add/remove managed roles (bot roles, boosting roles)

## Testing Strategy

1. **Unit Tests:**
   - Pagination logic
   - User ID validation
   - Delete message days validation (0-7)

2. **Integration Tests:**
   - List and search members
   - Get member info
   - Update member properties
   - Add/remove roles
   - Kick and ban operations

3. **Error Cases:**
   - User not in guild
   - Missing permissions (KICK_MEMBERS, BAN_MEMBERS, MANAGE_ROLES)
   - Cannot modify owner
   - Cannot kick/ban users with higher roles
   - Invalid delete_message_days value

## Notes

- Member list can be very large (use pagination)
- Search is rate-limited - use sparingly
- Cannot kick/ban guild owner
- Cannot modify members with higher role than bot
- Audit log reasons appear in guild's audit log
- Banning also kicks the member
- Delete message days only applies to ban, not kick
- Members have both user data (ID, username) and member data (nickname, roles)
