//! Memory system shim.
//!
//! The memory "core engine" now lives in the standalone `xai-grok-memory` crate.
//! This module re-exports that crate's public API under the historical `crate::session::memory::*` paths.
//!
//! Only `hooks` stays here: it is session glue (depends on `crate::sampling` and `crate::session::helpers::session_compact`).

pub mod hooks;

pub use xai_grok_memory::{
    EmbeddingAuthScheme, EmbeddingRuntimeConfig, EndpointScopedCredentials, MemoryBackendImpl,
    MemoryBackendParams, MemoryIndex, MemoryScope, MemorySearchSource, MemoryStorage, archive,
    backend, chunker, dream, dream_lock, embed_missing_chunks, embedding, index, init_sqlite_vec,
    mmr, noop_memory_observation_sink, query_expansion, schema, search, storage, text_utils,
    watcher,
};

/// Resolve the effective embedding request configuration once at session
/// startup. The resolved value is copied into `MemoryBackendParams`, so tool
/// search, context injection, compaction recovery, and background reindex all
/// use the same endpoint/model/auth contract.
pub(crate) fn resolve_embedding_runtime(
    config: Option<&crate::config::MemoryEmbeddingConfig>,
    sampling_config: &crate::sampling::SamplerConfig,
    credentials: &xai_chat_state::Credentials,
) -> Option<EmbeddingRuntimeConfig> {
    let config = config?;
    if !config.provider.trim().eq_ignore_ascii_case("api") {
        tracing::warn!(
            provider = %config.provider,
            "memory embeddings: provider is not implemented; using FTS-only"
        );
        return None;
    }

    let model = config
        .model
        .clone()
        .filter(|model| !model.trim().is_empty())?;
    if config.dimensions == 0 {
        tracing::warn!("memory embeddings: dimensions must be greater than zero; using FTS-only");
        return None;
    }
    let base_url = config
        .base_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
        .unwrap_or(sampling_config.base_url.as_str())
        .trim_end_matches('/')
        .to_owned();
    if reqwest::Url::parse(&base_url).is_err() {
        tracing::warn!(
            base_url = %base_url,
            "memory embeddings: invalid base_url; using FTS-only"
        );
        return None;
    }

    let explicit_key = config
        .api_key
        .as_deref()
        .filter(|key| !key.trim().is_empty())
        .map(str::to_owned)
        .or_else(|| {
            config
                .env_key
                .as_ref()
                .and_then(|keys| keys.resolve_value())
        });
    // A primary session token is never inherited by a different embedding
    // endpoint. Static BYOK credentials are safe to inherit because they are
    // already user-owned and are the established model-level fallback.
    let inherited_key = (credentials.auth_type == xai_chat_state::AuthType::ApiKey)
        .then(|| sampling_config.api_key.clone())
        .flatten()
        .filter(|key| !key.trim().is_empty());
    if credentials.auth_type == xai_chat_state::AuthType::SessionToken
        && base_url != sampling_config.base_url.trim_end_matches('/')
        && explicit_key.is_none()
    {
        tracing::warn!(
            base_url,
            "memory embeddings: refusing to forward a session/OAuth credential to a custom endpoint; configure api_key or env_key for embeddings (using FTS-only)"
        );
        return None;
    }
    let api_key = explicit_key.or(inherited_key);

    let auth_scheme = match config.auth_scheme.as_deref() {
        Some(raw) => EmbeddingAuthScheme::parse(Some(raw)),
        None if base_url == sampling_config.base_url.trim_end_matches('/') => {
            match sampling_config.auth_scheme {
                xai_grok_sampler::AuthScheme::XApiKey => Some(EmbeddingAuthScheme::XApiKey),
                xai_grok_sampler::AuthScheme::Bearer => Some(EmbeddingAuthScheme::Bearer),
            }
        }
        None => Some(EmbeddingAuthScheme::Bearer),
    };
    let Some(auth_scheme) = auth_scheme else {
        tracing::warn!(
            auth_scheme = ?config.auth_scheme,
            "memory embeddings: unsupported auth_scheme; using FTS-only"
        );
        return None;
    };

    let mut auth = config.auth.clone();
    if config.auth_scheme.is_some()
        && auth == xai_grok_config_types::ProviderAuthConfig::default()
        && matches!(
            config.auth_scheme.as_deref(),
            Some("x_api_key" | "x-api-key")
        )
    {
        auth.name = "x-api-key".to_owned();
        auth.prefix.clear();
    }
    let mut request = config.request.clone();
    if let Some(input_type) = config.input_type.as_deref() {
        request.defaults.insert(
            "input_type".to_owned(),
            serde_json::Value::String(input_type.to_owned()),
        );
    }

    Some(EmbeddingRuntimeConfig {
        base_url,
        model,
        dimensions: config.dimensions,
        protocol: config.protocol.clone(),
        path: config.path.clone().unwrap_or_else(|| {
            xai_grok_memory::embedding::default_embedding_path(&config.protocol)
        }),
        api_key,
        auth_scheme,
        auth,
        extra_headers: config.extra_headers.clone(),
        env_headers: config.env_headers.clone(),
        query_params: config.query_params.clone(),
        request,
        response: config.response.clone(),
    })
}

