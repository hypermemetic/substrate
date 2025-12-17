//! Session Schema - Extract runtime protocol schemas from compile-time session types
//!
//! This module provides the bridge between dialectic's session types and JSON Schema,
//! enabling full protocol introspection for client codegen.
//!
//! # Example
//!
//! ```ignore
//! use dialectic::prelude::*;
//! use substrate::plexus::session_schema::{SessionSchema, ProtocolSchema};
//!
//! // Define a streaming protocol
//! type ChatProtocol = Session! {
//!     send ChatInput;
//!     recv ChatStart;
//!     loop {
//!         offer {
//!             0 => { recv ChatContent; continue; },
//!             1 => { recv ChatComplete; break; },
//!         }
//!     }
//! };
//!
//! // Extract the schema at runtime
//! let schema = <ChatProtocol>::schema();
//! ```

use dialectic::tuple::Tuple;
use dialectic::types::{Choose, Continue, Done, Loop, Offer, Recv, Send};
use schemars::{schema_for, JsonSchema};
use serde::{Deserialize, Serialize};
use serde_json::Value;

// ============================================================================
// Protocol Schema Types - describes the shape of a session protocol
// ============================================================================

/// Runtime representation of a session protocol.
///
/// This enum mirrors dialectic's session type constructors but as runtime data,
/// enabling serialization for introspection and client codegen.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ProtocolSchema {
    /// Send a value of this type, then continue with the next protocol step.
    ///
    /// From the client's perspective, this means "I send data".
    /// From the server's perspective (dual), this becomes Recv.
    Send {
        /// JSON Schema describing the payload type
        payload: Value,
        /// The protocol continuation after sending
        then: Box<ProtocolSchema>,
    },

    /// Receive a value of this type, then continue with the next protocol step.
    ///
    /// From the client's perspective, this means "I receive data".
    /// From the server's perspective (dual), this becomes Send.
    Recv {
        /// JSON Schema describing the expected payload type
        payload: Value,
        /// The protocol continuation after receiving
        then: Box<ProtocolSchema>,
    },

    /// Offer multiple branches - the peer will choose which branch to take.
    ///
    /// Used when the server presents options and the client selects.
    /// Common pattern for streaming: offer { content | complete }
    Offer {
        /// The available branches the peer can choose from
        branches: Vec<ProtocolSchema>,
    },

    /// Choose from multiple branches - we select which branch to take.
    ///
    /// Used when we decide which path the protocol takes.
    Choose {
        /// The branches we can select from
        branches: Vec<ProtocolSchema>,
    },

    /// Loop construct for repeated protocol patterns.
    ///
    /// The body will be executed repeatedly until a branch breaks out
    /// (via a Done continuation instead of Continue).
    Loop {
        /// The protocol body to repeat
        body: Box<ProtocolSchema>,
    },

    /// Continue to the Nth enclosing loop.
    ///
    /// Depth 0 means continue to the innermost loop,
    /// depth 1 means skip one loop and continue to the next outer, etc.
    Continue {
        /// The loop nesting depth (0 = innermost)
        depth: usize,
    },

    /// Session complete - no more protocol steps.
    Done,
}

// ============================================================================
// SessionSchema Trait - extracts schema from session types
// ============================================================================

/// Extract a runtime protocol schema from a compile-time session type.
///
/// This trait is implemented for all dialectic session type constructors,
/// allowing any session type to be converted to its JSON Schema representation.
pub trait SessionSchema {
    /// Generate the protocol schema for this session type.
    fn schema() -> ProtocolSchema;
}

// Done - terminal
impl SessionSchema for Done {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Done
    }
}

// Send<T, P> - send T then continue with P
impl<T: JsonSchema + 'static, P: SessionSchema + 'static> SessionSchema for Send<T, P> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Send {
            payload: serde_json::to_value(schema_for!(T)).unwrap(),
            then: Box::new(P::schema()),
        }
    }
}

// Recv<T, P> - receive T then continue with P
impl<T: JsonSchema + 'static, P: SessionSchema + 'static> SessionSchema for Recv<T, P> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Recv {
            payload: serde_json::to_value(schema_for!(T)).unwrap(),
            then: Box::new(P::schema()),
        }
    }
}

// Loop<P> - loop over P
impl<P: SessionSchema + 'static> SessionSchema for Loop<P> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Loop {
            body: Box::new(P::schema()),
        }
    }
}

// Continue<N> - continue to Nth enclosing loop
// We need to handle const generic usize values
impl SessionSchema for Continue<0> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Continue { depth: 0 }
    }
}

impl SessionSchema for Continue<1> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Continue { depth: 1 }
    }
}

impl SessionSchema for Continue<2> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Continue { depth: 2 }
    }
}

impl SessionSchema for Continue<3> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Continue { depth: 3 }
    }
}

impl SessionSchema for Continue<4> {
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Continue { depth: 4 }
    }
}

// ============================================================================
// ListSchema - Helper for Offer/Choose branch extraction
// ============================================================================

/// Extract schemas from dialectic's inductive type-level lists.
///
/// Dialectic represents choice branches as inductive lists: `(A, (B, (C, ())))`
/// This trait allows us to traverse that structure and collect schemas.
pub trait ListSchema {
    /// Collect schemas from all elements in this type-level list.
    fn schemas() -> Vec<ProtocolSchema>;
}

