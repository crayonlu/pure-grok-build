//! Provider-neutral runtime overlay loader.
//!
//! This crate owns distribution-specific config discovery and precedence.
//! The host application consumes only the provider-neutral snapshot exported
//! by `xai-grok-overlay-api`.

use std::collections::BTreeMap;

use serde::Deserialize;
use xai_grok_overlay_api::{
    Capability, CapabilityProviderRef, CapabilitySet, EntitlementPolicy, OverlayMode,
    OverlayPolicy, OverlayRuntime, UpdateChannel, UpdateSourceRef,
};

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

/// Resolve the overlay snapshot from the same effective TOML document used by
/// the host application.
pub fn load_runtime() -> Result<OverlayRuntime, OverlayConfigError> {
    let document = xai_grok_config::load_effective_config_disk_only().ok();
    resolve_runtime(document.as_ref(), |key| std::env::var(key).ok())
}

/// Resolve an overlay snapshot from an already-loaded config document.
pub fn resolve_runtime(
    document: Option<&toml::Value>,
    getenv: impl Fn(&str) -> Option<String>,
) -> Result<OverlayRuntime, OverlayConfigError> {
    let overlay_document = document.and_then(|document| document.get("overlay"));
    // `[fork]` and `GROK_FORK_MODE` were the public compatibility surface of
    // the previous fork.  Keep accepting them while the host parser only sees
    // the upstream-neutral document.  Overlay settings win over legacy
    // settings, and the fork remains fail-closed (Open) when neither exists.
    let legacy_mode = document
        .and_then(|document| document.get("fork"))
        .and_then(|fork| fork.get("mode"))
        .and_then(toml::Value::as_str)
        .map(str::to_owned);
    let file = overlay_document
        .cloned()
        .map(toml::Value::try_into::<FileOverlayConfig>)
        .transpose()
        .map_err(|error| OverlayConfigError::InvalidMode(error.to_string()))?
        .unwrap_or_default();

    let explicit_mode = getenv("GROK_OVERLAY_MODE")
        .or(file.mode.clone())
        .or_else(|| {
            overlay_document
                .is_none()
                .then(|| getenv("GROK_FORK_MODE"))
                .flatten()
        })
        .or_else(|| overlay_document.is_none().then_some(legacy_mode).flatten());
    let mode = explicit_mode
        .map(|value| parse_mode(&value))
        .transpose()?
        .unwrap_or(OverlayMode::Open);

    let (policy, entitlement, default_capabilities) = match mode {
        OverlayMode::Upstream => (
            OverlayPolicy::upstream(),
            EntitlementPolicy::first_party(),
            CapabilitySet::inherited(),
        ),
        OverlayMode::Open => (
            OverlayPolicy::open(),
            EntitlementPolicy::provider_neutral(),
            CapabilitySet::disabled(),
        ),
        OverlayMode::XaiCompat => (
            OverlayPolicy::xai_compat(),
            EntitlementPolicy::first_party(),
            // Compatibility mode preserves the host's native xAI capability
            // drivers unless a provider-neutral profile explicitly overrides
            // one of them. Open mode remains fail-closed below.
            CapabilitySet::inherited(),
        ),
    };

    let capabilities =
        if (mode.is_upstream() || mode.is_xai_compat()) && file.capabilities.is_empty() {
            default_capabilities
        } else {
            capability_set(file.capabilities)?
        };
    let update_source = resolve_update_source(
        file.update_source,
        getenv("GROK_UPDATE_REPO"),
        getenv("GROK_CLI_BASE_URL"),
    )?;
    if let Some(source) = update_source.as_ref() {
        validate_update_source(source, mode)?;
    }

    Ok(OverlayRuntime::from_parts(
        policy,
        entitlement,
        capabilities,
        update_source,
    ))
}

/// Return the host config document with the overlay table removed.
///
/// The upstream config parser should not need to know the fork's `[overlay]`
/// schema. The loader consumes that table first, then the host parses this
/// sanitized document using its native config schema.
pub fn without_overlay(document: &toml::Value) -> toml::Value {
    let mut document = document.clone();
    if let toml::Value::Table(table) = &mut document {
        table.remove("overlay");
        // The legacy fork table is consumed by this loader as well.  Removing
        // it keeps the upstream parser independent of fork-only schema.
        table.remove("fork");
    }
    document
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
    #[serde(flatten)]
    profile: xai_grok_provider::CapabilityProviderConfig,
}

fn parse_mode(raw: &str) -> Result<OverlayMode, OverlayConfigError> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "open" | "byok" | "provider_neutral" => Ok(OverlayMode::Open),
        "upstream" | "native" => Ok(OverlayMode::Upstream),
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
            if provider.name.trim().is_empty() || provider.profile.protocol.trim().is_empty() {
                return Err(OverlayConfigError::InvalidCapability(name));
            }
            provider
                .profile
                .validate()
                .map_err(|_| OverlayConfigError::InvalidCapability(name.clone()))?;
            Ok(set.with_provider(
                capability,
                CapabilityProviderRef::from_profile(provider.name, provider.profile),
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
            repo.trim(),
            UpdateChannel::Stable,
        )));
    }
    if let Some(base_url) = base_url.filter(|value| !value.trim().is_empty()) {
        return Ok(Some(UpdateSourceRef::base_url(
            base_url.trim().trim_end_matches('/'),
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
        location: file.location.trim().trim_end_matches('/').to_owned(),
        channel: file.channel,
    }))
}

