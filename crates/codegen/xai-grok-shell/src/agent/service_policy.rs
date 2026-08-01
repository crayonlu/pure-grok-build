//! Fork-owned service routing policy.
//!
//! The upstream shell has a number of first-party auxiliary services (remote
//! settings, managed config, voice, media, telemetry, and Grok cloud
//! features).  The fork keeps those implementations for merge compatibility,
//! but the default fork mode is deliberately open: an external model key must
//! never be sent to an implicit xAI endpoint.

use serde::{Deserialize, Serialize};

/// Top-level fork operating mode.
///
/// `open` is the safe default for this fork.  `xai_compat` preserves the
/// upstream first-party auxiliary services for users who explicitly opt in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForkMode {
    Open,
    XaiCompat,
}

impl Default for ForkMode {
    fn default() -> Self {
        Self::Open
    }
}

impl ForkMode {
    pub fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    pub fn allows_xai_compat(self) -> bool {
        matches!(self, Self::XaiCompat)
    }
}

/// Fork-owned `[fork]` configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ForkConfig {
    pub mode: ForkMode,
}

impl ForkConfig {
    /// Return the effective mode for this process.  `GROK_FORK_MODE` is a
    /// deliberate process-level override (useful for deployments and test
    /// harnesses); otherwise the deserialized `[fork].mode` wins.
    pub fn effective_mode(&self) -> ForkMode {
        std::env::var("GROK_FORK_MODE")
            .ok()
            .and_then(|raw| parse_mode(&raw))
            .unwrap_or(self.mode)
    }

    pub fn is_open(&self) -> bool {
        self.effective_mode().is_open()
    }

    pub fn allows_xai_compat(&self) -> bool {
        self.effective_mode().allows_xai_compat()
    }
}

/// Auxiliary services that may be routed to xAI by upstream compatibility
/// code.  Keeping this list centralized makes policy decisions auditable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceKind {
    RemoteFetch,
    ManagedConfig,
    ImageGeneration,
    VideoGeneration,
    Voice,
    WebSearch,
    Embeddings,
    Telemetry,
    Feedback,
    TraceUpload,
    Assets,
    Updates,
    CloudProduct,
}

impl ServiceKind {
    /// Whether this service is an xAI-only compatibility surface rather than
    /// a core provider-neutral capability.
    pub fn is_xai_compat_only(self) -> bool {
        matches!(
            self,
            Self::RemoteFetch
                | Self::ManagedConfig
                | Self::Voice
                | Self::Telemetry
                | Self::Feedback
                | Self::TraceUpload
                | Self::CloudProduct
                | Self::Assets
                | Self::Updates
        )
    }
}

/// Resolve the fork mode before an `AgentConfig` exists (startup prefetch runs
/// before the shell has built its effective config).  Environment wins over
/// the user config; malformed values fail closed to open mode.
pub fn mode_from_disk() -> ForkMode {
    if let Some(mode) = std::env::var("GROK_FORK_MODE")
        .ok()
        .and_then(|raw| parse_mode(&raw))
    {
        return mode;
    }

    let Ok(layers) = crate::config::ConfigLayers::load() else {
        return ForkMode::Open;
    };

    // Match the safety precedence used by remote-fetch: requirements and
    // managed policy outrank the user layer, so an administrator can force
    // open mode and a stale user file cannot silently re-arm xAI services.
    [
        layers.mdm_requirements.as_ref(),
        layers.system_requirements.as_ref(),
        layers.user_requirements.as_ref(),
        Some(&layers.managed),
        Some(&layers.system_managed),
        Some(&layers.user),
    ]
    .into_iter()
    .flatten()
    .find_map(fork_mode_value)
    .unwrap_or(ForkMode::Open)
}

fn fork_mode_value(value: &toml::Value) -> Option<ForkMode> {
    value
        .get("fork")
        .and_then(|fork| fork.get("mode"))
        .and_then(|mode| mode.as_str())
        .and_then(parse_mode)
}

fn parse_mode(raw: &str) -> Option<ForkMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" => Some(ForkMode::Open),
        "xai_compat" | "xai-compat" | "compat" => Some(ForkMode::XaiCompat),
        _ => None,
    }
}

/// Whether an upstream remote fetch is allowed when no explicit local setting
/// exists.  Open mode changes only the implicit default; an explicit
/// `[features] remote_fetch = true` remains an intentional opt-in.
pub fn default_remote_fetch_enabled() -> bool {
    !mode_from_disk().is_open()
}

/// Core safety predicate used by auxiliary service constructors.
pub fn allows_xai_compat(mode: ForkMode, service: ServiceKind) -> bool {
    !service.is_xai_compat_only() || mode.allows_xai_compat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_mode_is_open() {
        assert_eq!(ForkMode::default(), ForkMode::Open);
        assert!(ForkMode::Open.is_open());
        assert!(!ForkMode::Open.allows_xai_compat());
    }

    #[test]
    fn compatibility_mode_allows_first_party_surfaces() {
        assert!(allows_xai_compat(
            ForkMode::XaiCompat,
            ServiceKind::CloudProduct
        ));
        assert!(!allows_xai_compat(
            ForkMode::Open,
            ServiceKind::CloudProduct
        ));
    }

    #[test]
    fn non_first_party_services_are_not_blocked_by_mode() {
        assert!(allows_xai_compat(
            ForkMode::Open,
            ServiceKind::ImageGeneration
        ));
        assert!(allows_xai_compat(ForkMode::Open, ServiceKind::WebSearch));
    }

    #[test]
    fn parse_mode_accepts_compat_aliases() {
        assert_eq!(parse_mode("xai_compat"), Some(ForkMode::XaiCompat));
        assert_eq!(parse_mode("compat"), Some(ForkMode::XaiCompat));
        assert_eq!(parse_mode("unknown"), None);
    }
}
