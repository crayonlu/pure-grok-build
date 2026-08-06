//! Provider-neutral runtime overlay.
//!
//! This crate owns distribution policy resolution. It intentionally depends
//! only on [`xai_grok_overlay_api`] and configuration serialization crates:
//! it must not depend on the upstream shell, pager, or tool implementation.
//! Host applications consume the resulting snapshot through the API crate.

use std::collections::BTreeMap;

use serde::Deserialize;
use xai_grok_overlay_api::{
    AuthPolicy, Capability, CapabilityAvailability, CapabilityProviderRef, CapabilitySet,
    EntitlementPolicy, OverlayMode, OverlayPolicy, UpdateChannel, UpdateSourceRef,
};

/// Resolve the overlay snapshot from the same effective TOML document used by
/// the host application.
pub fn load_runtime() -> Result<OverlayRuntime, OverlayConfigError> {
    let document = xai_grok_config::load_effective_config_disk_only().ok();
    OverlayRuntime::from_toml(document.as_ref(), |key| std::env::var(key).ok())
}

/// Errors raised while resolving the distribution overlay.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OverlayConfigError {
    InvalidMode(String),
    InvalidUpdateSource(String),
    InvalidCapability(String),
}

impl std::fmt::Display for OverlayConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidMode(value) => write!(f, "invalid overlay mode `{value}`"),
            Self::InvalidUpdateSource(value) => {
                write!(f, "invalid overlay update source `{value}`")
            }
            Self::InvalidCapability(value) => write!(f, "invalid overlay capability `{value}`"),
        }
    }
}

impl std::error::Error for OverlayConfigError {}

/// Fully resolved, immutable overlay state for one process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayRuntime {
    policy: OverlayPolicy,
    entitlement: EntitlementPolicy,
    capabilities: CapabilitySet,
    update_source: Option<UpdateSourceRef>,
}

impl Default for OverlayRuntime {
    fn default() -> Self {
        Self::open()
    }
}

impl OverlayRuntime {
    pub fn open() -> Self {
        Self {
            policy: OverlayPolicy::open(),
            entitlement: EntitlementPolicy::provider_neutral(),
            capabilities: CapabilitySet::disabled(),
            update_source: None,
        }
    }

    pub fn xai_compat() -> Self {
        Self {
            policy: OverlayPolicy::xai_compat(),
            entitlement: EntitlementPolicy::first_party(),
            capabilities: CapabilitySet::disabled(),
            update_source: None,
        }
    }

    /// Resolve `[overlay]` from a parsed config document.
    ///
    /// Environment overrides are intentionally supplied by the caller. This
    /// makes precedence deterministic and keeps tests independent of the
    /// process environment.
    pub fn from_toml(
        document: Option<&toml::Value>,
        getenv: impl Fn(&str) -> Option<String>,
    ) -> Result<Self, OverlayConfigError> {
        let file = document
            .and_then(|document| document.get("overlay"))
            .cloned()
            .map(toml::Value::try_into::<FileOverlayConfig>)
            .transpose()
            .map_err(|error| OverlayConfigError::InvalidMode(error.to_string()))?
            .unwrap_or_default();

        let mode = getenv("GROK_OVERLAY_MODE")
            .or(file.mode)
            .map(|value| parse_mode(&value))
            .transpose()?
            .unwrap_or_default();

        let mut runtime = if mode.is_open() {
            Self::open()
        } else {
            Self::xai_compat()
        };

        runtime.capabilities = capability_set(file.capabilities)?;
        runtime.update_source = resolve_update_source(
            file.update_source,
            getenv("GROK_UPDATE_REPO"),
            getenv("GROK_CLI_BASE_URL"),
        )?;

        Ok(runtime)
    }

    pub fn policy(&self) -> OverlayPolicy {
        self.policy.clone()
    }

