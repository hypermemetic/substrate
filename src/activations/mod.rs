// Health is the reference implementation for the new architecture (manual impl)
pub mod health;

// Echo demonstrates hub-macro usage with the new architecture
pub mod echo;

// Solar demonstrates nested plugin hierarchy (plugins with children)
pub mod solar;

// Arbor manages conversation trees
pub mod arbor;

// Bash executes shell commands
pub mod bash;

// Cone orchestrates LLM conversations with Arbor context
pub mod cone;

// ClaudeCode manages Claude Code sessions with Arbor-backed history
pub mod claudecode;

// Mustache provides template rendering for handle values
pub mod mustache;

// Changelog tracks plexus hash changes and enforces documentation
pub mod changelog;
