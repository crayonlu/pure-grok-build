//! `image_gen` tool — generates images via the xAI Imagine API and saves
//! them to the local filesystem so the model can reference them in code
//! (e.g. `<img src="images/hero.jpg">`).
//!
//! Architecture follows the same pattern as `web_search`:
//!
//! - [`ImageGenConfig`] is built from session credentials by the host and
//!   injected into the tool registry.
//! - When `Enabled`, an [`ImageGenClient`] is constructed once and injected
//!   into `Resources`. The tool reads it at runtime via `resources.require()`.
//! - When `Disabled`, the tool is not registered so the model never sees it.
//!
//! The generated image is written to `<session_folder>/images/<n>.jpg`
//! where `<n>` is a session-scoped counter (1, 2, 3, ... — 1 token each).
//! The tool returns the absolute path so the model can copy or move the
//! image into the project working directory when it needs a persistent asset.

use base64::Engine as _;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderValue};

use crate::attribution::{SharedAttributionCallback, ToolConsumer};
use crate::types::SharedApiKeyProvider;

use crate::types::output::{MediaGenOutput, ToolOutput};
use crate::types::requirements::{Expr, ToolRequirement};
use crate::types::resources::SessionFolder;
use crate::types::tool::{ToolKind, ToolNamespace};

// ---------------------------------------------------------------------------
// Provider-agnostic image generation configuration
// ---------------------------------------------------------------------------

/// How the size/aspect-ratio is represented in the request body.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SizeFormat {
    /// Pixel dimensions, e.g. `"1024x1024"`. Requires `size_map` to
    /// translate Grok aspect ratios into provider-specific pixel strings.
    #[default]
    Dimensions,
    /// Aspect ratio string, e.g. `"1:1"`. Passed through as-is.
    Ratio,
}

/// How the HTTP response carries the generated image.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResponseMode {
    /// Image bytes are base64-encoded inline in the JSON response.
    #[default]
    Base64,
    /// The response contains a URL that must be downloaded to get the bytes.
    Url,
}

/// How the `image` parameter is formatted in edit requests.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditImageFormat {
    /// Plain data-URL string: `"data:image/png;base64,..."`.
    #[default]
    String,
    /// Wrapped in an object: `{"url": "data:image/png;base64,..."}`.
    ObjectUrl,
}

/// Provider-agnostic image generation API configuration. A partial
/// `[image_gen]` section keeps the native xAI-compatible wire behavior;
/// populate the format fields when adapting a different image API.
///
/// Credentials (`env_key` / `api_key`) are resolved by the shell; the
/// tools crate only uses the format fields.
#[derive(Clone, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct ImageGenProviderConfig {
    /// Environment variable name(s) holding the API key (shell resolves).
    pub env_key: Option<String>,
    /// Literal API key (prefer `env_key` for secrets).
    pub api_key: Option<String>,
    /// Base URL for an explicitly configured image-generation endpoint.
    pub base_url: String,
    /// Path appended to `base_url` for text-to-image.
    pub gen_path: String,
    /// Path appended to `base_url` for image edit.
    pub edit_path: String,
    /// Request-body field name for the size/aspect-ratio value.
    pub size_field: String,
    /// How to represent the size.
    pub size_format: SizeFormat,
    /// Extra static key-value pairs merged into every request body.
    pub extra_fields: indexmap::IndexMap<String, serde_json::Value>,
    /// How the response carries image data.
    pub response_mode: ResponseMode,
    /// Top-level JSON field containing the image data array.
    pub response_field: String,
    /// Subfield within each array element. Empty string means the array
    /// elements are plain strings (URL or base64).
    pub response_subfield: String,
    /// How the `image` parameter is formatted in edit requests.
    pub edit_image_format: EditImageFormat,
    /// Mapping from Grok aspect-ratio strings to provider size strings.
    /// Only consulted when `size_format` is `Dimensions`.
    pub size_map: indexmap::IndexMap<String, String>,
}

impl std::fmt::Debug for ImageGenProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImageGenProviderConfig")
            .field("env_key", &self.env_key)
            .field("api_key", &self.api_key.as_ref().map(|_| "[redacted]"))
            .field("base_url", &self.base_url)
            .field("gen_path", &self.gen_path)
            .field("edit_path", &self.edit_path)
            .field("size_field", &self.size_field)
            .field("size_format", &self.size_format)
            .field("extra_fields", &self.extra_fields)
            .field("response_mode", &self.response_mode)
            .field("response_field", &self.response_field)
            .field("response_subfield", &self.response_subfield)
            .field("edit_image_format", &self.edit_image_format)
            .field("size_map", &self.size_map)
            .finish()
    }
}

impl ImageGenProviderConfig {
    /// Whether the caller supplied a custom wire shape. A base URL and key
    /// alone are intentionally treated as the native Imagine-compatible
    /// contract so older local configurations keep working.
    pub fn has_custom_wire_format(&self) -> bool {
        !self.gen_path.is_empty()
            || !self.edit_path.is_empty()
            || !self.size_field.is_empty()
            || !self.extra_fields.is_empty()
            || !matches!(self.response_mode, ResponseMode::Base64)
            || !self.response_field.is_empty()
            || !self.response_subfield.is_empty()
            || !matches!(self.edit_image_format, EditImageFormat::String)
            || !self.size_map.is_empty()
    }

    /// Build the full URL for a text-to-image request.
    pub fn gen_url(&self) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), self.gen_path)
    }

    /// Build the full URL for an image-edit request.
    pub fn edit_url(&self) -> String {
        format!("{}{}", self.base_url.trim_end_matches('/'), self.edit_path)
    }

    /// Map a Grok aspect-ratio string to the provider's size value.
    pub fn resolve_size<'a>(&'a self, aspect_ratio: &'a str) -> &'a str {
        match self.size_format {
            SizeFormat::Ratio => aspect_ratio,
            SizeFormat::Dimensions => self
                .size_map
                .get(aspect_ratio)
                .map(|s| s.as_str())
                .unwrap_or("auto"),
        }
    }
}

/// Default Imagine model for `image_gen`. Used unless an explicit
/// `model_override` is supplied via `ImageGenConfig::Enabled`.
const XAI_IMAGINE_MODEL: &str = "grok-imagine-image-quality";
// Some Imagine models (e.g. `grok-imagine-image`, selectable via `model_override`)
// expand the prompt then generate, and the proxy buffers
// the whole image before sending any bytes — so the client may receive nothing
// for well over a minute. Keep these generous so a slow-but-progressing
// generation isn't cut off.
const IMAGE_GEN_TIMEOUT_SECS: u64 = 300;
const IMAGE_GEN_READ_TIMEOUT_SECS: u64 = 240;
const DEFAULT_IMAGE_DIR: &str = "images";

pub use xai_grok_tools_api::slash_commands::{
    IMAGE_GEN_TOOL_NAME, IMAGINE_COMMAND_NAME, imagine_instruction, imagine_usage_message,
};