    pub fn auth_policy(&self) -> AuthPolicy {
        self.policy.auth
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

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct FileOverlayConfig {
    mode: Option<String>,
    update_source: Option<FileUpdateSource>,
    capabilities: BTreeMap<String, FileCapabilityProvider>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct FileUpdateSource {
    kind: String,
    location: String,
    channel: UpdateChannel,
}

impl Default for FileUpdateSource {
    fn default() -> Self {
        Self {
            kind: String::new(),
            location: String::new(),
            channel: UpdateChannel::Stable,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
struct FileCapabilityProvider {
    name: String,
    protocol: String,
    base_url: Option<String>,
}

fn parse_mode(raw: &str) -> Result<OverlayMode, OverlayConfigError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" | "byok" | "provider_neutral" => Ok(OverlayMode::Open),
        "xai_compat" | "xai-compat" | "compat" => Ok(OverlayMode::XaiCompat),
        _ => Err(OverlayConfigError::InvalidMode(raw.to_owned())),
    }
}

fn capability_set(
    providers: BTreeMap<String, FileCapabilityProvider>,
) -> Result<CapabilitySet, OverlayConfigError> {
    providers
        .into_iter()
        .try_fold(CapabilitySet::disabled(), |set, (name, provider)| {
            let capability = parse_capability(&name)?;
            if provider.name.trim().is_empty() || provider.protocol.trim().is_empty() {
                return Err(OverlayConfigError::InvalidCapability(name));
            }
            Ok(set.with_provider(
                capability,
                CapabilityProviderRef::new(provider.name, provider.protocol, provider.base_url),
            ))
        })
}

fn parse_capability(raw: &str) -> Result<Capability, OverlayConfigError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "chat" => Ok(Capability::Chat),
        "embeddings" | "embedding" => Ok(Capability::Embeddings),
        "web_search" | "web-search" => Ok(Capability::WebSearch),
        "web_fetch" | "web-fetch" => Ok(Capability::WebFetch),
        "image_generation" | "image-generation" | "image_gen" => Ok(Capability::ImageGeneration),
        "image_editing" | "image-editing" | "image_edit" => Ok(Capability::ImageEditing),
        "video_generation" | "video-generation" | "video_gen" => Ok(Capability::VideoGeneration),
        "voice" => Ok(Capability::Voice),
        _ => Err(OverlayConfigError::InvalidCapability(raw.to_owned())),
    }
}

fn resolve_update_source(
    file: Option<FileUpdateSource>,
    repo: Option<String>,
    base_url: Option<String>,
) -> Result<Option<UpdateSourceRef>, OverlayConfigError> {
    if let Some(repo) = repo.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(UpdateSourceRef::github_release(
            repo,
            UpdateChannel::Stable,
        )));
    }
    if let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(UpdateSourceRef::base_url(
            base_url,
            UpdateChannel::Stable,
        )));
    }
    let Some(file) = file else {
        return Ok(None);
    };
    if file.kind.trim().is_empty() || file.location.trim().is_empty() {
        return Err(OverlayConfigError::InvalidUpdateSource(
            "kind and location are required".to_owned(),
        ));
    }
    if !matches!(file.kind.as_str(), "github_release" | "base_url") {
        return Err(OverlayConfigError::InvalidUpdateSource(file.kind));
    }
    Ok(Some(UpdateSourceRef {
        kind: file.kind,
        location: file.location,
        channel: file.channel,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(entries: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            entries
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn default_runtime_is_open_and_has_no_implicit_capabilities() {
        let runtime = OverlayRuntime::from_toml(None, env(&[])).expect("resolve defaults");

        assert_eq!(runtime.auth_policy(), AuthPolicy::ByokOnly);
        assert_eq!(
            runtime.capability(Capability::ImageGeneration),
            CapabilityAvailability::Disabled
        );
        assert!(runtime.update_source().is_none());
    }

    #[test]
    fn environment_mode_overrides_file_mode() {
        let document: toml::Value = toml::toml! {
            [overlay]
            mode = "xai_compat"
        }
        .into();
        let runtime =
            OverlayRuntime::from_toml(Some(&document), env(&[("GROK_OVERLAY_MODE", "open")]))
                .expect("resolve mode");

        assert_eq!(runtime.auth_policy(), AuthPolicy::ByokOnly);
        assert!(!runtime.entitlement().show_billing);
    }

    #[test]
    fn file_capabilities_and_update_source_are_resolved() {
        let document: toml::Value = toml::toml! {
            [overlay]
            mode = "open"
            [overlay.capabilities.image_generation]
            name = "local-images"
            protocol = "generic_http"
            base_url = "https://images.example.test/v1"
            [overlay.update_source]
            kind = "base_url"
            location = "https://downloads.example.test/grok"
            channel = "nightly"
        }
        .into();
        let runtime = OverlayRuntime::from_toml(Some(&document), env(&[])).expect("resolve config");

        assert!(matches!(
            runtime.capability(Capability::ImageGeneration),
            CapabilityAvailability::Provider(ref provider)
                if provider.name == "local-images"
        ));
        assert_eq!(
            runtime
                .update_source()
                .map(|source| source.location.as_str()),
            Some("https://downloads.example.test/grok")
        );
    }

    #[test]
    fn update_environment_overrides_file_source() {
        let document: toml::Value = toml::toml! {
            [overlay.update_source]
            kind = "base_url"
            location = "https://file.example.test"
        }
        .into();
        let runtime = OverlayRuntime::from_toml(
            Some(&document),
            env(&[("GROK_CLI_BASE_URL", "https://env.example.test")]),
        )
        .expect("resolve update source");

        assert_eq!(
            runtime
                .update_source()
                .map(|source| source.location.as_str()),
            Some("https://env.example.test")
        );
    }
}
