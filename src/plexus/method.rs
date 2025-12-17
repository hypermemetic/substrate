//! Session-Typed Method Definitions
//!
//! This module provides the `Method` trait for defining RPC methods with
//! session-type protocols. Each method specifies its communication protocol
//! using dialectic session types, enabling:
//!
//! - Compile-time protocol verification
//! - Automatic schema extraction for introspection
//! - Client codegen from protocol definitions
//!
//! # Example
//!
//! ```ignore
//! use dialectic::prelude::*;
//! use substrate::plexus::method::{Method, MethodSchema};
//!
//! // Define the protocol from client's perspective
//! type TreeGetProtocol = Session! {
//!     send TreeGetInput;
//!     recv TreeData;
//! };
//!
//! struct TreeGetMethod;
//!
//! impl Method for TreeGetMethod {
//!     type Protocol = TreeGetProtocol;
//!     const NAME: &'static str = "tree_get";
//!     const DESCRIPTION: &'static str = "Get a tree by ID";
//! }
//!
//! // Get the schema at runtime
//! let schema = TreeGetMethod::schema();
//! ```

use super::session_schema::{ProtocolSchema, SessionSchema};
use dialectic::Session;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Schema for a single method including its session protocol
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MethodSchema {
    /// Method name (e.g., "tree_get")
    pub name: String,

    /// Human-readable description
    pub description: String,

    /// Client-side protocol schema
    ///
    /// Describes the protocol from the client's perspective:
    /// - `send` = client sends data
    /// - `recv` = client receives data
    /// - `offer` = client waits for server to choose
    /// - `choose` = client selects a branch
    pub protocol: ProtocolSchema,

    /// Server-side protocol schema (the dual)
    ///
    /// Automatically derived from the client protocol:
    /// - `send` ↔ `recv`
    /// - `offer` ↔ `choose`
    pub server_protocol: ProtocolSchema,
}

/// A typed RPC method with session-type protocol.
///
/// Implement this trait to define a method with compile-time protocol
/// verification and automatic schema generation.
///
/// # Protocol Perspective
///
/// The `Protocol` type defines the communication from the **client's perspective**.
/// The server implements the dual protocol automatically:
///
/// ```text
/// Client Protocol          Server Protocol (Dual)
/// ──────────────           ─────────────────────
/// send Input               recv Input
/// recv Output              send Output
/// offer { A | B }          choose { A | B }
/// choose { A | B }         offer { A | B }
/// ```
///
/// # Example: Simple Request-Response
///
/// ```ignore
/// type GetProtocol = Session! {
///     send GetInput;    // Client sends input
///     recv GetOutput;   // Client receives output
/// };
/// ```
///
/// # Example: Streaming Response
///
/// ```ignore
/// type StreamProtocol = Session! {
///     send StreamInput;      // Client sends input
///     recv StreamStart;      // Client receives start event
///     loop {
///         offer {            // Client waits for server to choose
///             0 => { recv StreamChunk; continue; },  // More data
///             1 => { recv StreamDone; break; },      // Stream complete
///         }
///     }
/// };
/// ```
pub trait Method: Send + Sync + 'static {
    /// The session protocol for this method (from client's perspective).
    ///
    /// Must implement `Session` (valid session type) and `SessionSchema` (schema extractable).
    /// The dual protocol must also implement `SessionSchema` for server-side schema extraction.
    type Protocol: Session + SessionSchema;

    /// The dual (server-side) protocol.
    /// This is automatically the dual of `Protocol` and must implement `SessionSchema`.
    type ServerProtocol: SessionSchema;

    /// Method name for RPC routing (e.g., "tree_get").
    ///
    /// This is the name clients use to call the method.
    const NAME: &'static str;

    /// Human-readable description of what this method does.
    const DESCRIPTION: &'static str = "";

    /// Get the full method schema including protocol.
    ///
    /// This method is automatically implemented and extracts:
    /// - Method name and description
    /// - Client protocol schema
    /// - Server protocol schema (dual)
    fn schema() -> MethodSchema {
        MethodSchema {
            name: Self::NAME.to_string(),
            description: Self::DESCRIPTION.to_string(),
            protocol: <Self::Protocol as SessionSchema>::schema(),
            server_protocol: <Self::ServerProtocol as SessionSchema>::schema(),
        }
    }
}

/// Schema for an entire activation's methods
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ActivationMethodsSchema {
    /// Activation namespace (e.g., "arbor")
    pub namespace: String,

    /// Activation version
    pub version: String,

    /// All methods provided by this activation
    pub methods: Vec<MethodSchema>,
}

/// A collection of methods for an activation.
///
/// This trait enables activations to enumerate all their methods
/// and route method calls by name.
pub trait MethodCollection {
    /// Get schemas for all methods in this collection.
    fn schemas() -> Vec<MethodSchema>;