/// Prose returned to the model (as a normal, successful tool result) when a
/// free / X Basic user calls `image_gen` or `image_edit`. The model relays it
/// to the user. The deliberate `/imagine` slash command shows the richer
/// SuperGrok upsell modal instead; this covers the natural-language path.
pub(crate) const TIER_RESTRICTED_UPSELL: &str = "Image generation is a SuperGrok feature and isn't available on the free or X Basic tier. Let the user know they can unlock image and video generation by upgrading to SuperGrok: https://grok.com/supergrok?referrer=grok-build. Do not retry this tool.";

/// HTTP client for xAI Imagine API. Cloned per-request; shares `Arc` state.
#[derive(Clone)]
pub struct ImageGenClient {
    http: reqwest::Client,
    base_url: String,
    /// Imagine model slug used by `generate()`. Selected at construction
    /// from `ImageGenConfig::model_override` (falling back to
    /// [`XAI_IMAGINE_MODEL`]). `image_edit` uses its own model and is
    /// unaffected.
    model: String,
    edit_model: String,
    writer: super::storage::SessionFileWriter,
    api_key_provider: Option<SharedApiKeyProvider>,
    /// Optional 401-attribution hook. Hosts wire this so a 401 from the
    /// Imagine API emits an `auth_401_attribution` event with
    /// `consumer == "ImageGen"` for unified auth-failure telemetry.
    attribution_callback: Option<SharedAttributionCallback>,
    /// When `true`, the user is on a tier the Imagine server zero-limits
    /// (free / X Basic). `image_gen` / `image_edit` short-circuit before any
    /// HTTP call and return the SuperGrok upsell prose instead. See
    /// [`ImageGenClient::is_tier_restricted`].
    tier_restricted: bool,
    /// Per-request [`SESSION_ID_HEADER`]; kept off `default_headers` so the
    /// transport stays session-independent and cacheable.
    session_header: Option<HeaderValue>,
    defaults_have_session_header: bool,
    /// Provider-agnostic API format config. When `Some`, the client uses
    /// the provider's endpoint paths, request body format, and response
    /// parsing instead of the default x.ai Imagine behavior.
    provider: Option<ImageGenProviderConfig>,
    /// Fully generic capability profile. This owns the endpoint, transport,
    /// request mapping, authentication, and response extraction.
    capability_profile: Option<xai_grok_provider::CapabilityProviderConfig>,
}

impl ImageGenClient {
    pub fn new(
        config: &ImageGenConfig,
        api_key_provider: Option<SharedApiKeyProvider>,
    ) -> Result<Self, xai_tool_runtime::ToolError> {
        let ImageGenConfig::Enabled {
            api_key,
            base_url,
            extra_headers,
            model_override,
            edit_model_override,
            tier_restricted,
            provider,
            capability_profile,
            ..
        } = config
        else {
            return Err(xai_tool_runtime::ToolError::invalid_arguments(
                "Cannot create ImageGenClient from disabled config",
            ));
        };
        let provider = provider
            .as_ref()
            .filter(|provider| provider.has_custom_wire_format())
            .cloned();
        let model = model_override
            .clone()
            .filter(|m| !m.trim().is_empty())
            .or_else(|| {
                capability_profile
                    .as_ref()
                    .and_then(|profile| profile.model.clone())
                    .filter(|m| !m.trim().is_empty())
            })
            .unwrap_or_else(|| XAI_IMAGINE_MODEL.to_owned());
        let edit_model = edit_model_override
            .clone()
            .filter(|m| !m.trim().is_empty())
            .unwrap_or_else(|| super::image_edit::XAI_IMAGINE_EDIT_MODEL.to_owned());

        let mut headers = reqwest::header::HeaderMap::new();
        headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
        // Always bake the static api_key as the default Authorization header.
        // The dynamic provider overrides per-request; this is the fallback.
        if capability_profile.is_none() {
            headers.insert(
                AUTHORIZATION,
                HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|e| {
                    xai_tool_runtime::ToolError::invalid_arguments(format!(
                        "Invalid API key for header: {e}"
                    ))
                })?,
            );
        }

        extra_headers.into_iter().try_for_each(|(key, value)| {
            let header_name =
                reqwest::header::HeaderName::from_bytes(key.as_bytes()).map_err(|e| {
                    xai_tool_runtime::ToolError::invalid_arguments(format!(
                        "Invalid header name '{key}': {e}"
                    ))
                })?;
            let header_value = HeaderValue::from_str(value).map_err(|e| {
                xai_tool_runtime::ToolError::invalid_arguments(format!(
                    "Invalid header value for '{key}': {e}"
                ))
            })?;
            headers.insert(header_name, header_value);
            Ok::<(), xai_tool_runtime::ToolError>(())
        })?;

