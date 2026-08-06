//! Embedding provider abstraction for memory vector search.
//!
//! Defines the `EmbeddingProvider` trait and an API-based implementation
//! that calls an OpenAI-compatible embeddings API endpoint.
//!
//! Embeddings are cached in the sqlite-vec `chunks_vec` table — the vec0
//! virtual table IS the cache. No separate cache needed.

use async_trait::async_trait;
use indexmap::IndexMap;
use xai_grok_config_types::{
    CapabilityOperationConfig, CapabilityProviderConfig, ProviderAuthConfig, RequestMapping,
    ResponseMapping,
};

/// Maximum retry attempts for transient API errors (429, 5xx).
const MAX_RETRIES: usize = 3;
/// Initial backoff delay in milliseconds (doubles on each retry: 1s, 2s, 4s).
const INITIAL_BACKOFF_MS: u64 = 1000;

/// Trait for generating text embeddings.
///
/// Implementations must be `Send + Sync` so they can be used in `Send`
/// futures (e.g., inside `tokio::spawn`). The `embed_batch` method is
/// async to support API-based providers.
#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    /// Embed a batch of texts, returning one vector per input text.
    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>>;

    /// The model name used for embeddings.
    fn model_name(&self) -> &str;

    /// The dimensionality of the embedding vectors.
    fn dimensions(&self) -> usize;
}

/// Authentication scheme used by a static embedding API key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmbeddingAuthScheme {
    #[default]
    Bearer,
    XApiKey,
}

impl EmbeddingAuthScheme {
    /// Parse the user-facing config spelling. Unknown values fail closed so a
    /// typo never silently sends a key using the wrong header contract.
    pub fn parse(raw: Option<&str>) -> Option<Self> {
        match raw.map(str::trim).filter(|value| !value.is_empty()) {
            None => Some(Self::Bearer),
            Some(value) if value.eq_ignore_ascii_case("bearer") => Some(Self::Bearer),
            Some(value)
                if value.eq_ignore_ascii_case("x_api_key")
                    || value.eq_ignore_ascii_case("x-api-key") =>
            {
                Some(Self::XApiKey)
            }
            Some(_) => None,
        }
    }
}

/// Fully resolved, provider-neutral embedding request configuration.
///
/// The shell resolves inheritance and credential precedence before creating
/// this value. The memory crate then uses it for both search and reindex,
/// preventing those paths from drifting apart.
#[derive(Clone, Default)]
pub struct EmbeddingRuntimeConfig {
    pub base_url: String,
    pub model: String,
    pub dimensions: usize,
    pub protocol: String,
    pub path: String,
    pub api_key: Option<String>,
    pub auth_scheme: EmbeddingAuthScheme,
    pub auth: ProviderAuthConfig,
    pub extra_headers: IndexMap<String, String>,
    pub env_headers: IndexMap<String, String>,
    pub query_params: IndexMap<String, String>,
    pub request: RequestMapping,
    pub response: ResponseMapping,
}

impl std::fmt::Debug for EmbeddingRuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmbeddingRuntimeConfig")
            .field("base_url", &self.base_url)
            .field("model", &self.model)
            .field("dimensions", &self.dimensions)
            .field("protocol", &self.protocol)
            .field("path", &self.path)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("auth_scheme", &self.auth_scheme)
            .field("auth", &self.auth)
            .field("extra_headers", &self.extra_headers)
            .finish()
    }
}

/// API-based embedding provider using an OpenAI-compatible embeddings endpoint.
pub struct ApiEmbeddingProvider {
    api_base: String,
    model: String,
    dimensions: usize,
    protocol: String,
    path: String,
    /// Static per-model key used by custom OpenAI-compatible endpoints. The
    /// credential-middleware path supplies its own bearer and leaves this
    /// unset.
    api_key: Option<String>,
    #[allow(dead_code)]
    auth_scheme: EmbeddingAuthScheme,
    auth: ProviderAuthConfig,
    extra_headers: IndexMap<String, String>,
    env_headers: IndexMap<String, String>,
    query_params: IndexMap<String, String>,
    request_mapping: RequestMapping,
    response_mapping: ResponseMapping,
    client: reqwest_middleware::ClientWithMiddleware,
    max_batch_size: usize,
}

