//! Stable, upstream-independent extension seams for Grok distributions.
//!
//! This crate deliberately has no dependency on `xai-grok-shell`, the pager,
//! or the upstream tool implementation. It contains only resolved policy and
//! provider-facing value types. That keeps distribution-specific behavior out
//! of upstream application control flow and gives an overlay a narrow API to
//! integrate against.

mod capabilities;
mod entitlement;
mod policy;
mod runtime;
mod updates;

pub use capabilities::{Capability, CapabilityAvailability, CapabilityProviderRef, CapabilitySet};
pub use entitlement::{EntitlementPolicy, EntitlementState};
pub use policy::{AuthPolicy, OverlayMode, OverlayPolicy, ServiceKind, ServicePolicy};
pub use runtime::OverlayRuntime;
pub use updates::{UpdateChannel, UpdateSourceRef};
