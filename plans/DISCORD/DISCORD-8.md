# DISCORD-8: Webhook Management

**Status:** Not started
**blocked_by:** [DISCORD-3, DISCORD-4, DISCORD-5]
**unlocks:** []

## Scope

Implement webhook creation, management, and execution. Webhooks allow external services to post messages to Discord channels. Accessed via: `discord.guilds.{guild_id}.webhooks` and `discord.guilds.{guild_id}.channels.{channel_id}.webhooks`.

## Acceptance Criteria

- [ ] Create webhooks for channels
- [ ] List webhooks for guild or channel
- [ ] Get, update, delete individual webhooks
- [ ] Execute webhooks (send messages)
- [ ] Implements `ChildRouter` for integration with DISCORD-4
- [ ] CLI commands functional

## Discord API Endpoints

```
POST   /channels/{channel_id}/webhooks           # Create webhook
GET    /channels/{channel_id}/webhooks           # List channel webhooks
GET    /guilds/{guild_id}/webhooks               # List guild webhooks
GET    /webhooks/{webhook_id}                    # Get webhook
PATCH  /webhooks/{webhook_id}                    # Update webhook
DELETE /webhooks/{webhook_id}                    # Delete webhook
POST   /webhooks/{webhook_id}/{webhook_token}    # Execute webhook
```

## Implementation Interface

### Required Constructors
```rust
WebhooksActivation::new(guild_id, client, token_manager) -> Self
```

### Key Methods
- `create(channel_id, name, avatar)` - Create webhook in channel
- `list()` - List all guild webhooks
- `{webhook_id}.info()` - Get webhook details
- `{webhook_id}.update(name, avatar, channel_id)` - Update webhook
- `{webhook_id}.delete()` - Delete webhook
- `{webhook_id}.execute(content, username, avatar_url, embeds)` - Send message

### Event Types
```rust
pub enum WebhookEvent {
    Created { webhook_id, url, token },
    Listed { webhooks: Vec<WebhookInfo> },
    Info { webhook_id, data },
    Updated { webhook_id },
    Deleted { webhook_id },
    Executed { webhook_id, message_id },
    Error { message },
}
```

## CLI Usage Examples

```bash
# Create webhook
synapse discord guilds {guild_id} webhooks create \
  --channel-id {channel_id} \
  --name "CI Bot"

# List webhooks
synapse discord guilds {guild_id} webhooks list

# Execute webhook
synapse discord guilds {guild_id} webhooks {webhook_id} execute \
  --content "Build succeeded!" \
  --username "GitHub Actions"
```

## Notes
- Webhook execution doesn't require bot token (uses webhook token)
- Webhooks can send rich embeds
- Rate limits apply per-webhook
- MANAGE_WEBHOOKS permission required for CRUD operations
