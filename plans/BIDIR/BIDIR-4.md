# BIDIR-4: Hub-Macro Bidirectional Support

**Status:** Planning
**Blocked By:** BIDIR-2
**Unlocks:** BIDIR-6, BIDIR-7

## Scope

Update the hub-macro to generate code for bidirectional methods while maintaining backward compatibility with existing unidirectional methods.

## Acceptance Criteria

- [ ] New `#[hub_method(bidirectional)]` attribute
- [ ] Generated dispatch code handles bidirectional context
- [ ] Backward compatible - methods without attribute unchanged
- [ ] Type-safe context injection

## Implementation Notes

### New Attribute Syntax

```rust
// Existing unidirectional method (unchanged)
#[hub_method(description = "List all items")]
pub async fn list(&self) -> impl Stream<Item = MyEvent> + Send + 'static {
    // ...
}

// NEW: Bidirectional method
#[hub_method(
    description = "Sync with confirmation",
    bidirectional,  // ← enables bidirectional context
)]
pub async fn sync(
    &self,
    ctx: &BidirChannel,  // ← context parameter injected
    dry_run: Option<bool>,
) -> impl Stream<Item = SyncEvent> + Send + 'static {
    // Can now use ctx.confirm(), ctx.prompt(), etc.
    if !dry_run.unwrap_or(false) {
        if !ctx.confirm("Proceed with sync?").await.unwrap_or(false) {
            return stream! { yield SyncEvent::Cancelled };
        }
    }
    // ... actual sync logic
}
```

### Parse Changes

```rust
// hub-macro/src/parse.rs

#[derive(Default)]
pub struct HubMethodAttrs {
    pub description: Option<String>,
    pub params: HashMap<String, String>,
    pub bidirectional: bool,  // ← NEW
}

impl Parse for HubMethodAttrs {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        // ... existing parsing ...

        // Parse bidirectional flag
        if input.peek(Ident) {
            let ident: Ident = input.parse()?;
            if ident == "bidirectional" {
                attrs.bidirectional = true;
            }
        }

        Ok(attrs)
    }
}
```

### MethodInfo Updates

```rust
// hub-macro/src/parse.rs

pub struct MethodInfo {
    pub name: Ident,
    pub args: Vec<MethodArg>,
    pub return_type: Type,
    pub description: Option<String>,
    pub params_doc: HashMap<String, String>,
    pub bidirectional: bool,  // ← NEW

    // For bidirectional methods, track the context parameter
    pub context_param: Option<Ident>,  // ← NEW
}

impl MethodInfo {
    pub fn from_fn(method: &ImplItemFn, attrs: Option<&HubMethodAttrs>) -> syn::Result<Self> {
        // ... existing parsing ...

        let bidirectional = attrs.map(|a| a.bidirectional).unwrap_or(false);

        // If bidirectional, find and extract context parameter
        let context_param = if bidirectional {
            args.iter()
                .find(|arg| {
                    // Look for &BidirChannel or Arc<BidirChannel>
                    matches_bidir_type(&arg.ty)
                })
                .map(|arg| arg.name.clone())
        } else {
            None
        };

        Ok(Self {
            name,
            args,
            return_type,
            description,
            params_doc,
            bidirectional,
            context_param,
        })
    }
}

fn matches_bidir_type(ty: &Type) -> bool {
    // Check for &BidirChannel, Arc<BidirChannel>, etc.
    let ty_str = quote!(#ty).to_string();
    ty_str.contains("BidirChannel") || ty_str.contains("CallContext")
}
```

### Code Generation Changes

```rust
// hub-macro/src/codegen/activation.rs

fn generate_dispatch_arm(method: &MethodInfo, crate_path: &syn::Path) -> TokenStream {
    let method_name = &method.name;
    let method_str = method_name.to_string();

    if method.bidirectional {
        // Bidirectional method - needs context
        generate_bidir_dispatch_arm(method, crate_path)
    } else {
        // Existing unidirectional dispatch
        generate_unidir_dispatch_arm(method, crate_path)
    }
}

fn generate_bidir_dispatch_arm(method: &MethodInfo, crate_path: &syn::Path) -> TokenStream {
    let method_name = &method.name;
    let method_str = method_name.to_string();

    // Extract non-context params
    let param_args: Vec<_> = method.args.iter()
        .filter(|arg| !matches_bidir_type(&arg.ty))
        .collect();

    // Generate parameter extraction
    let param_extraction = generate_param_extraction(&param_args);

    // Generate call with context
    quote! {
        #method_str => {
            #param_extraction

            // Check if context supports bidirectional
            let ctx = context.as_ref().ok_or_else(|| {
                #crate_path::plexus::PlexusError::ExecutionError(
                    "Bidirectional method called without context".into()
                )
            })?;

            let stream = self.#method_name(ctx, #(#param_args),*).await;
            Ok(#crate_path::plexus::wrap_stream(
                stream,
                concat!(#namespace, ".", #method_str),
                vec![#namespace.into()],
            ))
        }
    }
}
```