// Base case: empty list
impl ListSchema for () {
    fn schemas() -> Vec<ProtocolSchema> {
        vec![]
    }
}

// Inductive case: (Head, Tail)
impl<P: SessionSchema + 'static, Rest: ListSchema + 'static> ListSchema for (P, Rest) {
    fn schemas() -> Vec<ProtocolSchema> {
        let mut v = vec![P::schema()];
        v.extend(Rest::schemas());
        v
    }
}

// ============================================================================
// Offer and Choose implementations
// ============================================================================

// Offer<Choices> - offer multiple branches
// Dialectic uses flat tuples (A, B) externally but converts to (A, (B, ())) internally
impl<Choices> SessionSchema for Offer<Choices>
where
    Choices: Tuple + 'static,
    Choices::AsList: ListSchema,
{
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Offer {
            branches: <Choices::AsList as ListSchema>::schemas(),
        }
    }
}

// Choose<Choices> - choose from branches
impl<Choices> SessionSchema for Choose<Choices>
where
    Choices: Tuple + 'static,
    Choices::AsList: ListSchema,
{
    fn schema() -> ProtocolSchema {
        ProtocolSchema::Choose {
            branches: <Choices::AsList as ListSchema>::schemas(),
        }
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use dialectic::prelude::*;

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TestInput {
        id: String,
        value: i32,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct TestOutput {
        result: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct StreamChunk {
        content: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
    struct StreamComplete {
        total: i32,
    }

    #[test]
    fn test_simple_protocol() {
        type SimpleProtocol = Session! {
            send TestInput;
            recv TestOutput;
        };

        let schema = <SimpleProtocol>::schema();

        match schema {
            ProtocolSchema::Send { then, .. } => match *then {
                ProtocolSchema::Recv { then, .. } => {
                    assert_eq!(*then, ProtocolSchema::Done);
                }
                _ => panic!("Expected Recv after Send"),
            },
            _ => panic!("Expected Send first"),
        }
    }

    #[test]
    fn test_streaming_protocol() {
        type StreamingProtocol = Session! {
            send TestInput;
            loop {
                offer {
                    0 => { recv StreamChunk; continue; },
                    1 => { recv StreamComplete; break; },
                }
            }
        };

        let schema = <StreamingProtocol>::schema();

        // Verify structure: Send -> Loop -> Offer
        match schema {
            ProtocolSchema::Send { then, .. } => match *then {
                ProtocolSchema::Loop { body } => match *body {
                    ProtocolSchema::Offer { branches } => {
                        assert_eq!(branches.len(), 2);
                        // Branch 0: Recv -> Continue
                        match &branches[0] {
                            ProtocolSchema::Recv { then, .. } => {
                                assert!(matches!(**then, ProtocolSchema::Continue { depth: 0 }));
                            }
                            _ => panic!("Branch 0 should be Recv"),
                        }
                        // Branch 1: Recv -> Done (break)
                        match &branches[1] {
                            ProtocolSchema::Recv { then, .. } => {
                                assert!(matches!(**then, ProtocolSchema::Done));
                            }
                            _ => panic!("Branch 1 should be Recv"),
                        }
                    }
                    _ => panic!("Expected Offer in loop body"),
                },
                _ => panic!("Expected Loop after Send"),
            },
            _ => panic!("Expected Send first"),
        }
    }

    #[test]
    fn test_dual_protocol() {
        type ClientProtocol = Session! {
            send TestInput;
            recv TestOutput;
        };

        type ServerProtocol = <ClientProtocol as Session>::Dual;

        let client_schema = <ClientProtocol>::schema();
        let server_schema = <ServerProtocol>::schema();

        // Client: Send -> Recv
        // Server: Recv -> Send (dual flips directions)
        match (&client_schema, &server_schema) {
            (ProtocolSchema::Send { .. }, ProtocolSchema::Recv { .. }) => {}
            _ => panic!("Dual should flip Send to Recv"),
        }
    }

    #[test]
    fn test_schema_serialization() {
        type SimpleProtocol = Session! {
            send TestInput;
            recv TestOutput;
        };

        let schema = <SimpleProtocol>::schema();
        let json = serde_json::to_string_pretty(&schema).unwrap();

        // Verify it can round-trip
        let parsed: ProtocolSchema = serde_json::from_str(&json).unwrap();
        assert_eq!(schema, parsed);
    }

    #[test]
    fn test_payload_contains_json_schema() {
        type SimpleProtocol = Session! {
            send TestInput;
            recv TestOutput;
        };

        let schema = <SimpleProtocol>::schema();

        match schema {
            ProtocolSchema::Send { payload, .. } => {
                // Verify the payload is a JSON Schema
                assert!(payload.get("type").is_some() || payload.get("$schema").is_some());
                // Should have properties for TestInput fields
                if let Some(props) = payload.get("properties") {
                    assert!(props.get("id").is_some());
                    assert!(props.get("value").is_some());
                }
            }
            _ => panic!("Expected Send"),
        }
    }
}
