//! Provider-neutral runtime overlay loader.
//!
//! The resolved value types live in [`xai_grok_overlay_api`]. This crate owns
//! only distribution-specific discovery of the effective config and process
//! environment, so host applications can depend on the API without depending
//! on this loader implementation.

pub use xai_grok_overlay_api::{OverlayConfigError, OverlayRuntime};

/// Resolve the overlay snapshot from the same effective TOML document used by
/// the host application.
pub fn load_runtime() -> Result<OverlayRuntime, OverlayConfigError> {
    let document = xai_grok_config::load_effective_config_disk_only().ok();
    OverlayRuntime::from_toml(document.as_ref(), |key| std::env::var(key).ok())
}