        // Process-cached: timeouts are constants, so the headers key
        // suffices; the session id is attached per request, not here.
        let defaults_have_session_header = headers.contains_key(SESSION_ID_HEADER);
        let key = crate::util::shared_http::cache_key("image_gen", &headers);
        let http = crate::util::shared_http::cached_client(key, || {
            xai_grok_extra_ca::build_reqwest_client(|builder| {
                builder
                    .timeout(std::time::Duration::from_secs(IMAGE_GEN_TIMEOUT_SECS))
                    .read_timeout(std::time::Duration::from_secs(IMAGE_GEN_READ_TIMEOUT_SECS))
                    .default_headers(headers.clone())
            })
        })
        .map_err(|e| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Failed to build HTTP client: {e}"
            ))
        })?;

        Ok(Self {
            http,
            base_url: base_url.clone(),
            model,
            edit_model,
            writer: super::storage::SessionFileWriter::new(DEFAULT_IMAGE_DIR, "jpg"),
            api_key_provider,
            attribution_callback: None,
            tier_restricted: *tier_restricted,
            session_header: None,
            defaults_have_session_header,
            provider,
            capability_profile: capability_profile.clone(),
        })
    }

    /// Attach [`SESSION_ID_HEADER`] per request; a caller-provided
    /// `extra_headers` value is never overridden.
    pub fn with_session_id(mut self, session_id: &str) -> Self {
        if !self.defaults_have_session_header
            && let Ok(value) = HeaderValue::from_str(session_id)
        {
            self.session_header = Some(value);
        }
        self
    }

    /// Whether the current user's tier (free / X Basic) is zero-limited on
    /// Imagine server-side. `image_gen` / `image_edit` use this to short-circuit
    /// with the SuperGrok upsell instead of issuing a doomed request.
    pub(crate) fn is_tier_restricted(&self) -> bool {
        self.tier_restricted
    }

    /// Wire a 401-attribution callback into this client. Idempotent;
    /// safe to call before or after the first request. Builder-style
    /// so `new()` callers that don't care can ignore it.
    pub fn with_attribution_callback(
        mut self,
        callback: Option<SharedAttributionCallback>,
    ) -> Self {
        self.attribution_callback = callback;
        self
    }

    pub(crate) async fn current_bearer(&self) -> Option<String> {
        // A legacy `[image_gen]` provider owns its own resolved key. Do not
        // let the session's primary-model key provider overwrite it; that
        // would break independent BYOK and could send the wrong credential
        // to the image endpoint. Capability profiles already carry auth in
        // their provider mapping and never call this helper.
        if self.provider.is_some() {
            return None;
        }
        crate::types::api_key_provider::resolve_bearer(self.api_key_provider.as_ref()).await
    }

    pub(crate) fn record_401_attribution(&self, consumer: ToolConsumer, sent_bearer: Option<&str>) {
        crate::attribution::emit_401(self.attribution_callback.as_ref(), consumer, sent_bearer);
    }

    #[allow(dead_code)]
    pub(crate) fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Every Imagine-API POST goes through here so no call site can miss
    /// the bearer or per-request session header (image_edit once did).
    pub(crate) fn post_json(
        &self,
        url: &str,
        payload: &serde_json::Value,
        sent_bearer: Option<&str>,
    ) -> reqwest::RequestBuilder {
        let mut req = self.http.post(url).json(payload);
        if let Some(key) = sent_bearer {
            req = req.header(AUTHORIZATION, format!("Bearer {key}"));
        }
        if let Some(ref session) = self.session_header {
            req = req.header(SESSION_ID_HEADER, session.clone());
        }
        req
    }

    pub(crate) fn writer(&self) -> &super::storage::SessionFileWriter {
        &self.writer
    }

    pub(crate) fn edit_model(&self) -> &str {
        &self.edit_model
    }

    /// Provider-agnostic API format config, if any.
    pub(crate) fn provider(&self) -> Option<&ImageGenProviderConfig> {
        self.provider.as_ref()
    }

    pub(crate) fn capability_profile(
        &self,
    ) -> Option<&xai_grok_provider::CapabilityProviderConfig> {
        self.capability_profile.as_ref()
    }

    /// Build the full URL for a text-to-image request.
    pub(crate) fn gen_url(&self) -> String {
        match &self.provider {
            Some(p) => p.gen_url(),
            None => format!("{}/images/generations", self.base_url.trim_end_matches('/')),
        }
    }

    /// Build the full URL for an image-edit request.
    pub(crate) fn edit_url(&self) -> String {
        match &self.provider {
            Some(p) => p.edit_url(),
            None => format!("{}/images/edits", self.base_url.trim_end_matches('/')),
        }
    }

    /// Build the request body for a text-to-image call.
    pub(crate) fn build_gen_payload(&self, prompt: &str, aspect_ratio: &str) -> serde_json::Value {
        match &self.provider {
            Some(p) => {
                let mut payload = serde_json::json!({ "prompt": prompt });
                let size_value = p.resolve_size(aspect_ratio);
                payload[p.size_field.clone()] = serde_json::Value::String(size_value.to_string());
                for (k, v) in &p.extra_fields {
                    payload[k.clone()] = v.clone();
                }
                payload
            }
            None => serde_json::json!({
                "model": self.model,
                "prompt": prompt,
                "n": 1,
                "aspect_ratio": aspect_ratio,
                "resolution": "1k",
                "response_format": "b64_json",
            }),
        }
    }

    /// Extract image bytes from an HTTP response body. Handles both
    /// base64-inline and URL-download modes depending on the provider config.
    pub(crate) async fn extract_image_bytes(
        &self,
        body: &str,
    ) -> Result<Vec<u8>, xai_tool_runtime::ToolError> {
        match &self.provider {
            Some(p) => {
                let json: serde_json::Value = serde_json::from_str(body).map_err(|e| {
                    let preview: String = body.chars().take(500).collect();
                    xai_tool_runtime::ToolError::invalid_arguments(format!(
                        "Failed to parse image generation response: {e} — body preview: {preview}"
                    ))
                })?;
                let arr = json
                    .get(&p.response_field)
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        xai_tool_runtime::ToolError::invalid_arguments(format!(
                            "Image response missing '{}' array field",
                            p.response_field
                        ))
                    })?;
                let first = arr.first().ok_or_else(|| {
                    xai_tool_runtime::ToolError::invalid_arguments(
                        "Image generation returned no image data.".to_string(),
                    )
                })?;
                let data_str = if p.response_subfield.is_empty() {
                    first.as_str().ok_or_else(|| {
                        xai_tool_runtime::ToolError::invalid_arguments(
                            "Image response array element is not a string".to_string(),
                        )
                    })?
                } else {
                    first
                        .get(&p.response_subfield)
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| {
                            xai_tool_runtime::ToolError::invalid_arguments(format!(
                                "Image response missing '{}' subfield",
                                p.response_subfield
                            ))
                        })?
                };
                match p.response_mode {
                    ResponseMode::Base64 => base64::engine::general_purpose::STANDARD
                        .decode(data_str)
                        .map_err(|e| {
                            xai_tool_runtime::ToolError::invalid_arguments(format!(
                                "Failed to decode base64 image data: {e}"
                            ))
                        }),
                    ResponseMode::Url => {
                        let download_client = reqwest::Client::new();
                        let resp = download_client.get(data_str).send().await.map_err(|e| {
                            xai_tool_runtime::ToolError::invalid_arguments(format!(
                                "Failed to download image from URL: {e}"
                            ))
                        })?;
                        if !resp.status().is_success() {
                            return Err(xai_tool_runtime::ToolError::invalid_arguments(format!(
                                "Image download failed with HTTP {}",
                                resp.status()
                            )));
                        }
                        let bytes = resp.bytes().await.map_err(|e| {
                            xai_tool_runtime::ToolError::invalid_arguments(format!(
                                "Failed to read image download body: {e}"
                            ))
                        })?;
                        Ok(bytes.to_vec())
                    }
                }
            }
            None => {
                let resp_json: ImageGenResponse = serde_json::from_str(body).map_err(|e| {
                    let preview: String = body.chars().take(500).collect();
                    tracing::warn!("Imagine API returned unparseable body: {preview}");
                    xai_tool_runtime::ToolError::invalid_arguments(format!(
                        "Failed to parse image generation response: {e} — body preview: {preview}"
                    ))
                })?;
                let b64_data = resp_json.b64_data().unwrap_or("");
                if b64_data.is_empty() {
                    return Err(xai_tool_runtime::ToolError::invalid_arguments(
                        "Image generation returned no image data.".to_string(),
                    ));
                }
                base64::engine::general_purpose::STANDARD
                    .decode(b64_data)
                    .map_err(|e| {
                        xai_tool_runtime::ToolError::invalid_arguments(format!(
                            "Failed to decode base64 image data: {e}"
                        ))
                    })
            }
        }
    }

    pub async fn generate(
        &self,
        prompt: &str,
        aspect_ratio: &str,
    ) -> Result<Vec<u8>, xai_tool_runtime::ToolError> {
        if self.capability_profile.is_some() {
            return self.generate_profiled(prompt, aspect_ratio).await;
        }
        let url = self.gen_url();
        let payload = self.build_gen_payload(prompt, aspect_ratio);

        // Capture the bearer once so the request and the 401-attribution
        // emit see the same value (even if the provider rotates between
        // the send and the response handling).
        let sent_bearer = self.current_bearer().await;
        let req = self.post_json(&url, &payload, sent_bearer.as_deref());

        let response = req.send().await.map_err(|e| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Image generation API request failed: {e}"
            ))
        })?;

        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            self.record_401_attribution(ToolConsumer::ImageGen, sent_bearer.as_deref());
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            let truncated: String = body.chars().take(200).collect();
            tracing::warn!(http_status = %status, "Imagine API error: {truncated}");
            return Err(xai_tool_runtime::ToolError::new(
                xai_tool_runtime::ToolErrorKind::Custom,
                format!("Image generation failed with HTTP {status}: {truncated}"),
            )
            .with_details(serde_json::json!({"code": "http_failure", "status": status.as_u16()})));
        }

        let body = response.text().await.map_err(|e| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Failed to read image generation response body: {e}"
            ))
        })?;

        self.extract_image_bytes(&body).await
    }

    async fn generate_profiled(
        &self,
        prompt: &str,
        aspect_ratio: &str,
    ) -> Result<Vec<u8>, xai_tool_runtime::ToolError> {
        let profile = self.capability_profile.as_ref().expect("checked above");
        let operation = profile
            .operation("generate")
            .or_else(|| profile.operation("default"))
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments(
                    "image capability profile has no generate/default operation",
                )
            })?;
        let input = xai_grok_provider::ProviderRequestInput::new()
            .value("model", self.model.clone())
            .value("prompt", prompt.to_owned())
            .value("aspect_ratio", aspect_ratio.to_owned())
            .value("size", aspect_ratio.to_owned())
            .value("n", 1_i64)
            .value("response_format", "b64_json");
        let built = xai_grok_provider::ProviderHttpRuntime::new(self.http.clone())
            .build(profile, "generate", &input, |name| std::env::var(name).ok())
            .or_else(|_| {
                xai_grok_provider::ProviderHttpRuntime::new(self.http.clone()).build(
                    profile,
                    "default",
                    &input,
                    |name| std::env::var(name).ok(),
                )
            })
            .map_err(|error| xai_tool_runtime::ToolError::invalid_arguments(error.to_string()))?;
        let response = self.http.execute(built.request).await.map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Image provider request failed: {error}"
            ))
        })?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            self.record_401_attribution(ToolConsumer::ImageGen, profile.api_key.as_deref());
        }
        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(xai_tool_runtime::ToolError::new(
                xai_tool_runtime::ToolErrorKind::Custom,
                format!(
                    "Image provider failed with HTTP {}: {}",
                    status,
                    body.chars().take(200).collect::<String>()
                ),
            ));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let bytes = response.bytes().await.map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Failed to read image provider response: {error}"
            ))
        })?;
        if !content_type.contains("json") {
            return Ok(bytes.to_vec());
        }
        let body: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Invalid image provider JSON response: {error}"
            ))
        })?;
        let mapping = &operation.response;
        if let Some(pointer) = mapping.bytes.as_deref()
            && let Some(value) =
                xai_grok_provider::json_pointer(&body, pointer).and_then(|value| value.as_str())
        {
            return base64::engine::general_purpose::STANDARD
                .decode(value)
                .map_err(|error| {
                    xai_tool_runtime::ToolError::invalid_arguments(format!(
                        "Invalid base64 image response: {error}"
                    ))
                });
        }
        if let Some(pointer) = mapping.url.as_deref()
            && let Some(url) =
                xai_grok_provider::json_pointer(&body, pointer).and_then(|value| value.as_str())
        {
            return download_profiled_image(url).await;
        }
        let value = mapping
            .value
            .as_deref()
            .and_then(|pointer| xai_grok_provider::json_pointer(&body, pointer))
            .or_else(|| {
                mapping
                    .items
                    .as_deref()
                    .and_then(|pointer| xai_grok_provider::json_pointer(&body, pointer))
            })
            .or_else(|| body.get("data"));
        let value = value
            .and_then(|value| {
                value
                    .as_array()
                    .and_then(|items| items.first())
                    .or(Some(value))
            })
            .and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("b64_json").and_then(|v| v.as_str()))
            });
        let encoded = value.ok_or_else(|| {
            xai_tool_runtime::ToolError::invalid_arguments(
                "Image provider response did not contain base64 data",
            )
        })?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                xai_tool_runtime::ToolError::invalid_arguments(format!(
                    "Invalid base64 image response: {error}"
                ))
            })
    }

    pub(crate) async fn edit_profiled(
        &self,
        prompt: &str,
        images: Vec<String>,
        aspect_ratio: &str,
    ) -> Result<Vec<u8>, xai_tool_runtime::ToolError> {
        let profile = self.capability_profile.as_ref().ok_or_else(|| {
            xai_tool_runtime::ToolError::invalid_arguments(
                "image capability profile is not configured",
            )
        })?;
        let operation_name = if profile.operation("edit").is_some() {
            "edit"
        } else {
            "default"
        };
        let first_image = images.first().cloned().unwrap_or_default();
        let mut input = xai_grok_provider::ProviderRequestInput::new()
            .value("model", self.edit_model.clone())
            .value("prompt", prompt.to_owned())
            .value("image", first_image)
            .value(
                "images",
                serde_json::Value::Array(images.into_iter().map(serde_json::Value::from).collect()),
            )
            .value("aspect_ratio", aspect_ratio.to_owned())
            .value("size", aspect_ratio.to_owned());
        let operation = profile.operation(operation_name).ok_or_else(|| {
            xai_tool_runtime::ToolError::invalid_arguments(
                "image capability profile has no edit/default operation",
            )
        })?;
        // Providers such as OpenAI expose `/images/edits` as multipart and
        // require an actual file part rather than a data URL string. The tool
        // has already normalized attachments to base64 data URLs, so decode
        // the first image when the profile declares a binary file mapping.
        if operation.request.files.contains_key("image") {
            if let Some(part) = decode_image_data_url(
                input
                    .values
                    .get("image")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or_default(),
            )? {
                input = input.binary("image", part);
            }
        }
        let built = xai_grok_provider::ProviderHttpRuntime::new(self.http.clone())
            .build(profile, operation_name, &input, |name| {
                std::env::var(name).ok()
            })
            .map_err(|error| xai_tool_runtime::ToolError::invalid_arguments(error.to_string()))?;
        let response = self.http.execute(built.request).await.map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Image edit provider request failed: {error}"
            ))
        })?;
        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(xai_tool_runtime::ToolError::new(
                xai_tool_runtime::ToolErrorKind::Custom,
                format!(
                    "Image edit provider failed with HTTP {status}: {}",
                    body.chars().take(200).collect::<String>()
                ),
            ));
        }
        let content_type = response
            .headers()
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();
        let bytes = response.bytes().await.map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Failed to read image edit response: {error}"
            ))
        })?;
        if !content_type.contains("json") {
            return Ok(bytes.to_vec());
        }
        let body: serde_json::Value = serde_json::from_slice(&bytes).map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Invalid image edit JSON response: {error}"
            ))
        })?;
        if let Some(pointer) = operation.response.url.as_deref()
            && let Some(url) =
                xai_grok_provider::json_pointer(&body, pointer).and_then(|value| value.as_str())
        {
            return download_profiled_image(url).await;
        }
        let value = operation
            .response
            .bytes
            .as_deref()
            .and_then(|pointer| xai_grok_provider::json_pointer(&body, pointer))
            .or_else(|| {
                operation
                    .response
                    .value
                    .as_deref()
                    .and_then(|pointer| xai_grok_provider::json_pointer(&body, pointer))
            })
            .or_else(|| {
                operation
                    .response
                    .items
                    .as_deref()
                    .and_then(|pointer| xai_grok_provider::json_pointer(&body, pointer))
            })
            .or_else(|| body.get("data"));
        let value = value
            .and_then(|value| {
                value
                    .as_array()
                    .and_then(|items| items.first())
                    .or(Some(value))
            })
            .and_then(|value| {
                value
                    .as_str()
                    .or_else(|| value.get("b64_json").and_then(|v| v.as_str()))
            });
        let encoded = value.ok_or_else(|| {
            xai_tool_runtime::ToolError::invalid_arguments(
                "Image edit response did not contain image data",
            )
        })?;
        base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .map_err(|error| {
                xai_tool_runtime::ToolError::invalid_arguments(format!(
                    "Invalid base64 image edit response: {error}"
                ))
            })
    }
}

