# DISCORD-9: Message Operations

**Status:** Not started
**blocked_by:** [DISCORD-3, DISCORD-5]
**unlocks:** []

## Scope

Implement message sending, editing, deleting, and retrieval operations. Messages are accessed via: `discord.guilds.{guild_id}.channels.{channel_id}.messages`.

This is a critical feature for bot automation.

## Acceptance Criteria

- [ ] Send messages to channels
- [ ] Send messages with embeds, files, components
- [ ] Get message history (with pagination)
- [ ] Get individual message
- [ ] Edit and delete messages
- [ ] Add/remove reactions
- [ ] Implements `ChildRouter` for integration with DISCORD-5
- [ ] CLI commands functional

## Discord API Endpoints

```
POST   /channels/{channel_id}/messages                    # Send message
GET    /channels/{channel_id}/messages                    # Get messages
GET    /channels/{channel_id}/messages/{message_id}       # Get message
PATCH  /channels/{channel_id}/messages/{message_id}       # Edit message
DELETE /channels/{channel_id}/messages/{message_id}       # Delete message
PUT    /channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me  # Add reaction
DELETE /channels/{channel_id}/messages/{message_id}/reactions/{emoji}/@me  # Remove reaction
```

## Implementation Interface

### Required Constructors
```rust
MessagesActivation::new(channel_id, client, token_manager) -> Self
MessageActivation::new(channel_id, message_id, client, token_manager) -> Self
```

### Key Methods
- `send(content, embeds, components, files)` - Send message
- `list(limit, before, after, around)` - Get message history
- `{message_id}.get()` - Get specific message
- `{message_id}.edit(content, embeds)` - Edit message
- `{message_id}.delete()` - Delete message
- `{message_id}.react(emoji)` - Add reaction
- `{message_id}.unreact(emoji)` - Remove reaction

### Event Types
```rust
pub enum MessageEvent {
    Sent { message_id, channel_id, content },
    Listed { messages: Vec<MessageInfo> },
    Retrieved { message_id, data },
    Edited { message_id },
    Deleted { message_id },
    ReactionAdded { message_id, emoji },
    ReactionRemoved { message_id, emoji },
    Error { message },
}
```

## Embed Structure
```rust
pub struct Embed {
    pub title: Option<String>,
    pub description: Option<String>,
    pub color: Option<u32>,
    pub fields: Vec<EmbedField>,
    pub footer: Option<EmbedFooter>,
    pub image: Option<EmbedImage>,
    pub thumbnail: Option<EmbedThumbnail>,
}
```

## CLI Usage Examples

```bash
# Send simple message
synapse discord guilds {guild_id} channels {channel_id} messages send \
  --content "Hello, World!"

# Send embed
synapse discord guilds {guild_id} channels {channel_id} messages send \
  --embed '{"title":"Alert","description":"System status OK","color":65280}'

# Get message history
synapse discord guilds {guild_id} channels {channel_id} messages list \
  --limit 50

# Edit message
synapse discord guilds {guild_id} channels {channel_id} messages {message_id} edit \
  --content "Updated content"

# React to message
synapse discord guilds {guild_id} channels {channel_id} messages {message_id} react \
  --emoji "👍"
```

## Integration Requirements

### For DISCORD-5 (Channels):
Must call `MessagesActivation::new(channel_id, client, token_manager)` in `ChannelActivation::get_child("messages")`

## Notes
- Message content limited to 2000 characters
- Up to 10 embeds per message
- SEND_MESSAGES permission required
- MANAGE_MESSAGES required to delete others' messages
- Reactions use emoji names (`:thumbsup:`) or Unicode
- Bulk delete limited to messages <14 days old
