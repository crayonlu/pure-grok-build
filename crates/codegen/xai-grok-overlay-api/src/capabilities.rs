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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityAvailability {
    Disabled,
    Provider(CapabilityProviderRef),
}

/// A serializable reference to a provider profile.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityProviderRef {
    pub name: String,
    pub protocol: String,
    pub base_url: Option<String>,
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
        }
    }

    pub fn parsed_base_url(&self) -> Result<Option<url::Url>, url::ParseError> {
        self.base_url.as_deref().map(url::Url::parse).transpose()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapabilitySet {
    pub selected: BTreeMap<Capability, CapabilityAvailability>,
}

impl CapabilitySet {
    pub fn disabled() -> Self {
        Self::default()
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
            .unwrap_or(CapabilityAvailability::Disabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capability_set_defaults_to_disabled() {
        assert_eq!(
            CapabilitySet::default().get(Capability::ImageGeneration),
            CapabilityAvailability::Disabled
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
