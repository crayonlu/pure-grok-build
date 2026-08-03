//! Compatibility re-export for the provider-neutral HTTP runtime.
//!
//! The implementation lives in `xai-grok-provider`, a dependency leaf shared
//! by tools, memory, and shell configuration. Keeping this module preserves
//! the old `xai_grok_http::provider::*` paths for downstream callers.

pub use xai_grok_provider::*;