### Updated Activation Trait Call Signature

```rust
// hub-core/src/plexus/plexus.rs

#[async_trait]
pub trait Activation: Send + Sync + 'static {
    type Methods: MethodEnumSchema;

    /// Call method without bidirectional context (backward compatible)
    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError>;

    /// Call method with optional bidirectional context
    async fn call_with_context(
        &self,
        method: &str,
        params: Value,
        context: Option<Arc<BidirChannel>>,
    ) -> Result<PlexusStream, PlexusError> {
        // Default: ignore context, call regular method
        self.call(method, params).await
    }

    // ... other methods unchanged
}
```

### Generated Implementation

```rust
// What the macro generates for an activation with bidirectional methods:

#[async_trait]
impl Activation for MyActivation {
    type Methods = MyActivationMethod;

    async fn call(&self, method: &str, params: Value) -> Result<PlexusStream, PlexusError> {
        // For unidirectional methods only
        match method {
            "list" => { /* existing dispatch */ }
            "sync" => {
                // Bidirectional method called without context
                return Err(PlexusError::ExecutionError(
                    "Method 'sync' requires bidirectional context".into()
                ));
            }
            _ => Err(PlexusError::MethodNotFound { .. })
        }
    }

    async fn call_with_context(
        &self,
        method: &str,
        params: Value,
        context: Option<Arc<BidirChannel>>,
    ) -> Result<PlexusStream, PlexusError> {
        match method {
            "list" => {
                // Unidirectional - ignore context
                let stream = self.list().await;
                Ok(wrap_stream(stream, "myactivation.list", vec!["myactivation".into()]))
            }
            "sync" => {
                // Bidirectional - require context
                let ctx = context.as_ref().ok_or_else(|| {
                    PlexusError::ExecutionError("sync requires bidirectional context".into())
                })?;
                let dry_run: Option<bool> = /* extract from params */;
                let stream = self.sync(ctx.as_ref(), dry_run).await;
                Ok(wrap_stream(stream, "myactivation.sync", vec!["myactivation".into()]))
            }
            _ => Err(PlexusError::MethodNotFound { .. })
        }
    }
}
```

### Schema Generation Updates

```rust
// hub-macro/src/codegen/method_enum.rs

fn generate_method_schema(method: &MethodInfo) -> TokenStream {
    let name = method.name.to_string();
    let description = method.description.as_deref().unwrap_or("");
    let bidirectional = method.bidirectional;

    // Filter out context param from schema
    let params: Vec<_> = method.args.iter()
        .filter(|arg| !matches_bidir_type(&arg.ty))
        .map(generate_param_schema)
        .collect();

    quote! {
        MethodSchema {
            name: #name.into(),
            description: #description.into(),
            params: vec![#(#params),*],
            bidirectional: #bidirectional,  // ← NEW FIELD
            // ...
        }
    }
}
```

## Files to Create/Modify

| File | Action |
|------|--------|
| `hub-macro/src/parse.rs` | Add bidirectional flag parsing |
| `hub-macro/src/codegen/activation.rs` | Generate bidir dispatch arms |
| `hub-macro/src/codegen/method_enum.rs` | Add bidirectional to schema |
| `hub-core/src/plexus/plexus.rs` | Add call_with_context to Activation |
| `hub-core/src/plexus/schema.rs` | Add bidirectional field to MethodSchema |

## Testing

```rust
// Test that bidirectional attribute is parsed
#[test]
fn test_parse_bidirectional_attr() {
    let input: TokenStream = quote! {
        description = "Test method",
        bidirectional
    };
    let attrs: HubMethodAttrs = syn::parse2(input).unwrap();
    assert!(attrs.bidirectional);
}

// Test schema includes bidirectional flag
#[test]
fn test_schema_bidirectional_flag() {
    let schema = MyActivation::schema();
    let sync_method = schema.methods.iter().find(|m| m.name == "sync").unwrap();
    assert!(sync_method.bidirectional);

    let list_method = schema.methods.iter().find(|m| m.name == "list").unwrap();
    assert!(!list_method.bidirectional);
}
```

## Notes

- Context parameter must be first non-self parameter for consistency
- Schema excludes context parameter (not user-visible)
- Backward compatible: activations without bidirectional methods work unchanged
- Error message guides users when calling bidir method without context