impl ApiEmbeddingProvider {
    pub fn new(
        api_base: String,
        model: String,
        dimensions: usize,
        client: reqwest_middleware::ClientWithMiddleware,
    ) -> Self {
        Self {
            api_base,
            model,
            dimensions,
            protocol: "openai_compatible".to_owned(),
            path: "/embeddings".to_owned(),
            api_key: None,
            auth_scheme: EmbeddingAuthScheme::Bearer,
            auth: ProviderAuthConfig::default(),
            extra_headers: IndexMap::new(),
            env_headers: IndexMap::new(),
            query_params: IndexMap::new(),
            request_mapping: RequestMapping::default(),
            response_mapping: ResponseMapping::default(),
            client,
            max_batch_size: 32,
        }
    }

    pub fn from_runtime(
        runtime: EmbeddingRuntimeConfig,
        client: reqwest_middleware::ClientWithMiddleware,
    ) -> Option<Self> {
        if runtime.model.trim().is_empty()
            || runtime.base_url.trim().is_empty()
            || runtime.dimensions == 0
            || reqwest::Url::parse(runtime.base_url.trim_end_matches('/')).is_err()
        {
            return None;
        }
        let protocol = if runtime.protocol.trim().is_empty() {
            "openai_compatible".to_owned()
        } else {
            runtime.protocol.clone()
        };
        let path = if runtime.path.trim().is_empty() {
            default_embedding_path(&protocol)
        } else {
            runtime.path.clone()
        };
        Some(Self {
            api_base: runtime.base_url.trim_end_matches('/').to_owned(),
            model: runtime.model,
            dimensions: runtime.dimensions,
            protocol,
            path,
            api_key: runtime.api_key,
            auth_scheme: runtime.auth_scheme,
            auth: runtime.auth,
            extra_headers: runtime.extra_headers,
            env_headers: runtime.env_headers,
            query_params: runtime.query_params,
            request_mapping: effective_request_mapping(
                if runtime.protocol.trim().is_empty() {
                    "openai_compatible"
                } else {
                    runtime.protocol.as_str()
                },
                runtime.request.clone(),
                runtime.dimensions,
            ),
            response_mapping: effective_response_mapping(
                if runtime.protocol.trim().is_empty() {
                    "openai_compatible"
                } else {
                    runtime.protocol.as_str()
                },
                runtime.response,
            ),
            client,
            max_batch_size: 32,
        })
    }

    pub fn from_config(
        config: &xai_grok_config_types::MemoryEmbeddingConfig,
        api_base: String,
        client: reqwest_middleware::ClientWithMiddleware,
    ) -> Option<Self> {
        if !config.provider.trim().eq_ignore_ascii_case("api") {
            tracing::warn!(
                provider = %config.provider,
                "memory embeddings: provider is not implemented; using FTS-only"
            );
            return None;
        }
        let model = config.model.clone().filter(|m| !m.is_empty())?;
        let auth_scheme = EmbeddingAuthScheme::parse(config.auth_scheme.as_deref())?;
        let mut request = config.request.clone();
        if let Some(input_type) = config.input_type.as_deref() {
            request.defaults.insert(
                "input_type".to_owned(),
                serde_json::Value::String(input_type.to_owned()),
            );
        }
        Self::from_runtime(
            EmbeddingRuntimeConfig {
                base_url: api_base,
                model,
                dimensions: config.dimensions,
                protocol: config.protocol.clone(),
                path: config
                    .path
                    .clone()
                    .unwrap_or_else(|| default_embedding_path(&config.protocol)),
                api_key: config
                    .api_key
                    .as_deref()
                    .filter(|key| !key.trim().is_empty())
                    .map(str::to_owned)
                    .or_else(|| {
                        config
                            .env_key
                            .as_ref()
                            .and_then(|keys| keys.resolve_value())
                    }),
                auth_scheme,
                auth: effective_embedding_auth(config),
                extra_headers: config.extra_headers.clone(),
                env_headers: config.env_headers.clone(),
                query_params: config.query_params.clone(),
                request,
                response: config.response.clone(),
            },
            client,
        )
    }

    pub fn from_session(
        config: &xai_grok_config_types::MemoryEmbeddingConfig,
        proxy_base_url: String,
        auth_key: String,
    ) -> Option<Self> {
        let client = build_static_middleware_client(Some(auth_key.clone()));
        let mut provider = Self::from_config(config, proxy_base_url, client)?;
        provider.api_key = Some(auth_key);
        Some(provider)
    }
}