/// Download a provider-returned image URL without forwarding the capability
/// provider's credential. Image URLs are commonly presigned object URLs and
/// must be treated as a separate origin from the API endpoint.
async fn download_profiled_image(raw_url: &str) -> Result<Vec<u8>, xai_tool_runtime::ToolError> {
    let url = reqwest::Url::parse(raw_url).map_err(|error| {
        xai_tool_runtime::ToolError::invalid_arguments(format!(
            "Invalid image provider URL: {error}"
        ))
    })?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return Err(xai_tool_runtime::ToolError::invalid_arguments(
            "Image provider URL must use http(s) without embedded credentials",
        ));
    }
    let response = reqwest::Client::new()
        .get(url)
        .send()
        .await
        .map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Failed to download image provider URL: {error}"
            ))
        })?;
    if !response.status().is_success() {
        return Err(xai_tool_runtime::ToolError::invalid_arguments(format!(
            "Image provider download failed with HTTP {}",
            response.status()
        )));
    }
    response
        .bytes()
        .await
        .map(|bytes| bytes.to_vec())
        .map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "Failed to read image provider download: {error}"
            ))
        })
}

fn decode_image_data_url(
    value: &str,
) -> Result<Option<xai_grok_provider::BinaryPart>, xai_tool_runtime::ToolError> {
    let Some(rest) = value.strip_prefix("data:") else {
        return Ok(None);
    };
    let Some((meta, encoded)) = rest.split_once(',') else {
        return Err(xai_tool_runtime::ToolError::invalid_arguments(
            "malformed image data URL",
        ));
    };
    if !meta.ends_with(";base64") {
        return Err(xai_tool_runtime::ToolError::invalid_arguments(
            "image data URL must use base64 encoding",
        ));
    }
    let mime = meta.trim_end_matches(";base64");
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| {
            xai_tool_runtime::ToolError::invalid_arguments(format!(
                "invalid base64 image data URL: {error}"
            ))
        })?;
    let extension = mime.rsplit('/').next().unwrap_or("bin");
    Ok(Some(xai_grok_provider::BinaryPart {
        bytes: bytes::Bytes::from(bytes),
        filename: Some(format!("image.{extension}")),
        content_type: Some(mime.to_owned()),
    }))
}

