use crate::{
    AuthPolicy, Capability, CapabilityAvailability, CapabilitySet, EntitlementPolicy,
    OverlayPolicy, UpdateSourceRef,
};

/// Fully resolved, immutable overlay state for one process.
///
/// The API crate owns this value and the policy decisions derived from it.
/// Distribution-specific config discovery belongs to the loader crate, which
/// keeps host applications independent from a particular overlay format.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayRuntime {
    policy: OverlayPolicy,
    entitlement: EntitlementPolicy,
    capabilities: CapabilitySet,
    update_source: Option<UpdateSourceRef>,
}

impl Default for OverlayRuntime {
    fn default() -> Self {
        // Preserve the neutral API crate's upstream-compatible default. Fork
        // composition roots explicitly use `OverlayRuntime::open()` when the
        // distribution overlay cannot be loaded.
        Self::upstream()
    }
}

impl OverlayRuntime {
    pub fn open() -> Self {
        Self::from_parts(
            OverlayPolicy::open(),
            EntitlementPolicy::provider_neutral(),
            CapabilitySet::disabled(),
            None,
        )
    }

    pub fn xai_compat() -> Self {
        Self::from_parts(
            OverlayPolicy::xai_compat(),
            EntitlementPolicy::first_party(),
            CapabilitySet::inherited(),
            None,
        )
    }

    pub fn upstream() -> Self {
        Self::from_parts(
            OverlayPolicy::upstream(),
            EntitlementPolicy::first_party(),
            CapabilitySet::inherited(),
            None,
        )
    }

    /// Construct a runtime snapshot from already-resolved provider-neutral
    /// values. Config parsing and environment precedence are intentionally not
    /// part of this API.
    pub fn from_parts(
        policy: OverlayPolicy,
        entitlement: EntitlementPolicy,
        capabilities: CapabilitySet,
        update_source: Option<UpdateSourceRef>,
    ) -> Self {
        Self {
            policy,
            entitlement,
            capabilities,
            update_source,
        }
    }

    pub fn policy(&self) -> OverlayPolicy {
        self.policy.clone()
    }

    pub fn auth_policy(&self) -> AuthPolicy {
        self.policy.auth
    }

    /// Whether session credentials may be read, refreshed, or used by the
    /// host's normal authentication path.
    pub const fn allows_session_auth(&self) -> bool {
        self.policy.allows_session_auth()
    }

    /// Whether first-party authentication is allowed for auxiliary services.
    pub const fn allows_first_party_auth(&self) -> bool {
        self.policy.allows_first_party_auth()
    }

    /// Whether an implicit auxiliary service may run.
    pub const fn allows_implicit(&self, kind: crate::ServiceKind) -> bool {
        self.policy.allows_implicit(kind)
    }

    /// Whether an explicitly requested auxiliary service may run.
    pub const fn allows_explicit(&self, kind: crate::ServiceKind) -> bool {
        self.policy.allows_explicit(kind)
    }

    pub fn entitlement(&self) -> EntitlementPolicy {
        self.entitlement
    }

    pub fn capabilities(&self) -> &CapabilitySet {
        &self.capabilities
    }

    pub fn capability(&self, capability: Capability) -> CapabilityAvailability {
        self.capabilities.get(capability)
    }

    pub fn update_source(&self) -> Option<&UpdateSourceRef> {
        self.update_source.as_ref()
    }
}
