//! Compatibility facade for the legacy fork service-policy API.
//!
//! Policy resolution now belongs to `xai-grok-overlay` and the stable
//! `xai-grok-overlay-api` seam.  This small module keeps the old public names
//! available to downstream integrations without reintroducing policy logic
//! into the upstream shell.

use serde::{Deserialize, Serialize};

/// Legacy fork operating mode.
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
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    pub const fn allows_xai_compat(self) -> bool {
        matches!(self, Self::XaiCompat)
    }
}

/// Legacy `[fork]` configuration shape. New code should use `[overlay]`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ForkConfig {
    pub mode: ForkMode,
}

impl ForkConfig {
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

/// Legacy service classification retained for source compatibility.
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
    pub const fn is_xai_compat_only(self) -> bool {
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

/// Resolve the legacy mode using the authoritative overlay loader.
pub fn mode_from_disk() -> ForkMode {
    match xai_grok_overlay::load_runtime()
        .map(|runtime| runtime.policy().mode)
        .unwrap_or(xai_grok_overlay_api::OverlayMode::Open)
    {
        xai_grok_overlay_api::OverlayMode::XaiCompat
        | xai_grok_overlay_api::OverlayMode::Upstream => ForkMode::XaiCompat,
        xai_grok_overlay_api::OverlayMode::Open => ForkMode::Open,
    }
}

pub fn default_remote_fetch_enabled() -> bool {
    !mode_from_disk().is_open()
}

pub const fn allows_xai_compat(mode: ForkMode, service: ServiceKind) -> bool {
    !service.is_xai_compat_only() || mode.allows_xai_compat()
}

fn parse_mode(raw: &str) -> Option<ForkMode> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" | "byok" | "provider_neutral" => Some(ForkMode::Open),
        "xai_compat" | "xai-compat" | "compat" | "upstream" => Some(ForkMode::XaiCompat),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_defaults_are_fail_closed() {
        assert_eq!(ForkMode::default(), ForkMode::Open);
        assert!(ForkMode::Open.is_open());
        assert!(!ForkMode::Open.allows_xai_compat());
    }

    #[test]
    fn aliases_parse_without_reintroducing_vendor_logic() {
        assert_eq!(parse_mode("xai_compat"), Some(ForkMode::XaiCompat));
        assert_eq!(parse_mode("provider_neutral"), Some(ForkMode::Open));
        assert_eq!(parse_mode("unknown"), None);
    }
}
