# DISCORD-11: Integration Testing & Documentation

**Status:** Not started
**blocked_by:** [DISCORD-7, DISCORD-8, DISCORD-9, DISCORD-10]
**unlocks:** []

## Scope

End-to-end integration testing of the complete Discord plugin. Verify all components work together correctly and document usage patterns.

## Acceptance Criteria

- [ ] Integration test suite covering all major operations
- [ ] Test guild → channels → messages flow
- [ ] Test guild → roles → members flow
- [ ] Test error handling across components
- [ ] Test authentication and token management
- [ ] CLI usage documentation with examples
- [ ] Architecture documentation added to `docs/architecture/`
- [ ] README.md in `discord/` directory

## Test Scenarios

### 1. Full Server Setup Flow
```rust
#[tokio::test]
async fn test_full_server_setup() {
    // 1. Authenticate
    // 2. Create channels (text, voice, category)
    // 3. Create roles (admin, moderator, member)
    // 4. Set channel permissions for roles
    // 5. Send test message to channel
    // 6. Verify everything created successfully
}
```

### 2. Member Management Flow
```rust
#[tokio::test]
async fn test_member_management() {
    // 1. List members
    // 2. Get member info
    // 3. Assign role to member
    // 4. Update member nickname
    // 5. Verify changes
}
```

### 3. Message Workflow
```rust
#[tokio::test]
async fn test_message_workflow() {
    // 1. Send message to channel
    // 2. Edit message
    // 3. React to message
    // 4. Get message history
    // 5. Delete message
}
```

### 4. Error Handling
```rust
#[tokio::test]
async fn test_error_scenarios() {
    // 1. Invalid token
    // 2. Missing permissions
    // 3. Invalid guild/channel/user ID
    // 4. Rate limiting
    // 5. API errors
}
```

### 5. Webhook Integration
```rust
#[tokio::test]
async fn test_webhook_flow() {
    // 1. Create webhook
    // 2. Execute webhook (send message)
    // 3. List webhooks
    // 4. Delete webhook
}
```

## Documentation Requirements

### Architecture Doc
Create `docs/architecture/{timestamp}_discord-plugin.md` with:
- Plugin architecture overview
- Nested activation structure diagram
- Integration with hyperforge for secrets
- Rate limiting implementation
- Error handling patterns
- Extension points for future features

### README.md
Create `src/activations/discord/README.md` with:
- Quick start guide
- Authentication setup
- Common operations examples
- Troubleshooting section
- API endpoint reference

### CLI Guide
Document all CLI commands:
```bash
# Authentication
synapse discord auth-set --name default --token "..."
synapse discord auth-info

# Guild Operations
synapse discord guilds {id} info
synapse discord guilds {id} update --name "New Name"

# Channel Operations
synapse discord guilds {id} channels create --name "general"
synapse discord guilds {id} channels list
synapse discord guilds {id} channels {id} update --topic "New topic"

# Role Operations
synapse discord guilds {id} roles create --name "Moderator"
synapse discord guilds {id} roles list

# Member Operations
synapse discord guilds {id} members list
synapse discord guilds {id} members {id} add-role --role-id {id}

# Message Operations
synapse discord guilds {id} channels {id} messages send --content "Hello"
```

## Testing Strategy

1. **Unit Tests** (per-ticket): Already covered in individual tickets
2. **Integration Tests** (this ticket): End-to-end flows
3. **Manual Testing**: Real Discord server operations
4. **Error Testing**: Network failures, API errors, rate limits

## Success Criteria

- [ ] All integration tests pass
- [ ] Can perform complete server setup via CLI
- [ ] Error messages are clear and actionable
- [ ] Documentation is complete and accurate
- [ ] Architecture doc is linked in main docs/architecture/README.md
- [ ] Plugin is ready for production use

## Manual Testing Checklist

Using a real Discord bot token and test server:

- [ ] Set bot token
- [ ] Get guild info
- [ ] Create text channel
- [ ] Create voice channel
- [ ] Create category
- [ ] Create role with permissions
- [ ] Update role color
- [ ] Assign role to member
- [ ] Send message to channel
- [ ] Send embed message
- [ ] Edit message
- [ ] React to message
- [ ] Create webhook
- [ ] Execute webhook
- [ ] Create custom emoji
- [ ] List members
- [ ] Update member nickname
- [ ] Delete channel
- [ ] Delete role

## Notes

- Use a test Discord server, not production
- Test bot needs all required permissions
- Some operations require Discord premium (e.g., emoji limits)
- Rate limiting may require delays between tests
- Clean up test resources after each run