/// `Enabled` means credentials are present; each tool has its own gate.
#[derive(Debug, Clone, Default)]
pub enum ImageGenConfig {
    #[default]
    Disabled,
    Enabled {
        api_key: String,
        base_url: String,
        extra_headers: indexmap::IndexMap<String, String>,
        image_gen_enabled: bool,
        image_edit_enabled: bool,
        /// Optional Imagine model override for `image_gen`. When `Some(non-empty)`,
        /// `image_gen` calls that model instead of the default quality model
        /// ([`XAI_IMAGINE_MODEL`]). Driven by the remote
        /// `image_gen_model_override` config flag. `image_edit` is unaffected.
        model_override: Option<String>,
        edit_model_override: Option<String>,
        /// `true` when the user is on a tier the Imagine server zero-limits
        /// (free / X Basic). The tools stay advertised to the model, but
        /// `image_gen` / `image_edit` short-circuit at call time with the
        /// SuperGrok upsell prose instead of a doomed request. Set by the
        /// host from the subscription tier; always `false` for team /
        /// API-key / workspace callers.
        tier_restricted: bool,
        /// Provider-agnostic API format config. When `Some`, overrides the
        /// default x.ai Imagine behavior with a configurable endpoint,
        /// request body, and response format.
        provider: Option<ImageGenProviderConfig>,
        capability_profile: Option<xai_grok_provider::CapabilityProviderConfig>,
    },
}

/// Session-id header attached to imagine API requests; matches the header
/// chat requests already carry.
pub const SESSION_ID_HEADER: &str = "x-grok-session-id";

impl ImageGenConfig {
    /// Credentials present — required to construct any of the clients.
    pub fn has_credentials(&self) -> bool {
        matches!(self, Self::Enabled { .. })
    }

    pub fn image_gen_enabled(&self) -> bool {
        matches!(
            self,
            Self::Enabled {
                image_gen_enabled: true,
                ..
            }
        )
    }

    pub fn image_edit_enabled(&self) -> bool {
        matches!(
            self,
            Self::Enabled {
                image_edit_enabled: true,
                ..
            }
        )
    }

    /// The configured `image_gen` model override, if any. `None` means the
    /// default quality model ([`XAI_IMAGINE_MODEL`]) is used.
    pub fn model_override(&self) -> Option<&str> {
        match self {
            Self::Enabled { model_override, .. } => {
                model_override.as_deref().filter(|m| !m.trim().is_empty())
            }
            Self::Disabled => None,
        }
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
pub struct ImageGenInput {
    #[schemars(description = "Text description of the image to generate.")]
    pub prompt: String,

    #[serde(default = "default_aspect_ratio")]
    #[schemars(
        description = "Aspect ratio of the generated image, decide it based on the user's request. Defaults to 'auto'. 1:1 for square (icons, profiles), 16:9 for wide (landscapes, cinematic), 9:16 for tall (phone wallpapers, stories), 3:2 for horizontal photos, 2:3 for vertical (portraits, posters)."
    )]
    pub aspect_ratio: String,
}

