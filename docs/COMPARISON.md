# Plexus RPC Compared to Other RPC Frameworks

This document compares Plexus RPC to popular RPC frameworks and explains when to choose Plexus RPC.

## Quick Comparison Table

| Feature | Plexus RPC | gRPC | OpenAPI/REST | tRPC | GraphQL |
|---------|-----------|------|--------------|------|---------|
| **Schema source** | Rust code | .proto files | YAML/annotations | TypeScript code | .graphql schema |
| **Schema drift** | Impossible (hash-based) | Easy (proto vs code) | Very easy | Easy (monorepo only) | Easy (schema vs resolvers) |
| **Streaming** | Built-in (every method) | Bolt-on (special setup) | SSE (separate) | Limited | Subscriptions (complex) |
| **Type safety** | Rust → TS/clients | ✓ (with codegen) | ❌ (runtime only) | ✓ (TS only) | ✓ (with codegen) |
| **Runtime introspection** | Full (dynamic CLI) | Limited | Swagger UI | ❌ | GraphiQL |
| **Tree namespaces** | Native | ❌ | ❌ | ❌ | ❌ |
| **Progress events** | Built-in | Manual | Manual | Manual | Manual |
| **Error handling** | Structured (PlexusStreamItem) | Status codes | HTTP codes | Exceptions | Errors array |
| **Language flexibility** | Any (via codegen) | Any (via proto) | Any | TypeScript-first | Any (via codegen) |
| **Dev setup** | Rust + codegen | Proto compiler | OpenAPI tools | TypeScript mono repo | GraphQL tools |

---

## The Key Difference: Code IS Schema

### Plexus RPC Approach

```rust
#[hub_method]
async fn create_user(&self, email: String, name: String)
    -> impl Stream<Item = UserEvent>
{
    stream! {
        yield UserEvent::Validating;
        // validation logic
        yield UserEvent::Creating;
        // database insert
        yield UserEvent::Created { id };
    }
}
```

**What happens:**
1. Rust types define the schema
2. Schema is extracted at compile time
3. Schema is available at runtime via RPC
4. TypeScript client is generated from runtime schema
5. CLI is generated from runtime schema
6. **Zero separate schema files**

### gRPC Approach

```protobuf
// user.proto (separate file)
service UserService {
  rpc CreateUser(CreateUserRequest) returns (CreateUserResponse);
}

message CreateUserRequest {
  string email = 1;
  string name = 2;
}
```

```rust
// user_service.rs (implementation)
impl UserService for UserServiceImpl {
    async fn create_user(&self, request: Request<CreateUserRequest>)
        -> Result<Response<CreateUserResponse>, Status>
    {
        // Can diverge from proto
    }
}
```

**Problems:**
- Proto file and Rust code can diverge
- No compile-time guarantee of sync
- Manual proto updates when adding fields
- Streaming requires special setup

### OpenAPI/REST Approach

```rust
/// POST /users
/// Request: { "email": "...", "name": "..." }
#[post("/users")]
async fn create_user(data: Json<CreateUserRequest>) -> Result<Json<User>, Error> {
    // implementation
}
```

```yaml
# openapi.yaml (separate file, manually maintained)
paths:
  /users:
    post:
      requestBody:
        content:
          application/json:
            schema:
              type: object
              properties:
                email: { type: string }
                name: { type: string }
```

**Problems:**
- OpenAPI spec and code can diverge completely
- No compile-time checking
- Must manually update YAML when code changes
- Streaming requires SSE (separate implementation)
- Type safety only at runtime

---

## Detailed Comparisons

### vs gRPC

**When gRPC is better:**
- Existing Proto ecosystem
- Need maximum performance (binary protocol)
- Polyglot services already using Proto

**When Plexus RPC is better:**
- Starting a new Rust project
- Want to avoid maintaining .proto files
- Need runtime schema introspection
- Want streaming-first design
- Need dynamic CLIs

**Side-by-side:**

```proto
// gRPC: Separate proto file
service Echo {
  rpc Echo(EchoRequest) returns (stream EchoResponse);
}

message EchoRequest {
  string message = 1;
  uint32 count = 2;
}

message EchoResponse {
  string message = 1;
}
```

```rust
// Plexus RPC: Code IS schema
#[hub_method(streaming)]
async fn echo(&self, message: String, count: u32)
    -> impl Stream<Item = EchoEvent>
{
    stream! {
        for _ in 0..count {
            yield EchoEvent::Echo { message: message.clone() };
        }
    }
}
```

