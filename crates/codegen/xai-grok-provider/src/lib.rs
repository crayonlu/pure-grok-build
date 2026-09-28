//! Provider-neutral capability configuration.
//!
//! Auxiliary services do not all behave like chat endpoints: search may use
//! query parameters, image APIs may use multipart bodies, and video APIs may
//! expose several operations. These types describe those differences without
//! baking a vendor name into the runtime.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

mod http;
pub use http::{
    BinaryPart, ProviderHttpRequest, ProviderHttpRuntime, ProviderRequestInput,
    ProviderRuntimeError, json_pointer,
};

/// Environment variable names in priority order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ProviderEnvKeys {
    One(String),
    Many(Vec<String>),
}

impl ProviderEnvKeys {
    pub fn names(&self) -> Box<dyn Iterator<Item = &str> + '_> {
        match self {
            Self::One(name) => Box::new(std::iter::once(name.as_str())),
            Self::Many(names) => Box::new(names.iter().map(String::as_str)),
        }
    }

    pub fn resolve_with(&self, mut getenv: impl FnMut(&str) -> Option<String>) -> Option<String> {
        self.names()
            .find_map(|name| getenv(name).filter(|value| !value.trim().is_empty()))
    }
}

/// A profile name remains open-ended so adding a data-only profile does not
/// require changing this crate.
pub type ProtocolProfile = String;

/// Where and how a credential is sent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderAuthConfig {
    /// `header` (default) or `query`.
    pub location: String,
    /// Header or query parameter name.
    pub name: String,
    /// Optional prefix, e.g. `Bearer `.
    pub prefix: String,
}

impl Default for ProviderAuthConfig {
    fn default() -> Self {
        Self {
            location: "header".to_owned(),
            name: "Authorization".to_owned(),
            prefix: "Bearer ".to_owned(),
        }
    }
}

impl ProviderAuthConfig {
    pub fn validate(&self) -> Result<(), String> {
        if !matches!(self.location.as_str(), "header" | "query") {
            return Err(format!(
                "auth.location must be `header` or `query`, got `{}`",
                self.location
            ));
        }
        if self.name.trim().is_empty() {
            return Err("auth.name must not be empty".to_owned());
        }
        Ok(())
    }

    pub fn rendered_value(&self, key: &str) -> String {
        format!("{}{}", self.prefix, key)
    }
}

/// Wire-level request body codec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BodyCodec {
    #[default]
    Json,
    Query,
    Multipart,
    Binary,
}

/// A multipart file part sourced from a normalized request field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MultipartPartConfig {
    pub field: String,
    pub filename: Option<String>,
    pub content_type: Option<String>,
}

impl Default for MultipartPartConfig {
    fn default() -> Self {
        Self {
            field: String::new(),
            filename: None,
            content_type: None,
        }
    }
}

/// Mapping from normalized request names to provider wire names.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RequestMapping {
    pub body: BodyCodec,
    /// JSON/form/multipart scalar fields: normalized name -> wire name/path.
    pub fields: IndexMap<String, String>,
    /// Query fields: normalized name -> query parameter.
    pub query: IndexMap<String, String>,
    /// Static provider defaults merged into JSON/form bodies.
    pub defaults: IndexMap<String, Value>,
    /// Binary fields: normalized name -> multipart part metadata.
    pub files: IndexMap<String, MultipartPartConfig>,
}

/// JSON/content extraction paths. Paths use RFC 6901 JSON Pointer syntax.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResponseMapping {
    pub items: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub value: Option<String>,
    pub url: Option<String>,
    pub bytes: Option<String>,
    pub job_id: Option<String>,
    pub poll_url: Option<String>,
    pub status: Option<String>,
    pub error: Option<String>,
}

/// Async operation behavior for video, deployment and similar APIs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AsyncOperationConfig {
    pub poll_interval_ms: u64,
    pub timeout_secs: u64,
    pub success_statuses: Vec<String>,
    pub failure_statuses: Vec<String>,
}

impl Default for AsyncOperationConfig {
    fn default() -> Self {
        Self {
            poll_interval_ms: 2_000,
            timeout_secs: 300,
            success_statuses: vec![
                "completed".to_owned(),
                "succeeded".to_owned(),
                "done".to_owned(),
            ],
            failure_statuses: vec![
                "failed".to_owned(),
                "canceled".to_owned(),
                "expired".to_owned(),
            ],
        }
    }
}

/// One HTTP operation. Multi-step APIs use names such as `create`, `poll`,
/// `download`, and `cancel` in the enclosing `operations` map.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapabilityOperationConfig {
    pub method: String,
    pub path: String,
    pub request: RequestMapping,
    pub response: ResponseMapping,
    pub async_config: Option<AsyncOperationConfig>,
}

impl CapabilityOperationConfig {
    pub fn effective_method(&self) -> &str {
        if self.method.trim().is_empty() {
            "POST"
        } else {
            self.method.as_str()
        }
    }
}

/// Streaming transport hints. Event decoding is intentionally kept in a
/// protocol-family driver; these fields only describe framing.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamConfig {
    pub transport: String,
    pub framing: String,
    pub audio_message: String,
    pub final_event: Option<String>,
    pub error_event: Option<String>,
}

/// Artifact transfer hints for sandbox/deploy providers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ArtifactConfig {
    pub upload_operation: Option<String>,
    pub download_operation: Option<String>,
    pub content_type: Option<String>,
    pub max_bytes: Option<u64>,
}