/// Open mode blocks first-party xAI endpoints but permits an explicitly
/// configured local embedding server (Ollama, llama.cpp, or a test mock).
/// The shared URL helpers intentionally treat loopback as trusted for auth
/// tests, so this policy needs to exclude loopback before applying them.
pub(crate) fn embedding_endpoint_allowed_in_open(base_url: &str) -> bool {
    let is_loopback = reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
        .is_some_and(|host| {
            host.eq_ignore_ascii_case("localhost")
                || host
                    .parse::<std::net::IpAddr>()
                    .is_ok_and(|address| address.is_loopback())
        });
    is_loopback || !crate::util::is_first_party_remote_url(base_url)
}

/// Open mode requires the embedding endpoint to be explicitly configured.
/// This prevents an inherited chat endpoint from becoming an implicit
/// xAI-compatible auxiliary service.
pub(crate) fn embedding_config_allowed_in_open(
    config: &crate::config::MemoryEmbeddingConfig,
    base_url: &str,
) -> bool {
    config
        .base_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty())
        && embedding_endpoint_allowed_in_open(base_url)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sampling(base_url: &str, key: Option<&str>) -> crate::sampling::SamplerConfig {
        crate::sampling::SamplerConfig {
            base_url: base_url.to_owned(),
            model: "chat-model".to_owned(),
            api_key: key.map(str::to_owned),
            ..Default::default()
        }
    }

    fn credentials(
        auth_type: xai_chat_state::AuthType,
        key: Option<&str>,
    ) -> xai_chat_state::Credentials {
        xai_chat_state::Credentials {
            api_key: key.map(str::to_owned),
            auth_type,
            ..Default::default()
        }
    }

    #[test]
    fn resolver_prefers_embedding_endpoint_and_static_key() {
        let mut config = crate::config::MemoryEmbeddingConfig {
            model: Some("embed-model".to_owned()),
            base_url: Some("https://embed.example/v1/".to_owned()),
            api_key: Some("embed-key".to_owned()),
            auth_scheme: Some("x_api_key".to_owned()),
            ..Default::default()
        };
        config.extra_headers.insert("X-Test".into(), "yes".into());
        let runtime = resolve_embedding_runtime(
            Some(&config),
            &sampling("https://chat.example/v1", Some("chat-key")),
            &credentials(xai_chat_state::AuthType::ApiKey, Some("chat-key")),
        )
        .expect("explicit config should resolve");
        assert_eq!(runtime.base_url, "https://embed.example/v1");
        assert_eq!(runtime.model, "embed-model");
        assert_eq!(runtime.api_key.as_deref(), Some("embed-key"));
        assert_eq!(runtime.auth_scheme, EmbeddingAuthScheme::XApiKey);
        assert_eq!(
            runtime.extra_headers.get("X-Test").map(String::as_str),
            Some("yes")
        );
    }

    #[test]
    fn resolver_does_not_forward_session_token_to_custom_endpoint() {
        let config = crate::config::MemoryEmbeddingConfig {
            model: Some("embed-model".to_owned()),
            base_url: Some("https://embed.example/v1".to_owned()),
            ..Default::default()
        };
        let runtime = resolve_embedding_runtime(
            Some(&config),
            &sampling("https://chat.example/v1", Some("session-token")),
            &credentials(
                xai_chat_state::AuthType::SessionToken,
                Some("session-token"),
            ),
        );
        assert!(runtime.is_none(), "custom endpoint must remain FTS-only");
    }

    #[test]
    fn resolver_inherits_static_primary_key_and_endpoint() {
        let config = crate::config::MemoryEmbeddingConfig {
            model: Some("embed-model".to_owned()),
            ..Default::default()
        };
        let runtime = resolve_embedding_runtime(
            Some(&config),
            &sampling("https://chat.example/v1/", Some("primary-key")),
            &credentials(xai_chat_state::AuthType::ApiKey, Some("primary-key")),
        )
        .expect("model should enable embedding");
        assert_eq!(runtime.base_url, "https://chat.example/v1");
        assert_eq!(runtime.api_key.as_deref(), Some("primary-key"));
    }

    #[test]
    fn resolver_uses_embedding_env_key_before_primary_fallback() {
        let _guard = crate::env::EnvVarGuard::set(
            "GROK_TEST_MEMORY_EMBEDDING_KEY_UNIQUE",
            "env-embedding-key",
        );
        let config = crate::config::MemoryEmbeddingConfig {
            model: Some("embed-model".to_owned()),
            env_key: Some(crate::config::MemoryEnvKeys::One(
                "GROK_TEST_MEMORY_EMBEDDING_KEY_UNIQUE".to_owned(),
            )),
            ..Default::default()
        };
        let runtime = resolve_embedding_runtime(
            Some(&config),
            &sampling("https://chat.example/v1", Some("primary-key")),
            &credentials(xai_chat_state::AuthType::ApiKey, Some("primary-key")),
        )
        .expect("model should enable embedding");
        assert_eq!(runtime.api_key.as_deref(), Some("env-embedding-key"));
    }

    #[test]
    fn resolver_keeps_missing_model_fts_only() {
        let config = crate::config::MemoryEmbeddingConfig::default();
        assert!(
            resolve_embedding_runtime(
                Some(&config),
                &sampling("https://chat.example/v1", Some("primary-key")),
                &credentials(xai_chat_state::AuthType::ApiKey, Some("primary-key")),
            )
            .is_none()
        );
    }

    #[test]
    fn resolver_warns_and_disables_unimplemented_provider() {
        let config = crate::config::MemoryEmbeddingConfig {
            provider: "local".to_owned(),
            model: Some("local-model".to_owned()),
            ..Default::default()
        };
        assert!(
            resolve_embedding_runtime(
                Some(&config),
                &sampling("https://chat.example/v1", Some("primary-key")),
                &credentials(xai_chat_state::AuthType::ApiKey, Some("primary-key")),
            )
            .is_none()
        );
    }

    #[test]
    fn open_embedding_policy_allows_local_and_custom_but_not_xai() {
        assert!(embedding_endpoint_allowed_in_open(
            "http://localhost:11434/v1"
        ));
        assert!(embedding_endpoint_allowed_in_open(
            "https://embed.example/v1"
        ));
        assert!(!embedding_endpoint_allowed_in_open("https://api.x.ai/v1"));
        assert!(!embedding_endpoint_allowed_in_open(
            "https://cli-chat-proxy.grok.com/v1"
        ));
        let inherited = crate::config::MemoryEmbeddingConfig::default();
        assert!(!embedding_config_allowed_in_open(
            &inherited,
            "https://embed.example/v1"
        ));
        let explicit = crate::config::MemoryEmbeddingConfig {
            base_url: Some("https://embed.example/v1".to_owned()),
            ..Default::default()
        };
        assert!(embedding_config_allowed_in_open(
            &explicit,
            "https://embed.example/v1"
        ));
    }
}
