# HANDLE-8: Update Cone Tests to Use Direct ArborStorage

**blocked_by**: [HANDLE-7]
**unlocks**: []

## Scope

Refactor `test_cone_arbor_with_plexus` test to properly demonstrate:
1. Cone operations via Plexus (correct)
2. Arbor operations via direct ArborStorage (correct)

The current test incorrectly calls `plexus.route("arbor.tree_render", ...)` and `plexus.route("arbor.tree_list", ...)`, which violates the architectural principle that Arbor operations should use direct storage access.

## Current State

In `src/activations/cone/tests.rs`:

```rust
#[tokio::test]
async fn test_cone_arbor_with_plexus() {
    // ... setup ...

    // CORRECT: Cone operations go through Plexus
    let mut stream = plexus.route("cone.create", create_params).await.unwrap();

    // INCORRECT: Arbor operations should NOT go through Plexus
    let mut stream = plexus.route("arbor.tree_render", render_params).await.unwrap();
    let mut stream = plexus.route("arbor.tree_list", serde_json::json!({})).await.unwrap();
}
```

## Acceptance Criteria

- [ ] Rename test to `test_cone_with_plexus_and_direct_arbor`
- [ ] Keep Cone method calls through Plexus (correct pattern)
- [ ] Replace Arbor Plexus calls with direct ArborStorage calls
- [ ] Add comments explaining why each approach is used
- [ ] Add a new test `test_handle_resolution_via_plexus` that demonstrates the ONE valid case for Arbor + Plexus (HANDLE-9 territory)

## Implementation Notes

### Refactored Test Structure

```rust
#[tokio::test]
async fn test_cone_with_plexus_and_direct_arbor() {
    let dir = tempdir().unwrap();

    // Setup: Create activations
    let arbor = Arbor::new(arbor_config).await.unwrap();
    let arbor_storage = arbor.storage();  // Keep reference for direct access
    let cone = Cone::new(cone_config, arbor_storage.clone()).await.unwrap();

    let plexus = Arc::new(Plexus::new().register(arbor).register(cone));

    // Cone operations: Via Plexus (correct - Cone is a normal activation)
    let mut stream = plexus.route("cone.create", create_params).await.unwrap();
    // ... extract cone_id, tree_id, head_node ...

    // Arbor operations: Direct storage access (correct - Arbor is infrastructure)
    // NOT: plexus.route("arbor.tree_render", ...)
    let tree = arbor_storage.tree_get(&tree_id.parse().unwrap()).await.unwrap();
    let rendered = tree.render();
    println!("Tree render (direct):\n{}", rendered);

    // Tree listing: Direct
    let trees = arbor_storage.tree_list().await.unwrap();
    assert_eq!(trees.len(), 1);
}
```

### Test Organization

Consider reorganizing tests into:
1. `test_cone_message_to_arbor_handle_direct` - Storage-only tests (already correct)
2. `test_multi_turn_conversation` - Multi-message flow (already correct)
3. `test_cone_with_plexus_and_direct_arbor` - Shows proper Plexus vs direct usage
4. `test_handle_resolution_via_plexus` - Future test for HANDLE-9

### Why This Matters

The test as currently written might lead developers to believe:
- "To use Arbor, I should call through Plexus"

But the correct mental model is:
- "Arbor is injected at construction. I use it directly."
- "Plexus is for calling OTHER activations' methods"

## Estimated Complexity

Low - Test refactoring only, no production code changes