fn default_aspect_ratio() -> String {
    "auto".to_owned()
}

#[derive(Debug, serde::Deserialize)]
pub struct ImageGenResponse {
    #[serde(default)]
    data: Vec<ImageGenData>,
}

impl ImageGenResponse {
    pub fn b64_data(&self) -> Option<&str> {
        self.data.first().and_then(|d| d.b64_json.as_deref())
    }
}

#[derive(Debug, serde::Deserialize)]
struct ImageGenData {
    b64_json: Option<String>,
}

#[derive(Debug, Default)]
pub struct ImageGenTool;

impl crate::types::tool_metadata::ToolMetadata for ImageGenTool {
    fn kind(&self) -> ToolKind {
        ToolKind::ImageGen
    }

    fn tool_namespace(&self) -> ToolNamespace {
        ToolNamespace::GrokBuild
    }

    fn description_template(&self) -> &str {
        "Generate a new image from a text description using Imagine; returns the saved image's absolute path. When telling the user where it was saved, refer to it by its short session-relative path (e.g. `images/1.jpg`) rather than the absolute path, so it renders as a clickable link that opens the image. To produce multiple images, emit multiple tool calls with distinct prompts."
    }

    fn requires_expr(&self) -> Expr<ToolRequirement> {
        Expr::True
    }
}

impl xai_tool_runtime::Tool for ImageGenTool {
    type Args = ImageGenInput;
    type Output = ToolOutput;

    fn id(&self) -> xai_tool_protocol::ToolId {
        xai_tool_protocol::ToolId::new("image_gen").expect("valid tool id")
    }

    fn description(
        &self,
        _ctx: &::xai_tool_runtime::ListToolsContext,
    ) -> xai_tool_types::ToolDescription {
        xai_tool_types::ToolDescription::new(
            "image_gen",
            crate::types::tool_metadata::ToolMetadata::sanitized_description_template(self),
        )
    }

    fn capabilities(&self) -> xai_tool_protocol::ToolCapabilities {
        xai_tool_protocol::ToolCapabilities {
            is_read_only: false,
            tool_scope: Some(xai_tool_protocol::ToolScope::Write),
            ..Default::default()
        }
    }

