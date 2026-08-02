//! Compatibility re-export for provider-neutral capability configuration.
//!
//! The actual leaf types live in `xai-grok-provider` so low-level HTTP clients
//! and tools can use them without depending on this aggregate config crate.

pub use xai_grok_provider::*;
