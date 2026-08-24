use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Chat,
    Embeddings,
    WebSearch,
    WebFetch,
    ImageGeneration,
    ImageEditing,
    VideoGeneration,
    Voice,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAvailability {
    /// Let the host's native capability resolver decide.
    Inherited,
    Disabled,
    Provider(CapabilityProviderRef),
}

impl CapabilityAvailability {
    pub const fn is_inherited(&self) -> bool {
        matches!(self, Self::Inherited)
    }

    pub const fn is_disabled(&self) -> bool {
        matches!(self, Self::Disabled)
    }

    pub fn provider(&self) -> Option<&CapabilityProviderRef> {
        match self {
            Self::Provider(provider) => Some(provider),
            Self::Inherited | Self::Disabled => None,
        }
    }
}

/// A serializable reference to a provider profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapabilityProviderRef {
    pub name: String,
    pub protocol: String,
    pub base_url: Option<String>,
    /// Full provider-neutral request/auth profile. The legacy scalar fields
    /// remain populated for callers that only need discovery metadata.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<xai_grok_provider::CapabilityProviderConfig>,
}

impl CapabilityProviderRef {
    pub fn new(
        name: impl Into<String>,
        protocol: impl Into<String>,
        base_url: Option<impl Into<String>>,
    ) -> Self {
        Self {
            name: name.into(),
            protocol: protocol.into(),
            base_url: base_url.map(Into::into),
            profile: None,
        }
    }

    pub fn from_profile(
        name: impl Into<String>,
        profile: xai_grok_provider::CapabilityProviderConfig,
    ) -> Self {
        Self {
            name: name.into(),
            protocol: profile.protocol.clone(),
            base_url: profile.base_url.clone(),
            profile: Some(profile),
        }
    }

    pub fn parsed_base_url(&self) -> Result<Option<url::Url>, url::ParseError> {
        self.base_url.as_deref().map(url::Url::parse).transpose()
    }

    pub fn provider_profile(&self) -> Option<&xai_grok_provider::CapabilityProviderConfig> {
        self.profile.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapabilitySet {
    pub selected: BTreeMap<Capability, CapabilityAvailability>,
    default: CapabilityAvailability,
}

impl Default for CapabilitySet {
    fn default() -> Self {
        Self::inherited()
    }
}

impl CapabilitySet {
    pub fn disabled() -> Self {
        Self {
            selected: BTreeMap::new(),
            default: CapabilityAvailability::Disabled,
        }
    }

    pub fn inherited() -> Self {
        Self {
            selected: BTreeMap::new(),
            default: CapabilityAvailability::Inherited,
        }
    }

    pub fn with_provider(
        mut self,
        capability: Capability,
        provider: CapabilityProviderRef,
    ) -> Self {
        self.selected
            .insert(capability, CapabilityAvailability::Provider(provider));
        self
    }

    pub fn disable(mut self, capability: Capability) -> Self {
        self.selected
            .insert(capability, CapabilityAvailability::Disabled);
        self
    }

    pub fn get(&self, capability: Capability) -> CapabilityAvailability {
        self.selected
            .get(&capability)
            .cloned()
            .unwrap_or_else(|| self.default.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_set_defaults_to_inherited() {
        assert_eq!(
            CapabilitySet::default().get(Capability::ImageGeneration),
            CapabilityAvailability::Inherited
        );
    }

    #[test]
    fn provider_reference_validates_base_url_without_owning_http() {
        let provider = CapabilityProviderRef::new(
            "local-images",
            "generic_http",
            Some("https://images.example.test/v1"),
        );

        assert_eq!(
            provider
                .parsed_base_url()
                .expect("valid URL")
                .expect("base URL")
                .host_str(),
            Some("images.example.test")
        );
    }
}