    #[tracing::instrument(
        name = "tool.image_gen",
        skip_all,
        fields(prompt_len = input.prompt.len(), aspect_ratio = %input.aspect_ratio)
    )]
    async fn run(
        &self,
        ctx: xai_tool_runtime::ToolCallContext,
        input: ImageGenInput,
    ) -> Result<ToolOutput, xai_tool_runtime::ToolError> {
        use crate::types::tool_metadata::shared_resources;
        let resources = shared_resources(&ctx)?;

        let client = {
            let res = resources.lock().await;
            res.require::<ImageGenClient>()?.clone()
        };

        // Free / X Basic users are zero-limited on Imagine server-side; return
        // the upsell prose instead of a doomed request (the tool stays
        // advertised so the model can surface the nudge in-conversation).
        if client.is_tier_restricted() {
            return Ok(ToolOutput::Text(TIER_RESTRICTED_UPSELL.into()));
        }

        let image_bytes = client.generate(&input.prompt, &input.aspect_ratio).await?;

        let session_folder = {
            let res = resources.lock().await;
            res.require::<SessionFolder>()?.0.clone()
        };

        let absolute_path = client
            .writer
            .save(&session_folder, &image_bytes, None)
            .await
            .map_err(|e| xai_tool_runtime::ToolError::invalid_arguments(e.to_string()))?;

        tracing::info!(
            path = %absolute_path.display(),
            bytes = image_bytes.len(),
            "image saved to disk"
        );

        Ok(ToolOutput::ImageGen(MediaGenOutput::new(absolute_path)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::tool_metadata::test_ctx_with_call_id;
    use wiremock::matchers::{any, body_json, header, method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[test]
    fn image_provider_debug_redacts_static_key() {
        let provider = ImageGenProviderConfig {
            api_key: Some("do-not-log".into()),
            ..Default::default()
        };
        let rendered = format!("{provider:?}");
        assert!(!rendered.contains("do-not-log"));
        assert!(rendered.contains("[redacted]"));
    }

    #[test]
    fn tool_name_and_description() {
        let tool = ImageGenTool;
        assert_eq!(xai_tool_runtime::Tool::id(&tool).as_str(), "image_gen");
        assert!(
            crate::types::tool_metadata::ToolMetadata::description_template(&tool)
                .contains("Generate a new image from a text description")
        );
    }

    #[test]
    fn default_aspect_ratio_is_auto() {
        let input: ImageGenInput = serde_json::from_str(r#"{"prompt": "test"}"#).unwrap();
        assert_eq!(input.aspect_ratio, "auto");
    }

    #[test]
    fn multipart_edit_data_url_decodes_to_binary_part() {
        let part = decode_image_data_url("data:image/png;base64,aGVsbG8=")
            .unwrap()
            .expect("data URL should produce a file part");
        assert_eq!(part.bytes.as_ref(), b"hello");
        assert_eq!(part.filename.as_deref(), Some("image.png"));
        assert_eq!(part.content_type.as_deref(), Some("image/png"));
        assert!(
            decode_image_data_url("https://example.test/image.png")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn per_tool_gates_are_independent() {
        let cfg = ImageGenConfig::Enabled {
            api_key: "k".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: false,
            image_edit_enabled: true,
            model_override: Some("grok-imagine-image".into()),
            edit_model_override: None,
            tier_restricted: false,
            provider: None,
            capability_profile: None,
        };
        assert!(cfg.has_credentials());
        assert!(!cfg.image_gen_enabled());
        assert!(cfg.image_edit_enabled());
        assert_eq!(cfg.model_override(), Some("grok-imagine-image"));

        assert!(!ImageGenConfig::Disabled.has_credentials());
    }

    #[test]
    fn with_session_id_defers_to_caller_configured_header() {
        let mut preset = indexmap::IndexMap::new();
        preset.insert(SESSION_ID_HEADER.to_string(), "caller-set".to_string());
        let cfg = ImageGenConfig::Enabled {
            api_key: "k".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: preset,
            image_gen_enabled: true,
            image_edit_enabled: true,
            model_override: None,
            edit_model_override: None,
            tier_restricted: false,
            provider: None,
            capability_profile: None,
        };
        let client = ImageGenClient::new(&cfg, None)
            .unwrap()
            .with_session_id("sess-1");
        assert!(client.session_header.is_none());

        let cfg_plain = ImageGenConfig::Enabled {
            api_key: "k".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: true,
            model_override: None,
            edit_model_override: None,
            tier_restricted: false,
        };
        let client = ImageGenClient::new(&cfg_plain, None)
            .unwrap()
            .with_session_id("sess-1");
        assert_eq!(
            client.session_header.as_ref().and_then(|v| v.to_str().ok()),
            Some("sess-1")
        );
    }

    // Pins the image_edit wire regression: every POST routes through
    // post_json, which attaches both bearer and session id.
    #[tokio::test]
    async fn post_json_attaches_session_and_bearer_headers() {
        let cfg = ImageGenConfig::Enabled {
            api_key: "k".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: true,
            model_override: None,
            edit_model_override: None,
            tier_restricted: false,
        };
        let client = ImageGenClient::new(&cfg, None)
            .unwrap()
            .with_session_id("sess-42");
        let req = client
            .post_json(
                "https://api.x.ai/v1/images",
                &serde_json::json!({}),
                Some("tok"),
            )
            .build()
            .unwrap();
        assert_eq!(
            req.headers()
                .get(SESSION_ID_HEADER)
                .and_then(|v| v.to_str().ok()),
            Some("sess-42")
        );
        assert_eq!(
            req.headers()
                .get(reqwest::header::AUTHORIZATION)
                .and_then(|v| v.to_str().ok()),
            Some("Bearer tok")
        );
    }

    #[test]
    fn client_selects_model_from_override() {
        let mk = |model_override: Option<&str>| ImageGenConfig::Enabled {
            api_key: "k".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: true,
            model_override: model_override.map(String::from),
            edit_model_override: None,
            tier_restricted: false,
            provider: None,
            capability_profile: None,
        };
        // No override → default quality model.
        assert_eq!(
            ImageGenClient::new(&mk(None), None).unwrap().model,
            XAI_IMAGINE_MODEL
        );
        // Empty override → treated as no override.
        assert_eq!(
            ImageGenClient::new(&mk(Some("")), None).unwrap().model,
            XAI_IMAGINE_MODEL
        );
        // Override → that exact model slug.
        assert_eq!(
            ImageGenClient::new(&mk(Some("grok-imagine-image")), None)
                .unwrap()
                .model,
            "grok-imagine-image"
        );
    }

    #[test]
    fn client_selects_edit_model_from_override() {
        let mk = |edit_model_override: Option<&str>| ImageGenConfig::Enabled {
            api_key: "k".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: true,
            model_override: None,
            edit_model_override: edit_model_override.map(String::from),
            tier_restricted: false,
            provider: None,
            capability_profile: None,
        };
        assert_eq!(
            ImageGenClient::new(&mk(None), None).unwrap().edit_model(),
            super::super::image_edit::XAI_IMAGINE_EDIT_MODEL
        );
        assert_eq!(
            ImageGenClient::new(&mk(Some("  ")), None)
                .unwrap()
                .edit_model(),
            super::super::image_edit::XAI_IMAGINE_EDIT_MODEL
        );
        let client = ImageGenClient::new(&mk(Some("grok-imagine-image-v2")), None).unwrap();
        assert_eq!(client.edit_model(), "grok-imagine-image-v2");
        assert_eq!(client.model, XAI_IMAGINE_MODEL);
    }

    #[tokio::test]
    async fn errors_when_client_missing() {
        let tool = ImageGenTool;
        let resources = crate::types::resources::Resources::new();
        let result = xai_tool_runtime::Tool::run(
            &tool,
            test_ctx_with_call_id(resources.into_shared(), "test-call"),
            ImageGenInput {
                prompt: "a test image".into(),
                aspect_ratio: "auto".into(),
            },
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("missing required resource"),
            "Expected MissingResource error, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn tier_restricted_short_circuits_with_upsell() {
        // A free / X Basic user's image_gen call returns the SuperGrok upsell
        // prose as a normal result (no HTTP, no error card) so the model can
        // relay it. Only the client is inserted — the short-circuit returns
        // before any other resource (e.g. SessionFolder) is required.
        let cfg = ImageGenConfig::Enabled {
            api_key: "k".into(),
            base_url: "https://api.x.ai/v1".into(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: true,
            model_override: None,
            edit_model_override: None,
            tier_restricted: true,
            provider: None,
            capability_profile: None,
        };
        let mut resources = crate::types::resources::Resources::new();
        resources.insert(ImageGenClient::new(&cfg, None).unwrap());

        let result = xai_tool_runtime::Tool::run(
            &ImageGenTool,
            test_ctx_with_call_id(resources.into_shared(), "test-call"),
            ImageGenInput {
                prompt: "a cat".into(),
                aspect_ratio: "auto".into(),
            },
        )
        .await
        .expect("tier-restricted call must succeed with upsell prose");

        match result {
            ToolOutput::Text(t) => {
                assert!(t.text.contains("SuperGrok"), "got: {}", t.text);
                assert!(t.text.contains("supergrok?referrer=grok-build"));
            }
            other => panic!("expected Text upsell, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn profiled_image_provider_uses_mapping_and_x_api_key() {
        let server = MockServer::start().await;
        let profile = xai_grok_provider::CapabilityProviderConfig {
            protocol: "openai_images".into(),
            base_url: Some(server.uri()),
            model: Some("image-model".into()),
            api_key: Some("image-secret".into()),
            auth: xai_grok_provider::ProviderAuthConfig {
                name: "x-api-key".into(),
                prefix: String::new(),
                ..Default::default()
            },
            operations: [(
                "generate".into(),
                xai_grok_provider::CapabilityOperationConfig {
                    method: "POST".into(),
                    path: "/images/generations".into(),
                    request: xai_grok_provider::RequestMapping {
                        fields: [
                            ("model".into(), "model".into()),
                            ("prompt".into(), "prompt".into()),
                            ("size".into(), "size".into()),
                        ]
                        .into_iter()
                        .collect(),
                        ..Default::default()
                    },
                    response: xai_grok_provider::ResponseMapping {
                        items: Some("/data".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        Mock::given(method("POST"))
            .and(path("/images/generations"))
            .and(header("x-api-key", "image-secret"))
            .and(body_json(serde_json::json!({
                "model": "image-model",
                "prompt": "a cat",
                "size": "1:1"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"b64_json": "aGVsbG8="}]
            })))
            .mount(&server)
            .await;
        let config = ImageGenConfig::Enabled {
            api_key: String::new(),
            base_url: server.uri(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: false,
            model_override: None,
            edit_model_override: None,
            tier_restricted: false,
            provider: None,
            capability_profile: Some(profile),
        };
        let client = ImageGenClient::new(&config, None).unwrap();
        assert_eq!(client.generate("a cat", "1:1").await.unwrap(), b"hello");
    }

    #[tokio::test]
    async fn partial_legacy_provider_keeps_native_imagine_wire_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/generations"))
            .and(header("authorization", "Bearer image-provider-key"))
            .and(body_json(serde_json::json!({
                "model": "grok-imagine-image-quality",
                "prompt": "a cat",
                "n": 1,
                "aspect_ratio": "1:1",
                "resolution": "1k",
                "response_format": "b64_json",
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"b64_json": "aGVsbG8="}]
            })))
            .mount(&server)
            .await;
        let config = ImageGenConfig::Enabled {
            api_key: "image-provider-key".into(),
            base_url: server.uri(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: false,
            model_override: None,
            edit_model_override: None,
            tier_restricted: false,
            provider: Some(ImageGenProviderConfig {
                api_key: Some("image-provider-key".into()),
                base_url: server.uri(),
                ..Default::default()
            }),
            capability_profile: None,
        };
        let client = ImageGenClient::new(&config, None).unwrap();
        assert_eq!(client.generate("a cat", "1:1").await.unwrap(), b"hello");
    }

    struct PrimaryKeyProvider;

    impl crate::types::ApiKeyProvider for PrimaryKeyProvider {
        fn current_api_key(&self) -> Option<String> {
            Some("primary-model-key".into())
        }
    }

    #[tokio::test]
    async fn legacy_image_gen_provider_preserves_its_dedicated_key_and_wire_shape() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v3/gpt-image-2-text-to-image"))
            .and(header("authorization", "Bearer image-provider-key"))
            .and(body_json(serde_json::json!({
                "prompt": "a cat",
                "size": "1024x1024",
                "output_format": "png"
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "images": [format!("{}/downloads/image", server.uri())]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/downloads/image"))
            .and(any())
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(b"ppio-image".to_vec()),
            )
            .mount(&server)
            .await;

        let provider = ImageGenProviderConfig {
            api_key: Some("image-provider-key".into()),
            base_url: server.uri(),
            gen_path: "/v3/gpt-image-2-text-to-image".into(),
            edit_path: "/v3/gpt-image-2-edit".into(),
            size_field: "size".into(),
            size_format: SizeFormat::Dimensions,
            response_mode: ResponseMode::Url,
            response_field: "images".into(),
            size_map: [("1:1".into(), "1024x1024".into())].into_iter().collect(),
            extra_fields: [("output_format".into(), serde_json::json!("png"))]
                .into_iter()
                .collect(),
            ..Default::default()
        };
        let config = ImageGenConfig::Enabled {
            api_key: "image-provider-key".into(),
            base_url: server.uri(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: false,
            model_override: None,
            edit_model_override: None,
            tier_restricted: false,
            provider: Some(provider),
            capability_profile: None,
        };
        let client =
            ImageGenClient::new(&config, Some(std::sync::Arc::new(PrimaryKeyProvider))).unwrap();
        assert_eq!(
            client.generate("a cat", "1:1").await.unwrap(),
            b"ppio-image"
        );
    }

    #[tokio::test]
    async fn profiled_image_edit_sends_openai_style_multipart_file() {
        use wiremock::matchers::body_string_contains;

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/edits"))
            .and(header("Authorization", "Bearer image-secret"))
            .and(body_string_contains("name=\"model\""))
            .and(body_string_contains("image-model"))
            .and(body_string_contains("name=\"prompt\""))
            .and(body_string_contains("replace the sky"))
            .and(body_string_contains(
                "name=\"image\"; filename=\"image.png\"",
            ))
            .and(body_string_contains("hello"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"b64_json": "aGVsbG8="}]
            })))
            .mount(&server)
            .await;

        let profile = xai_grok_provider::CapabilityProviderConfig {
            protocol: "openai_images".into(),
            base_url: Some(server.uri()),
            model: Some("image-model".into()),
            api_key: Some("image-secret".into()),
            operations: [(
                "edit".into(),
                xai_grok_provider::CapabilityOperationConfig {
                    method: "POST".into(),
                    path: "/images/edits".into(),
                    request: xai_grok_provider::RequestMapping {
                        body: xai_grok_provider::BodyCodec::Multipart,
                        fields: [
                            ("model".into(), "model".into()),
                            ("prompt".into(), "prompt".into()),
                        ]
                        .into_iter()
                        .collect(),
                        files: [(
                            "image".into(),
                            xai_grok_provider::MultipartPartConfig {
                                field: "image".into(),
                                filename: None,
                                content_type: None,
                            },
                        )]
                        .into_iter()
                        .collect(),
                        ..Default::default()
                    },
                    response: xai_grok_provider::ResponseMapping {
                        items: Some("/data".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let config = ImageGenConfig::Enabled {
            api_key: String::new(),
            base_url: server.uri(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: false,
            image_edit_enabled: true,
            model_override: None,
            edit_model_override: Some("image-model".into()),
            tier_restricted: false,
            provider: None,
            capability_profile: Some(profile),
        };
        let client = ImageGenClient::new(&config, None).unwrap();
        let edited = client
            .edit_profiled(
                "replace the sky",
                vec!["data:image/png;base64,aGVsbG8=".into()],
                "auto",
            )
            .await
            .unwrap();
        assert_eq!(edited, b"hello");
    }

    #[tokio::test]
    async fn profiled_image_generation_downloads_url_without_api_auth() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/images/generations"))
            .and(header("Authorization", "Bearer image-secret"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "data": [{"url": format!("{}/downloads/image", server.uri())}]
            })))
            .mount(&server)
            .await;
        Mock::given(method("GET"))
            .and(path("/downloads/image"))
            .and(any())
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header("content-type", "image/png")
                    .set_body_bytes(b"url-image".to_vec()),
            )
            .mount(&server)
            .await;

        let profile = xai_grok_provider::CapabilityProviderConfig {
            protocol: "openai_images".into(),
            base_url: Some(server.uri()),
            model: Some("image-model".into()),
            api_key: Some("image-secret".into()),
            operations: [(
                "generate".into(),
                xai_grok_provider::CapabilityOperationConfig {
                    method: "POST".into(),
                    path: "/images/generations".into(),
                    request: xai_grok_provider::RequestMapping {
                        fields: [
                            ("model".into(), "model".into()),
                            ("prompt".into(), "prompt".into()),
                        ]
                        .into_iter()
                        .collect(),
                        ..Default::default()
                    },
                    response: xai_grok_provider::ResponseMapping {
                        url: Some("/data/0/url".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let config = ImageGenConfig::Enabled {
            api_key: String::new(),
            base_url: server.uri(),
            extra_headers: indexmap::IndexMap::new(),
            image_gen_enabled: true,
            image_edit_enabled: false,
            model_override: None,
            edit_model_override: None,
            tier_restricted: false,
            provider: None,
            capability_profile: Some(profile),
        };
        let client = ImageGenClient::new(&config, None).unwrap();
        assert_eq!(client.generate("a cat", "1:1").await.unwrap(), b"url-image");

        let download_request = server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|request| request.url.path() == "/downloads/image")
            .expect("image URL download request");
        assert!(!download_request.headers.contains_key("authorization"));
    }
}