pub(crate) fn effective_embedding_auth(
    config: &xai_grok_config_types::MemoryEmbeddingConfig,
) -> ProviderAuthConfig {
    let mut auth = config.auth.clone();
    // The legacy shorthand wins only when the nested auth block is still at
    // its default, preserving existing `auth_scheme = "x_api_key"` configs.
    if config.auth_scheme.is_some()
        && auth == ProviderAuthConfig::default()
        && matches!(
            config.auth_scheme.as_deref(),
            Some("x_api_key" | "x-api-key")
        )
    {
        auth.name = "x-api-key".to_owned();
        auth.prefix.clear();
    }
    auth
}

fn effective_request_mapping(
    protocol: &str,
    mut mapping: RequestMapping,
    dimensions: usize,
) -> RequestMapping {
    if mapping.fields.is_empty() {
        mapping
            .fields
            .insert("model".to_owned(), "model".to_owned());
        match protocol.to_ascii_lowercase().as_str() {
            "cohere" | "cohere_v1" | "cohere_v2" => {
                mapping
                    .fields
                    .insert("input".to_owned(), "texts".to_owned());
            }
            "voyage" => {
                mapping
                    .fields
                    .insert("input".to_owned(), "input".to_owned());
                mapping
                    .fields
                    .insert("dimensions".to_owned(), "output_dimension".to_owned());
            }
            _ => {
                mapping
                    .fields
                    .insert("input".to_owned(), "input".to_owned());
                mapping
                    .fields
                    .insert("dimensions".to_owned(), "dimensions".to_owned());
            }
        }
        if protocol.eq_ignore_ascii_case("cohere_v2") {
            mapping
                .fields
                .insert("dimensions".to_owned(), "output_dimension".to_owned());
        }
    }
    if matches!(
        protocol.to_ascii_lowercase().as_str(),
        "cohere" | "cohere_v1" | "cohere_v2"
    ) && !mapping.defaults.contains_key("input_type")
    {
        mapping.defaults.insert(
            "input_type".to_owned(),
            serde_json::Value::String("search_document".to_owned()),
        );
    }
    if protocol.eq_ignore_ascii_case("cohere_v2")
        && !mapping.defaults.contains_key("embedding_types")
    {
        mapping
            .defaults
            .insert("embedding_types".to_owned(), serde_json::json!(["float"]));
    }
    // Keep this value available to profiles that explicitly map dimensions,
    // while allowing providers that choose a fixed model dimension to omit it.
    if dimensions == 0 {
        mapping.fields.shift_remove("dimensions");
    }
    mapping
}

pub fn default_embedding_path(protocol: &str) -> String {
    match protocol.trim().to_ascii_lowercase().as_str() {
        "cohere_v2" => "/v2/embed".to_owned(),
        "cohere" | "cohere_v1" => "/embed".to_owned(),
        _ => "/embeddings".to_owned(),
    }
}

fn effective_response_mapping(protocol: &str, mut mapping: ResponseMapping) -> ResponseMapping {
    if mapping.value.is_none() && mapping.items.is_none() {
        match protocol.to_ascii_lowercase().as_str() {
            // Cohere Embed v2 groups vectors below an embedding type, while
            // the legacy `/embed` contract returns a plain array at
            // `embeddings`.
            "cohere_v2" => mapping.value = Some("/embeddings/float".to_owned()),
            "cohere" | "cohere_v1" => mapping.value = Some("/embeddings".to_owned()),
            _ => {
                mapping.items = Some("/data".to_owned());
                mapping.value = Some("/embedding".to_owned());
            }
        }
    }
    mapping
}

fn provider_profile(provider: &ApiEmbeddingProvider) -> CapabilityProviderConfig {
    let operation = CapabilityOperationConfig {
        method: "POST".to_owned(),
        path: provider.path.clone(),
        request: provider.request_mapping.clone(),
        response: provider.response_mapping.clone(),
        ..Default::default()
    };
    CapabilityProviderConfig {
        protocol: provider.protocol.clone(),
        base_url: Some(provider.api_base.clone()),
        model: Some(provider.model.clone()),
        api_key: provider.api_key.clone(),
        auth: provider.auth.clone(),
        extra_headers: provider.extra_headers.clone(),
        env_headers: provider.env_headers.clone(),
        query_params: provider.query_params.clone(),
        operations: [("default".to_owned(), operation)].into_iter().collect(),
        ..Default::default()
    }
}

