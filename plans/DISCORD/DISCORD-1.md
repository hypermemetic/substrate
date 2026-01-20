# DISCORD-1: Discord Server Management Plugin (Master Plan)

## Goal

Build a comprehensive Discord server management plugin for Plexus that provides full API access to Discord's server management capabilities. The plugin will use a hierarchical structure matching Discord's API organization (guilds → channels/roles/members), with bot token authentication stored securely in hyperforge.

## Context

Discord provides a rich REST API for server management operations. We need to expose this functionality through Plexus in a way that:
- Mirrors Discord's API structure 1:1 for predictability
- Uses nested dynamic activations (like solar system's planet.moon pattern)
- Stores bot tokens securely using hyperforge
- Handles Discord's rate limiting properly
- Provides clear CLI commands via Synapse

## Architecture Overview

```
discord/                          # Root plugin (namespace: "discord")
├── authenticate                  # Store/update bot token in hyperforge
├── guilds/                      # Guild management hub
│   └── {guild_id}/             # Dynamic activation per guild
│       ├── channels/           # Channel management hub
│       │   ├── create         # Create channel
│       │   ├── list           # List all channels
│       │   └── {channel_id}/  # Dynamic per channel
│       │       ├── get
│       │       ├── update
│       │       ├── delete
│       │       ├── messages/  # Message operations
│       │       └── permissions/
│       ├── roles/              # Role management hub
│       │   ├── create
│       │   ├── list
│       │   └── {role_id}/     # Dynamic per role
│       │       ├── get
│       │       ├── update
│       │       ├── delete
│       │       └── permissions/
│       ├── members/            # Member management hub
│       │   ├── list
│       │   ├── search
│       │   └── {user_id}/     # Dynamic per member
│       │       ├── get
│       │       ├── update
│       │       ├── kick
│       │       ├── ban
│       │       └── roles/     # Manage member roles
│       ├── webhooks/
│       │   ├── create
│       │   ├── list
│       │   └── {webhook_id}/
│       ├── emojis/
│       ├── invites/
│       ├── audit_log/
│       └── events/             # Guild scheduled events
```

## Key Design Decisions

### 1. Hierarchical Dynamic Activations
Following the **solar system pattern**: each guild is a dynamic activation (like a planet), and each guild contains nested dynamic activations for channels, roles, members (like moons).

### 2. Bot Token Management
- Store bot tokens in **hyperforge** secrets
- Each Discord plugin instance references a hyperforge secret
- Support multiple bot tokens for different guilds/applications
- Token format: `discord:bot_token:{name}` in hyperforge

### 3. Discord API Client
- Use `reqwest` or similar HTTP client
- Implement Discord's rate limiting (bucket-based)
- Handle 429 (Rate Limited) responses properly
- Base URL: `https://discord.com/api/v10`

### 4. Error Handling
- Map Discord API errors to PlexusError
- Provide clear error messages for common issues (invalid token, missing permissions, not found)
- Support Discord's error response format

### 5. Event Types
Each resource (guild, channel, role, member) gets its own event enum:
- `GuildEvent` - guild operations
- `ChannelEvent` - channel operations
- `RoleEvent` - role operations
- `MemberEvent` - member operations
- `MessageEvent` - message operations

## Information Required from User

To use the Discord plugin effectively, users need:

1. **Bot Token** (required)
   - Created at https://discord.com/developers/applications
   - Format: `YOUR_BOT_TOKEN_HERE`
   - Stored in hyperforge: `synapse hyperforge secret set discord:bot_token:my_bot`

2. **Guild ID** (required for most operations)
   - Snowflake ID of the Discord server
   - Found by enabling Developer Mode and right-clicking server

3. **Bot Permissions** (setup requirement)
   - The bot must be invited to the guild with appropriate permissions
   - Required permissions depend on operations (e.g., MANAGE_CHANNELS, MANAGE_ROLES)

## Dependency DAG

```
           DISCORD-2 (Foundation)
               │
      ┌────────┼────────┬────────┐
      ▼        ▼        ▼        ▼
   DISCORD-3  DISCORD-4 DISCORD-5 DISCORD-6  ← Phase 1: Core Resources
   (Auth)     (Guild)   (Channel) (Role)
      │        │        │        │
      └────────┴────────┴────────┘
               │
      ┌────────┼────────┬────────┐
      ▼        ▼        ▼        ▼
   DISCORD-7  DISCORD-8 DISCORD-9 DISCORD-10 ← Phase 2: Advanced Features
   (Member)   (Webhook) (Message) (Emoji)
      │        │        │        │
      └────────┴────────┴────────┘
               │
               ▼
           DISCORD-11 (Integration Testing)
```

## Phase Breakdown

### Phase 0: Planning & Design ✓
- [DISCORD-1] Master plan (this document)

### Phase 1: Foundation & Core Resources
**Critical Path:** DISCORD-2 → DISCORD-3 → DISCORD-4
- [DISCORD-2] Plugin foundation & Discord API client
  - HTTP client setup
  - Rate limiting
  - Error handling
  - unlocks: [DISCORD-3, DISCORD-4, DISCORD-5, DISCORD-6]

- [DISCORD-3] Authentication & token management
  - Hyperforge integration
  - Token storage/retrieval
  - requires: [DISCORD-2]
  - unlocks: [DISCORD-4, DISCORD-5, DISCORD-6, DISCORD-7, DISCORD-8, DISCORD-9, DISCORD-10]

- [DISCORD-4] Guild activation (root hub)
  - Guild info retrieval
  - Guild configuration
  - Dynamic guild activation by ID
  - requires: [DISCORD-2, DISCORD-3]
  - unlocks: [DISCORD-5, DISCORD-6, DISCORD-7]

- [DISCORD-5] Channel management (parallel with 6,7)
  - Create/list/update/delete channels
  - Channel permissions
  - Channel nested activation
  - requires: [DISCORD-2, DISCORD-3, DISCORD-4]

- [DISCORD-6] Role management (parallel with 5,7)
  - Create/list/update/delete roles
  - Role permissions
  - Role nested activation
  - requires: [DISCORD-2, DISCORD-3, DISCORD-4]

### Phase 2: Advanced Features (All parallel after Phase 1)
- [DISCORD-7] Member management
  - List/get members
  - Kick/ban/manage roles
  - requires: [DISCORD-4, DISCORD-6]

- [DISCORD-8] Webhook management
  - Create/list/update webhooks
  - Execute webhooks
  - requires: [DISCORD-3, DISCORD-5]

- [DISCORD-9] Message operations
  - Send/edit/delete messages
  - Reactions, embeds
  - requires: [DISCORD-3, DISCORD-5]

- [DISCORD-10] Emoji & sticker management
  - Create/list/update custom emojis
  - requires: [DISCORD-3, DISCORD-4]

### Phase 3: Integration & Polish
- [DISCORD-11] Integration testing & documentation
  - End-to-end tests
  - CLI usage examples
  - Architecture documentation
  - requires: [DISCORD-7, DISCORD-8, DISCORD-9, DISCORD-10]

## Success Criteria

1. **Core Operations Working:**
   - Authenticate with bot token
   - Create channels in a guild
   - Create roles in a guild
   - Assign roles to members
   - Send messages to channels

2. **CLI Experience:**
   ```bash
   # Set up authentication
   synapse hyperforge secret set discord:bot_token:my_bot "YOUR_TOKEN"

   # Create a channel
   synapse discord guilds <guild_id> channels create \
     --name "general" \
     --type text

   # Create a role
   synapse discord guilds <guild_id> roles create \
     --name "Moderator" \
     --color 0xFF0000 \
     --permissions 8

   # List channels
   synapse discord guilds <guild_id> channels list
   ```

3. **Error Handling:**
   - Clear messages for missing tokens
   - Proper rate limit handling
   - Permission error messages

4. **Architecture:**
   - Follows solar system nested pattern
   - Uses hub_macro for method registration
   - Clean event type hierarchy
   - Proper handle support for resources

## Non-Goals (Future Work)

- Gateway/WebSocket support (real-time events)
- Voice channel operations
- Application commands (slash commands)
- OAuth2 user authentication
- Direct message operations
- Auto-moderation configuration

## References

- Discord API Documentation: https://discord.com/developers/docs/intro
- Plugin Development Guide: `docs/architecture/16678373036159325695_plugin-development-guide.md`
- Solar System Reference: `src/activations/solar/`
- Hyperforge Integration: `hyperforge/src/activations/`
