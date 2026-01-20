# HANDLE-5: Plexus resolve_handle RPC Method

**blocked_by**: [HANDLE-3, HANDLE-4]
**unlocks**: [HANDLE-6, HANDLE-9]

## Scope

Expose handle resolution as an MCP-callable method on Plexus.

## Acceptance Criteria

- [ ] Add `resolve_handle` method to Plexus hub_methods
- [ ] Route resolution through plugin registry
- [ ] Return resolved data as streaming response
- [ ] Handle errors gracefully (unknown plugin, resolution failure)

## Implementation

The infrastructure already exists (`Plexus::do_resolve_handle`). Just need to expose it:

```rust
#[hub_macro::hub_methods(...)]
impl Plexus {
    /// Resolve a handle through its owning plugin
    #[hub_macro::hub_method(
        streaming,
        description = "Resolve a handle through its owning plugin",
        params(handle = "The handle to resolve")
    )]
    async fn resolve_handle(
        &self,
        handle: Handle,
    ) -> impl Stream<Item = PlexusStreamItem> + Send + 'static {
        let result = self.do_resolve_handle(&handle).await;
        // ... forward stream or return error
    }
}
```

## MCP Usage

```json
{
  "method": "plexus.resolve_handle",
  "params": {
    "handle": {
      "plugin_id": "51b330e5-ed88-5fe2-8b58-0c57f2b02ab3",
      "version": "1.0.0",
      "method": "chat_event",
      "meta": ["event-123"]
    }
  }
}
```

## Testing

1. Create a handle for an existing resource
2. Call `plexus.resolve_handle` via MCP
3. Verify correct data is returned
4. Test error cases: unknown plugin, missing resource

## Estimated Complexity

Low - infrastructure exists, just exposing it
