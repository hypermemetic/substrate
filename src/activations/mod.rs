// Activations pruned for caller-wraps streaming architecture refactor.
// These will be migrated one at a time to use wrap_stream pattern.
// See docs/architecture/16680179837700061695_caller-wraps-streaming.md

// pub mod arbor;
// pub mod bash;
// pub mod claudecode;
// pub mod cone;

// Health is the reference implementation for the new architecture (manual impl)
pub mod health;

// Echo demonstrates hub-macro usage with the new architecture
pub mod echo;