/// Complete capability provider definition.
#[derive(Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapabilityProviderConfig {
    /// `openai_compatible`, `cohere_v2`, `generic_http`, etc.
    pub protocol: ProtocolProfile,
    pub base_url: Option<String>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub env_key: Option<ProviderEnvKeys>,
    pub auth: ProviderAuthConfig,
    pub extra_headers: IndexMap<String, String>,
    pub env_headers: IndexMap<String, String>,
    pub query_params: IndexMap<String, String>,
    pub operations: IndexMap<String, CapabilityOperationConfig>,
    pub stream: Option<StreamConfig>,
    pub artifact: Option<ArtifactConfig>,
}

impl std::fmt::Debug for CapabilityProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CapabilityProviderConfig")
            .field("protocol", &self.protocol)
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("env_key", &self.env_key)
            .field("auth", &self.auth)
            .field("extra_headers", &self.extra_headers)
            .field("env_headers", &self.env_headers)
            .field("query_params", &self.query_params)
            .field("operations", &self.operations)
            .field("stream", &self.stream)
            .field("artifact", &self.artifact)
            .finish()
    }
}

impl CapabilityProviderConfig {
    /// Resolve a key using explicit static key, then ordered environment keys.
    pub fn resolve_api_key(
        &self,
        mut getenv: impl FnMut(&str) -> Option<String>,
    ) -> Option<String> {
        self.api_key
            .as_deref()
            .map(str::trim)
            .filter(|key| !key.is_empty())
            .map(str::to_owned)
            .or_else(|| {
                self.env_key
                    .as_ref()
                    .and_then(|keys| keys.resolve_with(&mut getenv))
            })
    }

    /// Validate endpoint and protocol-level security invariants before a
    /// request is built.
    pub fn validate(&self) -> Result<(), String> {
        self.auth.validate()?;
        if let Some(raw) = self.base_url.as_deref() {
            let url = url::Url::parse(raw.trim_end_matches('/'))
                .map_err(|error| format!("invalid base_url: {error}"))?;
            if !matches!(url.scheme(), "http" | "https") {
                return Err("base_url must use http or https".to_owned());
            }
            if !url.username().is_empty() || url.password().is_some() {
                return Err("base_url must not contain embedded credentials".to_owned());
            }
        }
        for (name, operation) in &self.operations {
            if operation.path.contains("..") {
                return Err(format!("operation `{name}` path must not contain `..`"));
            }
            if !operation.path.is_empty() && !operation.path.starts_with('/') {
                return Err(format!("operation `{name}` path must start with `/`"));
            }
            if !matches!(
                operation.effective_method().to_ascii_uppercase().as_str(),
                "GET" | "POST" | "PUT" | "PATCH" | "DELETE"
            ) {
                return Err(format!(
                    "operation `{name}` has unsupported HTTP method `{}`",
                    operation.effective_method()
                ));
            }
        }
        Ok(())
    }

    pub fn operation(&self, name: &str) -> Option<&CapabilityOperationConfig> {
        self.operations
            .get(name)
            .or_else(|| self.operations.get("default"))
    }
}

/// User-facing capability sections. Each capability is independent so an
/// embedding endpoint never inherits search or chat request parameters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CapabilityProvidersConfig {
    pub search: Option<CapabilityProviderConfig>,
    pub embedding: Option<CapabilityProviderConfig>,
    pub image: Option<CapabilityProviderConfig>,
    pub video: Option<CapabilityProviderConfig>,
    pub voice: Option<CapabilityProviderConfig>,
    pub sandbox: Option<CapabilityProviderConfig>,
    pub deploy: Option<CapabilityProviderConfig>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_layered_cohere_profile() {
        let value: toml::Value = toml::from_str(
            r#"
            protocol = "cohere_v2"
            base_url = "https://api.cohere.com"
            model = "embed-v4.0"
            env_key = ["COHERE_API_KEY", "BACKUP_KEY"]
            [auth]
            location = "header"
            name = "Authorization"
            prefix = "Bearer "
            [operations.default]
            method = "POST"
            path = "/v2/embed"
            [operations.default.request]
            body = "json"
            [operations.default.request.fields]
            model = "model"
            inputs = "texts"
            input_type = "input_type"
            [operations.default.response]
            value = "/embeddings/float"
            "#,
        )
        .unwrap();
        let profile: CapabilityProviderConfig = value.try_into().unwrap();
        assert_eq!(profile.protocol, "cohere_v2");
        assert_eq!(profile.operations["default"].path, "/v2/embed");
        assert_eq!(profile.operations["default"].request.body, BodyCodec::Json);
        assert!(profile.validate().is_ok());
    }

    #[test]
    fn static_key_wins_over_ordered_environment_keys() {
        let profile = CapabilityProviderConfig {
            api_key: Some("literal".into()),
            env_key: Some(ProviderEnvKeys::Many(vec!["A".into(), "B".into()])),
            ..Default::default()
        };
        assert_eq!(
            profile.resolve_api_key(|name| Some(format!("{name}-value"))),
            Some("literal".into())
        );
    }

    #[test]
    fn debug_redacts_static_key() {
        let profile = CapabilityProviderConfig {
            api_key: Some("do-not-log".into()),
            ..Default::default()
        };
        let rendered = format!("{profile:?}");
        assert!(!rendered.contains("do-not-log"));
        assert!(rendered.contains("[redacted]"));
    }

    #[test]
    fn rejects_credentials_in_base_url_and_unsafe_paths() {
        let profile = CapabilityProviderConfig {
            base_url: Some("https://user:pass@example.test".into()),
            operations: [(
                "default".into(),
                CapabilityOperationConfig {
                    path: "/../secret".into(),
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        assert!(profile.validate().is_err());
    }

    #[test]
    fn provider_env_keys_resolve_in_order() {
        let keys = ProviderEnvKeys::Many(vec!["FIRST".into(), "SECOND".into()]);
        assert_eq!(
            keys.resolve_with(|name| (name == "SECOND").then(|| "key".into())),
            Some("key".into())
        );
    }
}