fn validate_update_source(
    source: &UpdateSourceRef,
    mode: OverlayMode,
) -> Result<(), OverlayConfigError> {
    match source.kind.as_str() {
        "github_release" => {
            let mut parts = source.location.split('/');
            let owner = parts.next().unwrap_or_default();
            let repository = parts.next().unwrap_or_default();
            if owner.is_empty()
                || repository.is_empty()
                || parts.next().is_some()
                || source.location.chars().any(char::is_whitespace)
            {
                return Err(OverlayConfigError::InvalidUpdateSource(
                    "github_release location must be an owner/repository pair".to_owned(),
                ));
            }
            if mode.is_open()
                && (owner.eq_ignore_ascii_case("xai-org")
                    || owner.eq_ignore_ascii_case("xai-org-shared"))
            {
                return Err(OverlayConfigError::InvalidUpdateSource(
                    "Open mode cannot use an xAI GitHub release source".to_owned(),
                ));
            }
        }
        "base_url" => {
            let url = url::Url::parse(&source.location).map_err(|error| {
                OverlayConfigError::InvalidUpdateSource(format!("invalid base URL: {error}"))
            })?;
            if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
                return Err(OverlayConfigError::InvalidUpdateSource(
                    "base_url must use http or https and include a host".to_owned(),
                ));
            }
            if !url.username().is_empty() || url.password().is_some() {
                return Err(OverlayConfigError::InvalidUpdateSource(
                    "base_url must not contain embedded credentials".to_owned(),
                ));
            }
            if mode.is_open() && is_xai_first_party_host(url.host_str().unwrap_or_default()) {
                return Err(OverlayConfigError::InvalidUpdateSource(
                    "Open mode cannot use an xAI update endpoint".to_owned(),
                ));
            }
        }
        kind => {
            return Err(OverlayConfigError::InvalidUpdateSource(kind.to_owned()));
        }
    }
    Ok(())
}

fn is_xai_first_party_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    host == "x.ai"
        || host.ends_with(".x.ai")
        || host == "grok.com"
        || host.ends_with(".grok.com")
        || host == "api.x.ai"
}

#[cfg(test)]
mod tests {
    use super::*;
    use xai_grok_overlay_api::{AuthPolicy, CapabilityAvailability};

    fn env<'a>(entries: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |key| {
            entries
                .iter()
                .find(|(name, _)| *name == key)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn no_overlay_preserves_open_fork_defaults() {
        let runtime = resolve_runtime(None, env(&[])).expect("resolve defaults");

        assert_eq!(runtime.policy().mode, OverlayMode::Open);
        assert_eq!(runtime.auth_policy(), AuthPolicy::ByokOnly);
        assert_eq!(
            runtime.capability(Capability::ImageGeneration),
            CapabilityAvailability::Disabled
        );
        assert!(runtime.update_source().is_none());
    }

    #[test]
    fn explicit_overlay_table_defaults_to_open_mode() {
        let document: toml::Value = toml::toml! {
            [overlay]
        }
        .into();
        let runtime = resolve_runtime(Some(&document), env(&[])).expect("resolve overlay");

        assert_eq!(runtime.policy().mode, OverlayMode::Open);
        assert_eq!(runtime.auth_policy(), AuthPolicy::ByokOnly);
        assert_eq!(
            runtime.capability(Capability::ImageGeneration),
            CapabilityAvailability::Disabled
        );
    }

    #[test]
    fn environment_mode_overrides_file_mode() {
        let document: toml::Value = toml::toml! {
            [overlay]
            mode = "xai_compat"
        }
        .into();
        let runtime = resolve_runtime(Some(&document), env(&[("GROK_OVERLAY_MODE", "open")]))
            .expect("resolve mode");

        assert_eq!(runtime.auth_policy(), AuthPolicy::ByokOnly);
        assert!(!runtime.entitlement().show_billing);
    }

    #[test]
    fn xai_compat_preserves_native_capabilities_without_profiles() {
        let document: toml::Value = toml::toml! {
            [overlay]
            mode = "xai_compat"
        }
        .into();
        let runtime = resolve_runtime(Some(&document), env(&[])).expect("resolve compat mode");

        assert_eq!(
            runtime.capability(Capability::ImageGeneration),
            CapabilityAvailability::Inherited
        );
        assert_eq!(
            runtime.capability(Capability::WebSearch),
            CapabilityAvailability::Inherited
        );
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
        let runtime = resolve_runtime(Some(&document), env(&[])).expect("resolve config");

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
        let runtime = resolve_runtime(
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

    #[test]
    fn open_mode_rejects_first_party_update_sources() {
        let document: toml::Value = toml::toml! {
            [overlay]
            mode = "open"
            [overlay.update_source]
            kind = "base_url"
            location = "https://x.ai/cli"
        }
        .into();
        assert!(matches!(
            resolve_runtime(Some(&document), env(&[])),
            Err(OverlayConfigError::InvalidUpdateSource(_))
        ));
    }

    #[test]
    fn update_source_rejects_malformed_github_repository() {
        let document: toml::Value = toml::toml! {
            [overlay]
            mode = "open"
            [overlay.update_source]
            kind = "github_release"
            location = "not-a-repository"
        }
        .into();
        assert!(matches!(
            resolve_runtime(Some(&document), env(&[])),
            Err(OverlayConfigError::InvalidUpdateSource(_))
        ));
    }

    #[test]
    fn without_overlay_keeps_upstream_document_unchanged() {
        let document: toml::Value = toml::toml! {
            [overlay]
            mode = "open"
            [models]
            default = "grok-3"
        }
        .into();

        let sanitized = without_overlay(&document);
        assert!(sanitized.get("overlay").is_none());
        assert_eq!(
            sanitized
                .get("models")
                .and_then(|models| models.get("default"))
                .and_then(toml::Value::as_str),
            Some("grok-3")
        );
    }
}