fn parse_embedding_vectors(
    body: &serde_json::Value,
    mapping: &ResponseMapping,
    expected_dimensions: usize,
) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
    let vectors = if let Some(items) = mapping.items.as_deref() {
        let items = xai_grok_provider::json_pointer(body, items)
            .ok_or("embedding response missing configured items path")?
            .as_array()
            .ok_or("embedding items path is not an array")?;
        items
            .iter()
            .map(|item| {
                let vector = item
                    .pointer(mapping.value.as_deref().unwrap_or("/embedding"))
                    .and_then(serde_json::Value::as_array)
                    .ok_or("embedding item missing configured vector path")?;
                parse_vector(vector)
            })
            .collect::<Result<Vec<_>, _>>()?
    } else {
        let value = mapping
            .value
            .as_deref()
            .and_then(|pointer| xai_grok_provider::json_pointer(body, pointer))
            .ok_or("embedding response missing configured vector path")?;
        if let Some(array) = value.as_array() {
            if array.first().is_some_and(serde_json::Value::is_array) {
                array
                    .iter()
                    .map(|vector| {
                        parse_vector(
                            vector
                                .as_array()
                                .ok_or("embedding vector is not an array")?,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?
            } else {
                vec![parse_vector(array)?]
            }
        } else {
            return Err("configured embedding value is not an array".into());
        }
    };
    for vector in &vectors {
        if vector.len() != expected_dimensions {
            return Err(format!(
                "embedding vector dimension mismatch: expected {}, got {}",
                expected_dimensions,
                vector.len()
            )
            .into());
        }
    }
    Ok(vectors)
}

fn parse_vector(values: &[serde_json::Value]) -> Result<Vec<f32>, Box<dyn std::error::Error>> {
    values
        .iter()
        .map(|value| {
            value
                .as_f64()
                .map(|number| number as f32)
                .ok_or_else(|| "embedding vector contains a non-number".into())
        })
        .collect()
}

pub(super) fn build_middleware_client(
    credentials: std::sync::Arc<dyn xai_grok_auth::AuthCredentialProvider>,
) -> reqwest_middleware::ClientWithMiddleware {
    xai_grok_http::with_auth_retry(xai_grok_http::shared_client(), credentials)
}

pub(super) fn build_static_middleware_client(
    api_key: Option<String>,
) -> reqwest_middleware::ClientWithMiddleware {
    let provider: std::sync::Arc<dyn xai_grok_auth::AuthCredentialProvider> = std::sync::Arc::new(
        xai_grok_auth::StaticAuthCredentialProvider::new(Box::new(NoopHttpAuth), api_key),
    );
    build_middleware_client(provider)
}

struct NoopHttpAuth;

impl xai_grok_auth::HttpAuth for NoopHttpAuth {
    fn apply(&self, builder: reqwest::RequestBuilder, _base_url: &str) -> reqwest::RequestBuilder {
        builder
    }
}

#[async_trait]
impl EmbeddingProvider for ApiEmbeddingProvider {
    #[tracing::instrument(name = "memory.embed_batch", skip_all, fields(batch_size = texts.len()))]
    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        if texts.is_empty() {
            return Ok(vec![]);
        }

        let mut all_embeddings = Vec::with_capacity(texts.len());
        let profile = provider_profile(self);
        let runtime = xai_grok_provider::ProviderHttpRuntime::shared();

        // Process in batches to respect API payload limits
        for batch in texts.chunks(self.max_batch_size) {
            let input = xai_grok_provider::ProviderRequestInput::new()
                .value("model", self.model.clone())
                .value("input", serde_json::json!(batch));
            let input = if self.request_mapping.fields.contains_key("dimensions") {
                input.value("dimensions", self.dimensions)
            } else {
                input
            };

            // Retry with exponential backoff on transient errors (429, 5xx)
            let mut last_err = String::new();
            let mut success = false;
            for attempt in 0..MAX_RETRIES {
                if attempt > 0 {
                    let delay = INITIAL_BACKOFF_MS * 2u64.pow(attempt as u32 - 1);
                    tracing::warn!(
                        attempt,
                        delay_ms = delay,
                        "retrying embedding API call after transient error"
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }

                let mut req = match runtime
                    .build(&profile, "default", &input, |name| std::env::var(name).ok())
                {
                    Ok(request) => request.request,
                    Err(error) => return Err(error.to_string().into()),
                };
                if is_xai_owned_url(&self.api_base) {
                    let headers = req.headers_mut();
                    headers.insert(
                        "X-XAI-Token-Auth",
                        reqwest::header::HeaderValue::from_static("xai-grok-cli"),
                    );
                    headers.insert(
                        "x-grok-client-version",
                        reqwest::header::HeaderValue::from_static(xai_grok_version::VERSION),
                    );
                }
                let response = match self.client.execute(req).await {
                    Ok(r) => r,
                    Err(e) => {
                        last_err = format!("request failed: {e}");
                        continue;
                    }
                };

                let status = response.status();
                if status.is_success() {
                    let body: serde_json::Value = response.json().await?;
                    all_embeddings.extend(parse_embedding_vectors(
                        &body,
                        &self.response_mapping,
                        self.dimensions,
                    )?);
                    success = true;
                    break;
                }

                // Retry on 429 (rate limit) or 5xx (server error)
                if status == reqwest::StatusCode::TOO_MANY_REQUESTS || status.is_server_error() {
                    last_err = format!(
                        "HTTP {status}: {}",
                        response.text().await.unwrap_or_default()
                    );
                    continue;
                }

                // Non-retryable error (4xx other than 429)
                let body = response.text().await.unwrap_or_default();
                return Err(format!("embedding API error {status}: {body}").into());
            }

            if !success {
                return Err(format!(
                    "embedding API failed after {MAX_RETRIES} attempts: {last_err}"
                )
                .into());
            }
        }

        Ok(all_embeddings)
    }

    fn model_name(&self) -> &str {
        &self.model
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

fn is_xai_owned_url(raw: &str) -> bool {
    let Ok(url) = reqwest::Url::parse(raw) else {
        return false;
    };
    let Some(host) = url.host_str() else {
        return false;
    };
    host == "x.ai" || host.ends_with(".x.ai") || host == "grok.com" || host.ends_with(".grok.com")
}

/// A mock embedding provider for testing that returns deterministic vectors.
/// Uses blake3 hash of text → float values for reproducible results.
#[cfg(any(test, feature = "test-support"))]
pub struct MockEmbeddingProvider {
    pub dimensions: usize,
}

#[cfg(any(test, feature = "test-support"))]
#[async_trait]
impl EmbeddingProvider for MockEmbeddingProvider {
    async fn embed_batch(
        &self,
        texts: &[&str],
    ) -> Result<Vec<Vec<f32>>, Box<dyn std::error::Error>> {
        Ok(texts
            .iter()
            .map(|text| {
                let hash = blake3::hash(text.as_bytes());
                let bytes = hash.as_bytes();
                (0..self.dimensions)
                    .map(|i| bytes[i % 32] as f32 / 255.0)
                    .collect()
            })
            .collect())
    }

    fn model_name(&self) -> &str {
        "mock-embedding"
    }

    fn dimensions(&self) -> usize {
        self.dimensions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn api_embedding_uses_runtime_endpoint_auth_and_headers() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(header("x-api-key", "embedding-secret"))
            .and(header("x-custom-header", "test-value"))
            .and(body_json(serde_json::json!({
                "model": "embed-model",
                "input": ["hello"],
                "dimensions": 2,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": [0.25, 0.75]}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let mut extra_headers = IndexMap::new();
        extra_headers.insert("X-Custom-Header".to_owned(), "test-value".to_owned());
        let provider = ApiEmbeddingProvider::from_runtime(
            EmbeddingRuntimeConfig {
                base_url: format!("{}/v1", server.uri()),
                model: "embed-model".to_owned(),
                dimensions: 2,
                api_key: Some("embedding-secret".to_owned()),
                auth_scheme: EmbeddingAuthScheme::XApiKey,
                extra_headers,
                auth: ProviderAuthConfig {
                    name: "x-api-key".to_owned(),
                    prefix: String::new(),
                    ..Default::default()
                },
                ..Default::default()
            },
            build_static_middleware_client(Some("embedding-secret".to_owned())),
        )
        .expect("runtime config should build");

        let embeddings = provider.embed_batch(&["hello"]).await.unwrap();
        assert_eq!(embeddings, vec![vec![0.25, 0.75]]);
    }

    #[tokio::test]
    async fn cohere_profile_maps_texts_and_nested_float_vectors() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/embed"))
            .and(header("authorization", "Bearer cohere-key"))
            .and(body_json(serde_json::json!({
                "model": "embed-v4.0",
                "texts": ["hello"],
                "input_type": "search_document",
                "embedding_types": ["float"],
                "output_dimension": 2,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "embeddings": {"float": [[0.1, 0.2]]}
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = ApiEmbeddingProvider::from_runtime(
            EmbeddingRuntimeConfig {
                base_url: server.uri(),
                model: "embed-v4.0".into(),
                dimensions: 2,
                protocol: "cohere_v2".into(),
                path: "/v2/embed".into(),
                api_key: Some("cohere-key".into()),
                ..Default::default()
            },
            build_static_middleware_client(Some("cohere-key".into())),
        )
        .unwrap();

        assert_eq!(
            provider.embed_batch(&["hello"]).await.unwrap(),
            vec![vec![0.1, 0.2]]
        );
    }

    #[tokio::test]
    async fn cohere_v1_profile_reads_plain_embedding_arrays() {
        use wiremock::matchers::{body_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/embed"))
            .and(body_json(serde_json::json!({
                "model": "embed-english-v3.0",
                "texts": ["hello"],
                "input_type": "search_document",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "embeddings": [[0.5, 0.6]]
            })))
            .mount(&server)
            .await;

        let provider = ApiEmbeddingProvider::from_runtime(
            EmbeddingRuntimeConfig {
                base_url: server.uri(),
                model: "embed-english-v3.0".into(),
                dimensions: 2,
                protocol: "cohere_v1".into(),
                path: "/embed".into(),
                api_key: Some("cohere-key".into()),
                ..Default::default()
            },
            build_static_middleware_client(Some("cohere-key".into())),
        )
        .unwrap();

        assert_eq!(
            provider.embed_batch(&["hello"]).await.unwrap(),
            vec![vec![0.5, 0.6]]
        );
    }

    #[tokio::test]
    async fn voyage_profile_maps_output_dimension() {
        use wiremock::matchers::{body_json, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v1/embeddings"))
            .and(body_json(serde_json::json!({
                "model": "voyage-3-large",
                "input": ["hello"],
                "output_dimension": 2,
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": [0.3, 0.4]}]
            })))
            .expect(1)
            .mount(&server)
            .await;

        let provider = ApiEmbeddingProvider::from_runtime(
            EmbeddingRuntimeConfig {
                base_url: format!("{}/v1", server.uri()),
                model: "voyage-3-large".into(),
                dimensions: 2,
                protocol: "voyage".into(),
                api_key: Some("voyage-key".into()),
                ..Default::default()
            },
            build_static_middleware_client(Some("voyage-key".into())),
        )
        .unwrap();

        assert_eq!(
            provider.embed_batch(&["hello"]).await.unwrap(),
            vec![vec![0.3, 0.4]]
        );
    }

    #[tokio::test]
    async fn embedding_dimension_mismatch_fails_closed() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/embeddings"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"embedding": [0.3]}]
            })))
            .mount(&server)
            .await;

        let provider = ApiEmbeddingProvider::from_runtime(
            EmbeddingRuntimeConfig {
                base_url: server.uri(),
                model: "embed".into(),
                dimensions: 2,
                api_key: Some("key".into()),
                ..Default::default()
            },
            build_static_middleware_client(Some("key".into())),
        )
        .unwrap();

        let error = provider.embed_batch(&["hello"]).await.unwrap_err();
        assert!(error.to_string().contains("dimension mismatch"));
    }

    #[tokio::test]
    async fn test_mock_embedding_deterministic() {
        let provider = MockEmbeddingProvider { dimensions: 4 };
        let r1 = provider.embed_batch(&["hello"]).await.unwrap();
        let r2 = provider.embed_batch(&["hello"]).await.unwrap();
        assert_eq!(r1, r2);
    }

    #[tokio::test]
    async fn test_mock_embedding_different_texts() {
        let provider = MockEmbeddingProvider { dimensions: 4 };
        let results = provider.embed_batch(&["hello", "world"]).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_ne!(results[0], results[1]);
    }

    #[tokio::test]
    async fn test_mock_embedding_empty_input() {
        let provider = MockEmbeddingProvider { dimensions: 4 };
        let results = provider.embed_batch(&[]).await.unwrap();
        assert!(results.is_empty());
    }

    #[tokio::test]
    async fn test_mock_embedding_correct_dimensions() {
        let provider = MockEmbeddingProvider { dimensions: 128 };
        let results = provider.embed_batch(&["test"]).await.unwrap();
        assert_eq!(results[0].len(), 128);
    }
}
