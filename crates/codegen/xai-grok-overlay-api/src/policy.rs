use serde::{Deserialize, Serialize};

/// Distribution mode resolved once during startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OverlayMode {
    /// No distribution overlay is active; preserve host/upstream behavior.
    Upstream,
    Open,
    XaiCompat,
}

impl Default for OverlayMode {
    fn default() -> Self {
        Self::Upstream
    }
}

impl OverlayMode {
    pub const fn is_upstream(self) -> bool {
        matches!(self, Self::Upstream)
    }

    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    pub const fn is_xai_compat(self) -> bool {
        matches!(self, Self::XaiCompat)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServiceKind {
    RemoteSettings,
    ManagedConfig,
    Telemetry,
    Feedback,
    TraceUpload,
    Relay,
    Billing,
    Subscription,
    Updates,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServicePolicy {
    Disabled,
    ExplicitOnly,
    Enabled,
}

impl ServicePolicy {
    pub const fn allows_implicit(self) -> bool {
        matches!(self, Self::Enabled)
    }

    pub const fn allows_explicit(self) -> bool {
        !matches!(self, Self::Disabled)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthPolicy {
    Inherited,
    ByokOnly,
    ProviderOrByok,
    FirstPartyOnly,
}

impl AuthPolicy {
    pub const fn allows_session_auth(self) -> bool {
        !matches!(self, Self::ByokOnly)
    }

    pub const fn allows_first_party_auth(self) -> bool {
        !matches!(self, Self::ByokOnly)
    }
}

/// Resolved service and authentication policy for one process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlayPolicy {
    pub mode: OverlayMode,
    pub auth: AuthPolicy,
    pub remote_settings: ServicePolicy,
    pub managed_config: ServicePolicy,
    pub telemetry: ServicePolicy,
    pub feedback: ServicePolicy,
    pub trace_upload: ServicePolicy,
    pub relay: ServicePolicy,
    pub billing: ServicePolicy,
    pub subscription: ServicePolicy,
    pub updates: ServicePolicy,
}

impl Default for OverlayPolicy {
    fn default() -> Self {
        Self::upstream()
    }
}

impl OverlayPolicy {
    pub const fn upstream() -> Self {
        Self {
            mode: OverlayMode::Upstream,
            auth: AuthPolicy::Inherited,
            remote_settings: ServicePolicy::Enabled,
            managed_config: ServicePolicy::Enabled,
            telemetry: ServicePolicy::Enabled,
            feedback: ServicePolicy::Enabled,
            trace_upload: ServicePolicy::Enabled,
            relay: ServicePolicy::Enabled,
            billing: ServicePolicy::Enabled,
            subscription: ServicePolicy::Enabled,
            updates: ServicePolicy::Enabled,
        }
    }

    pub const fn open() -> Self {
        Self {
            mode: OverlayMode::Open,
            auth: AuthPolicy::ByokOnly,
            remote_settings: ServicePolicy::ExplicitOnly,
            managed_config: ServicePolicy::Disabled,
            telemetry: ServicePolicy::Disabled,
            feedback: ServicePolicy::Disabled,
            trace_upload: ServicePolicy::Disabled,
            relay: ServicePolicy::Disabled,
            billing: ServicePolicy::Disabled,
            subscription: ServicePolicy::Disabled,
            updates: ServicePolicy::ExplicitOnly,
        }
    }

    pub const fn xai_compat() -> Self {
        Self {
            mode: OverlayMode::XaiCompat,
            auth: AuthPolicy::ProviderOrByok,
            remote_settings: ServicePolicy::Enabled,
            managed_config: ServicePolicy::Enabled,
            telemetry: ServicePolicy::Enabled,
            feedback: ServicePolicy::Enabled,
            trace_upload: ServicePolicy::Enabled,
            relay: ServicePolicy::Enabled,
            billing: ServicePolicy::Enabled,
            subscription: ServicePolicy::Enabled,
            updates: ServicePolicy::Enabled,
        }
    }

    pub const fn service(&self, kind: ServiceKind) -> ServicePolicy {
        match kind {
            ServiceKind::RemoteSettings => self.remote_settings,
            ServiceKind::ManagedConfig => self.managed_config,
            ServiceKind::Telemetry => self.telemetry,
            ServiceKind::Feedback => self.feedback,
            ServiceKind::TraceUpload => self.trace_upload,
            ServiceKind::Relay => self.relay,
            ServiceKind::Billing => self.billing,
            ServiceKind::Subscription => self.subscription,
            ServiceKind::Updates => self.updates,
        }
    }

    pub const fn allows_implicit(&self, kind: ServiceKind) -> bool {
        self.service(kind).allows_implicit()
    }

    pub const fn allows_explicit(&self, kind: ServiceKind) -> bool {
        self.service(kind).allows_explicit()
    }

    pub const fn allows_session_auth(&self) -> bool {
        self.auth.allows_session_auth()
    }

    /// Whether the host may perform an implicit first-party authentication
    /// operation such as token refresh or cached-session recovery.
    pub const fn allows_first_party_auth(&self) -> bool {
        self.auth.allows_first_party_auth()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn open_policy_fails_closed_for_first_party_services() {
        let policy = OverlayPolicy::open();

        assert!(policy.mode.is_open());
        assert_eq!(policy.auth, AuthPolicy::ByokOnly);
        assert!(!policy.allows_implicit(ServiceKind::Telemetry));
        assert!(!policy.allows_explicit(ServiceKind::Telemetry));
        assert!(!policy.allows_implicit(ServiceKind::ManagedConfig));
        assert!(!policy.allows_implicit(ServiceKind::Relay));
        assert!(policy.allows_explicit(ServiceKind::RemoteSettings));
        assert!(policy.allows_explicit(ServiceKind::Updates));
    }

    #[test]
    fn upstream_policy_preserves_host_defaults() {
        let policy = OverlayPolicy::upstream();

        assert!(policy.mode.is_upstream());
        assert_eq!(policy.auth, AuthPolicy::Inherited);
        assert!(policy.auth.allows_session_auth());
        assert!(policy.allows_implicit(ServiceKind::Telemetry));
        assert!(policy.allows_implicit(ServiceKind::Updates));
    }

    #[test]
    fn byok_policy_disables_session_auth() {
        assert!(!AuthPolicy::ByokOnly.allows_session_auth());
        assert!(AuthPolicy::ProviderOrByok.allows_session_auth());
    }

    #[test]
    fn compatibility_policy_enables_first_party_services() {
        let policy = OverlayPolicy::xai_compat();

        assert!(policy.mode.is_xai_compat());
        assert_eq!(policy.auth, AuthPolicy::ProviderOrByok);
        assert!(policy.allows_implicit(ServiceKind::Telemetry));
        assert!(policy.allows_implicit(ServiceKind::Subscription));
    }

    #[test]
    fn policy_serializes_as_stable_snake_case() {
        let json = serde_json::to_value(OverlayPolicy::open()).expect("serialize policy");

        assert_eq!(json["mode"], "open");
        assert_eq!(json["auth"], "byok_only");
        assert_eq!(json["remote_settings"], "explicit_only");
    }
}