**Result:**
- gRPC: 2 files (proto + impl), manual sync, streaming setup complex
- Plexus RPC: 1 file, automatic sync, streaming built-in

---

### vs OpenAPI/REST

**When OpenAPI is better:**
- Existing HTTP/REST infrastructure
- Need to match RESTful conventions
- Consumers expect REST APIs

**When Plexus RPC is better:**
- New service without REST constraints
- Need streaming (SSE is clunky in REST)
- Want type safety without separate YAML
- Need progress reporting

**Key differences:**

| Aspect | Plexus RPC | OpenAPI/REST |
|--------|-----------|--------------|
| Schema sync | Automatic | Manual |
| Streaming | Built-in | SSE (separate) |
| Progress | PlexusStreamItem::Progress | Custom headers/events |
| Errors | Structured in stream | HTTP status codes |
| Validation | Compile-time + runtime | Runtime only |
| Type generation | From runtime schema | From YAML spec |

---

### vs tRPC

**When tRPC is better:**
- TypeScript mono repo (frontend + backend)
- Don't want to leave Node.js ecosystem
- Need simplest possible setup

**When Plexus RPC is better:**
- Backend is Rust (better performance, type safety)
- Need language-agnostic clients (not just TS)
- Want streaming as first-class feature
- Need runtime schema introspection

**Philosophy difference:**

| Aspect | Plexus RPC | tRPC |
|--------|-----------|------|
| Language | Rust backend, any client | TypeScript only |
| Schema | Runtime introspection | Type inference |
| Deployment | Backend + frontend separate | Monorepo required |
| Streaming | Every method streams | Limited |
| CLI generation | Automatic | Not supported |

---

### vs GraphQL

**When GraphQL is better:**
- Need flexible client-driven queries
- Want to expose a graph of related data
- Clients need to minimize over-fetching

**When Plexus RPC is better:**
- RPC/method-call model fits better
- Want simpler mental model
- Need streaming progress (not just subscriptions)
- Want to avoid N+1 query problems

**Conceptual differences:**

| Aspect | Plexus RPC | GraphQL |
|--------|-----------|----------|
| Model | RPC (method calls) | Graph (queries) |
| Schema | Rust types | .graphql files |
| Streaming | Every method | Subscriptions only |
| Over-fetching | Caller specifies method | Resolver-based |
| Complexity | Simple (call method) | Complex (resolvers, N+1) |

---

## Real-World Decision Matrix

### Choose Plexus RPC when:

✅ **You're building a new Rust service**
- No legacy constraints
- Want Rust's type safety and performance
- Streaming is important

✅ **You hate maintaining separate schema files**
- OpenAPI YAML drifts from code
- Proto files get out of sync
- Want code to be source of truth

✅ **You need streaming-first design**
- Progress reporting
- Long-running operations
- Real-time updates

✅ **You want runtime introspection**
- Generate CLIs automatically
- Generate documentation automatically
- Adapt clients at runtime

### Choose something else when:

❌ **You have existing infrastructure**
- Already using gRPC/REST everywhere
- Migration cost outweighs benefits
- Ecosystem lock-in is acceptable

❌ **You need maximum interop**
- Proto is universal standard
- GraphQL has massive ecosystem
- REST is everywhere

❌ **Your team doesn't know Rust**
- Learning curve for backend team
- Or use Plexus RPC from another language (future)

---

## Migration Paths

### From gRPC

1. Keep gRPC for external APIs
2. Use Plexus RPC for internal services
3. Gradually migrate as proto files become painful

### From OpenAPI/REST

1. Build new endpoints in Plexus RPC
2. Proxy old REST endpoints through Plexus RPC
3. Generate OpenAPI spec from Plexus RPC schemas (future)

### From GraphQL

1. Map GraphQL resolvers to Plexus RPC activations
2. Use Plexus RPC for RPC-style operations
3. Keep GraphQL for graph queries if needed

---

## Summary

**Plexus RPC is best when:**
- You're building Rust services
- Code-as-schema appeals to you
- Streaming is important
- You want runtime introspection

**Consider alternatives when:**
- You need maximum ecosystem compatibility
- Your team doesn't know Rust
- You have heavy investment in existing RPC framework

**The unique value proposition:**
> "Write Rust methods. Get type-safe clients, dynamic CLIs, and streaming by default. Zero separate schema files, zero drift."

---

## Learn More

- [Quick Start](./QUICKSTART.md) - Get started in 5 minutes
- [Architecture](../README.md) - Deep dive into Plexus RPC design
- [Examples](../examples/) - Real-world usage patterns