    /// Get method names.
    fn method_names() -> Vec<&'static str>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use dialectic::Session;
    use schemars::JsonSchema;
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TestInput {
        id: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TestOutput {
        result: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct StreamStart {
        total: i32,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct StreamChunk {
        content: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct StreamDone {
        count: i32,
    }

    // Simple request-response method
    struct SimpleMethod;

    type SimpleProtocol = Session! {
        send TestInput;
        recv TestOutput;
    };

    impl Method for SimpleMethod {
        type Protocol = SimpleProtocol;
        type ServerProtocol = <SimpleProtocol as Session>::Dual;
        const NAME: &'static str = "simple_method";
        const DESCRIPTION: &'static str = "A simple request-response method";
    }

    // Streaming method
    struct StreamingMethod;

    type StreamingProtocol = Session! {
        send TestInput;
        recv StreamStart;
        loop {
            offer {
                0 => { recv StreamChunk; continue; },
                1 => { recv StreamDone; break; },
            }
        }
    };

    impl Method for StreamingMethod {
        type Protocol = StreamingProtocol;
        type ServerProtocol = <StreamingProtocol as Session>::Dual;
        const NAME: &'static str = "streaming_method";
        const DESCRIPTION: &'static str = "A streaming response method";
    }

    #[test]
    fn test_simple_method_schema() {
        let schema = SimpleMethod::schema();

        assert_eq!(schema.name, "simple_method");
        assert_eq!(schema.description, "A simple request-response method");

        // Client: Send -> Recv -> Done
        match &schema.protocol {
            ProtocolSchema::Send { then, .. } => match &**then {
                ProtocolSchema::Recv { then, .. } => {
                    assert!(matches!(&**then, ProtocolSchema::Done));
                }
                _ => panic!("Expected Recv after Send"),
            },
            _ => panic!("Expected Send first"),
        }

        // Server (dual): Recv -> Send -> Done
        match &schema.server_protocol {
            ProtocolSchema::Recv { then, .. } => match &**then {
                ProtocolSchema::Send { then, .. } => {
                    assert!(matches!(&**then, ProtocolSchema::Done));
                }
                _ => panic!("Expected Send after Recv"),
            },
            _ => panic!("Expected Recv first in server protocol"),
        }
    }

    #[test]
    fn test_streaming_method_schema() {
        let schema = StreamingMethod::schema();

        assert_eq!(schema.name, "streaming_method");

        // Verify client protocol has loop with offer
        match &schema.protocol {
            ProtocolSchema::Send { then, .. } => match &**then {
                ProtocolSchema::Recv { then, .. } => match &**then {
                    ProtocolSchema::Loop { body } => match &**body {
                        ProtocolSchema::Offer { branches } => {
                            assert_eq!(branches.len(), 2);
                        }
                        _ => panic!("Expected Offer in loop body"),
                    },
                    _ => panic!("Expected Loop after Recv"),
                },
                _ => panic!("Expected Recv after Send"),
            },
            _ => panic!("Expected Send first"),
        }

        // Verify server protocol has loop with choose (dual of offer)
        match &schema.server_protocol {
            ProtocolSchema::Recv { then, .. } => match &**then {
                ProtocolSchema::Send { then, .. } => match &**then {
                    ProtocolSchema::Loop { body } => match &**body {
                        ProtocolSchema::Choose { branches } => {
                            assert_eq!(branches.len(), 2);
                        }
                        _ => panic!("Expected Choose in loop body (dual of Offer)"),
                    },
                    _ => panic!("Expected Loop after Send"),
                },
                _ => panic!("Expected Send after Recv"),
            },
            _ => panic!("Expected Recv first in server protocol"),
        }
    }

    #[test]
    fn test_schema_serialization() {
        let schema = SimpleMethod::schema();
        let json = serde_json::to_string_pretty(&schema).unwrap();

        // Verify it's valid JSON with expected fields
        let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(parsed.get("name").is_some());
        assert!(parsed.get("protocol").is_some());
        assert!(parsed.get("server_protocol").is_some());
    }

    // Test MethodCollection
    struct TestMethodCollection;

    impl MethodCollection for TestMethodCollection {
        fn schemas() -> Vec<MethodSchema> {
            vec![SimpleMethod::schema(), StreamingMethod::schema()]
        }

        fn method_names() -> Vec<&'static str> {
            vec![SimpleMethod::NAME, StreamingMethod::NAME]
        }
    }

    #[test]
    fn test_method_collection() {
        let schemas = TestMethodCollection::schemas();
        assert_eq!(schemas.len(), 2);
        assert_eq!(schemas[0].name, "simple_method");
        assert_eq!(schemas[1].name, "streaming_method");

        let names = TestMethodCollection::method_names();
        assert_eq!(names, vec!["simple_method", "streaming_method"]);
    }
}
