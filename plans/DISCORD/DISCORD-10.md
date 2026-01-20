# DISCORD-10: Emoji & Sticker Management

**Status:** Not started
**blocked_by:** [DISCORD-3, DISCORD-4]
**unlocks:** []

## Scope

Implement custom emoji and sticker management for guilds. Allows creating, listing, updating, and deleting custom emojis. Accessed via: `discord.guilds.{guild_id}.emojis`.

## Acceptance Criteria

- [ ] Create custom emojis
- [ ] List guild emojis
- [ ] Get, update, delete individual emojis
- [ ] Implements `ChildRouter` for integration with DISCORD-4
- [ ] CLI commands functional

## Discord API Endpoints

```
GET    /guilds/{guild_id}/emojis           # List emojis
GET    /guilds/{guild_id}/emojis/{id}      # Get emoji
POST   /guilds/{guild_id}/emojis           # Create emoji
PATCH  /guilds/{guild_id}/emojis/{id}      # Update emoji
DELETE /guilds/{guild_id}/emojis/{id}      # Delete emoji
```

## Implementation Interface

### Required Constructors
```rust
EmojisActivation::new(guild_id, client, token_manager) -> Self
EmojiActivation::new(guild_id, emoji_id, client, token_manager) -> Self
```

### Key Methods
- `create(name, image_data, roles)` - Create custom emoji
- `list()` - List all guild emojis
- `{emoji_id}.info()` - Get emoji details
- `{emoji_id}.update(name, roles)` - Update emoji
- `{emoji_id}.delete()` - Delete emoji

### Event Types
```rust
pub enum EmojiEvent {
    Created { emoji_id, name },
    Listed { emojis: Vec<EmojiInfo> },
    Info { emoji_id, data },
    Updated { emoji_id },
    Deleted { emoji_id },
    Error { message },
}

pub struct EmojiInfo {
    pub id: String,
    pub name: String,
    pub roles: Vec<String>,
    pub require_colons: bool,
    pub managed: bool,
    pub animated: bool,
}
```

## CLI Usage Examples

```bash
# Create emoji from file
synapse discord guilds {guild_id} emojis create \
  --name "custom_emoji" \
  --image "path/to/image.png"

# List emojis
synapse discord guilds {guild_id} emojis list

# Update emoji
synapse discord guilds {guild_id} emojis {emoji_id} update \
  --name "renamed_emoji"

# Delete emoji
synapse discord guilds {guild_id} emojis {emoji_id} delete
```

## Integration Requirements

### For DISCORD-4 (Guild):
Must call `EmojisActivation::new(guild_id, client, token_manager)` in `get_child("emojis")`

## Notes
- Image must be base64 encoded data URI
- Image size limit: 256KB
- Supported formats: PNG, JPEG, GIF (animated)
- MANAGE_EMOJIS_AND_STICKERS permission required
- Normal servers: 50 emojis, Boosted: up to 250
- Emoji names must be 2-32 alphanumeric/underscore characters
