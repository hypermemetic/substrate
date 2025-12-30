//! Plexus module - re-exported from hub-core
//!
//! This module re-exports all plexus types from hub-core to avoid duplication.
//! Substrate-specific activations use these types via this re-export.

// Re-export everything from hub-core's plexus module
pub use hub_core::plexus::*;

// Also re-export Handle from hub-core's types
pub use hub_core::types::Handle;
