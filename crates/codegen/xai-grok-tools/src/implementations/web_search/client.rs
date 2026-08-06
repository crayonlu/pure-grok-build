use super::types::WebSearchConfig;
use crate::attribution::{SharedAttributionCallback, ToolConsumer};
use crate::types::SharedApiKeyProvider;
use async_openai::types::responses as rs;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
/// A minimal, purpose-built HTTP client for calling the Responses API
/// with web search capability.
#[derive(Clone)]
pub struct WebSearchClient {
    http: reqwest::Client,
    base_url: String,
    model: String,
    profile: Option<xai_grok_provider::CapabilityProviderConfig>,
    api_key_provider: Option<SharedApiKeyProvider>,
    /// Optional 401-attribution hook. Callers can wire this so a 401
    /// from the Responses API emits an `auth_401_attribution` event
    /// with `consumer == "WebSearch"`.
    attribution_callback: Option<SharedAttributionCallback>,
}
impl WebSearchClient {
    /// Create a new web search client from `WebSearchConfig::Enabled`.
    ///
    /// Returns `Err` if the config is `Disabled` or if header values are invalid.
    pub fn new(
        config: &WebSearchConfig,
        api_key_provider: Option<SharedApiKeyProvider>,
    ) -> Result<Self, xai_tool_runtime::ToolError> {
        let ((base_url, model), default_headers, profile) = match config {
            WebSearchConfig::Enabled {
                api_key,
                base_url,
                model,
                extra_headers,
                alpha_test_key,
            } => {
                let mut headers = HeaderMap::new();
                headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
                headers.insert(
                    AUTHORIZATION,
                    HeaderValue::from_str(&format!("Bearer {api_key}")).map_err(|e| {
                        xai_tool_runtime::ToolError::execution(
                            xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                            format!("Invalid API key for header: {e}"),
                        )
                    })?,
                );
                for (key, value) in extra_headers {
                    let header_name = HeaderName::from_bytes(key.as_bytes()).map_err(|e| {
                        xai_tool_runtime::ToolError::execution(
                            xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                            format!("Invalid header name '{key}': {e}"),
                        )
                    })?;
                    let header_value = HeaderValue::from_str(value).map_err(|e| {
                        xai_tool_runtime::ToolError::execution(
                            xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                            format!("Invalid header value for '{key}': {e}"),
                        )
                    })?;
                    headers.insert(header_name, header_value);
                }
                let _ = alpha_test_key;
                ((base_url.clone(), model.clone()), Some(headers), None)
            }
            WebSearchConfig::Profiled { profile } => {
                let base_url = profile.base_url.clone().ok_or_else(|| {
                    xai_tool_runtime::ToolError::invalid_arguments(
                        "profiled web search requires base_url",
                    )
                })?;
                (
                    (base_url, profile.model.clone().unwrap_or_default()),
                    None,
                    Some(profile.clone()),
                )
            }
            WebSearchConfig::Disabled => {
                return Err(xai_tool_runtime::ToolError::execution(
                    xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                    "Cannot create WebSearchClient from disabled config".to_string(),
                ));
            }
        };
        let http = xai_grok_extra_ca::with_extra_root_certificates(match default_headers {
            Some(headers) => reqwest::Client::builder().default_headers(headers),
            None => reqwest::Client::builder(),
        })
        .build()
        .map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Failed to build HTTP client: {e}"),
            )
        })?;
        Ok(Self {
            http,
            base_url: base_url.clone(),
            model: model.clone(),
            profile,
            api_key_provider,
            attribution_callback: None,
        })
    }
    /// Wire a 401-attribution callback into this client. Idempotent;
    /// safe to call before or after the first request.
    pub fn with_attribution_callback(
        mut self,
        callback: Option<SharedAttributionCallback>,
    ) -> Self {
        self.attribution_callback = callback;
        self
    }
    async fn current_bearer(&self) -> Option<String> {
        crate::types::api_key_provider::resolve_bearer(self.api_key_provider.as_ref()).await
    }
    fn record_401_attribution(&self, sent_bearer: Option<&str>) {
        crate::attribution::emit_401(
            self.attribution_callback.as_ref(),
            ToolConsumer::WebSearch,
            sent_bearer,
        );
    }
    /// Perform a web search query using the Responses API.
    ///
    /// Returns `(content, citations)` where content is the assistant's text
    /// and citations are unique URLs found in the response annotations.
    pub async fn search(
        &self,
        query: &str,
        allowed_domains: Option<Vec<String>>,
    ) -> Result<(String, Vec<String>), xai_tool_runtime::ToolError> {
        if self.profile.is_some() {
            return self.search_profile(query, allowed_domains).await;
        }
        let web_search = rs::WebSearchToolArgs::default()
            .filters(rs::WebSearchToolFilters { allowed_domains })
            .build()
            .map_err(|e| {
                xai_tool_runtime::ToolError::execution(
                    xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                    format!("Failed to build web search tool: {e}"),
                )
            })?;
        let request = rs::CreateResponseArgs::default()
            .model(self.model.clone())
            .input(query.to_string())
            .tools(vec![rs::Tool::WebSearch(web_search)])
            .store(false)
            .temperature(0.1)
            .top_p(0.95)
            .max_output_tokens(8192u32)
            .build()
            .map_err(|e| {
                xai_tool_runtime::ToolError::execution(
                    xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                    format!("Failed to build request: {e}"),
                )
            })?;
        let url = format!("{}/responses", self.base_url.trim_end_matches('/'));
        let sent_bearer = self.current_bearer().await;
        let mut req = self.http.post(&url).json(&request);
        if let Some(ref key) = sent_bearer {
            req = req.header(AUTHORIZATION, format!("Bearer {key}"));
        }
        let response = req.send().await.map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("HTTP request failed: {e}"),
            )
        })?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            self.record_401_attribution(sent_bearer.as_deref());
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());
            return Err(xai_tool_runtime::ToolError::unauthorized(format!(
                "Responses API returned 401 Unauthorized: {body}"
            ))
            .with_details(serde_json::json!({
                "tool_id": "web_search",
                "status": 401,
            })));
        }
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());
            return Err(xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Responses API returned {status}: {body}"),
            ));
        }
        let bytes = response.bytes().await.map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Failed to read response body: {e}"),
            )
        })?;
        let response_obj: rs::Response = serde_json::from_slice(&bytes).map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Failed to parse response: {e}"),
            )
        })?;
        let content = response_obj
            .output_text()
            .unwrap_or_else(|| "No search results found.".to_string());
        let citations = extract_citations(&response_obj);
        Ok((content, citations))
    }

    async fn search_profile(
        &self,
        query: &str,
        allowed_domains: Option<Vec<String>>,
    ) -> Result<(String, Vec<String>), xai_tool_runtime::ToolError> {
        let profile = self.profile.as_ref().expect("checked by caller");
        let input = xai_grok_provider::ProviderRequestInput::new()
            .value("query", query.to_owned())
            .value("max_results", 10)
            .value(
                "allowed_domains",
                serde_json::to_value(allowed_domains.unwrap_or_default()).unwrap_or_default(),
            );
        let built = xai_grok_provider::ProviderHttpRuntime::new(self.http.clone())
            .build(profile, "search", &input, |name| std::env::var(name).ok())
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
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Generic search request failed: {error}"),
            )
        })?;
        let status = response.status();
        let body: serde_json::Value = response.json().await.map_err(|error| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Generic search response was not JSON: {error}"),
            )
        })?;
        if !status.is_success() {
            return Err(xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Generic search returned HTTP {status}: {body}"),
            ));
        }
        let mapping = profile
            .operation("search")
            .or_else(|| profile.operation("default"))
            .map(|operation| &operation.response)
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments("search profile has no operation")
            })?;
        let items = mapping
            .items
            .as_deref()
            .and_then(|pointer| xai_grok_provider::json_pointer(&body, pointer))
            .and_then(serde_json::Value::as_array)
            .ok_or_else(|| {
                xai_tool_runtime::ToolError::invalid_arguments(
                    "search profile response.items is not an array",
                )
            })?;
        let mut citations = Vec::new();
        let mut content = String::new();
        for (index, item) in items.iter().enumerate() {
            let url = mapping
                .url
                .as_deref()
                .and_then(|pointer| item.pointer(pointer))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            if url.is_empty() {
                continue;
            }
            let title = mapping
                .title
                .as_deref()
                .and_then(|pointer| item.pointer(pointer))
                .and_then(serde_json::Value::as_str)
                .unwrap_or(url);
            let snippet = mapping
                .content
                .as_deref()
                .and_then(|pointer| item.pointer(pointer))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            citations.push(url.to_owned());
            content.push_str(&format!("{}. [{}]({})", index + 1, title, url));
            if !snippet.is_empty() {
                content.push_str(&format!(" — {snippet}"));
            }
            content.push('\n');
        }
        Ok((content.trim_end().to_owned(), citations))
    }
    /// Same as [`Self::search`] but also extracts per-citation titles when
    /// the Responses API surfaces them. Returns `(content, citations_with_titles)`
    /// where each citation is `(title, url)`. Empty `title` strings indicate
    /// the upstream didn't supply one for that URL.
    ///
    /// Used by the cursor-compat `WebSearch` adapter to render a
    /// `Links:\n1. [title](url)` list instead of the LLM synthesis text.
    pub async fn search_with_titles(
        &self,
        query: &str,
        allowed_domains: Option<Vec<String>>,
    ) -> Result<(String, Vec<(String, String)>), xai_tool_runtime::ToolError> {
        if self.profile.is_some() {
            let (content, urls) = self.search_profile(query, allowed_domains).await?;
            return Ok((
                content,
                urls.into_iter().map(|url| (String::new(), url)).collect(),
            ));
        }
        let web_search = rs::WebSearchToolArgs::default()
            .filters(rs::WebSearchToolFilters { allowed_domains })
            .build()
            .map_err(|e| {
                xai_tool_runtime::ToolError::execution(
                    xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                    format!("Failed to build web search tool: {e}"),
                )
            })?;
        let request = rs::CreateResponseArgs::default()
            .model(self.model.clone())
            .input(query.to_string())
            .tools(vec![rs::Tool::WebSearch(web_search)])
            .store(false)
            .temperature(0.1)
            .top_p(0.95)
            .max_output_tokens(8192u32)
            .build()
            .map_err(|e| {
                xai_tool_runtime::ToolError::execution(
                    xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                    format!("Failed to build request: {e}"),
                )
            })?;
        let url = format!("{}/responses", self.base_url.trim_end_matches('/'));
        let sent_bearer = self.current_bearer().await;
        let mut req = self.http.post(&url).json(&request);
        if let Some(ref key) = sent_bearer {
            req = req.header(AUTHORIZATION, format!("Bearer {key}"));
        }
        let response = req.send().await.map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("HTTP request failed: {e}"),
            )
        })?;
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            self.record_401_attribution(sent_bearer.as_deref());
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());
            return Err(xai_tool_runtime::ToolError::unauthorized(format!(
                "Responses API returned 401 Unauthorized: {body}"
            ))
            .with_details(serde_json::json!({
                "tool_id": "web_search",
                "status": 401,
            })));
        }
        if !status.is_success() {
            let body = response
                .text()
                .await
                .unwrap_or_else(|_| "Failed to read error body".to_string());
            return Err(xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Responses API returned {status}: {body}"),
            ));
        }
        let bytes = response.bytes().await.map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Failed to read response body: {e}"),
            )
        })?;
        let response_obj: rs::Response = serde_json::from_slice(&bytes).map_err(|e| {
            xai_tool_runtime::ToolError::execution(
                xai_tool_protocol::ToolId::new("web_search").expect("valid"),
                format!("Failed to parse response: {e}"),
            )
        })?;
        let content = response_obj
            .output_text()
            .unwrap_or_else(|| "No search results found.".to_string());
        let pairs = extract_citation_pairs(&response_obj);
        Ok((content, pairs))
    }
}
/// Extract citation URLs from the Response output items.
/// The async-openai crate doesn't provide a helper for this, and the `url` field
/// in `UrlCitationBody` is private, so we serialize to JSON to extract it.
fn extract_citations(response: &rs::Response) -> Vec<String> {
    let mut citations = Vec::new();
    for output_item in &response.output {
        if let rs::OutputItem::Message(output_message) = output_item {
            for message_content in &output_message.content {
                if let rs::OutputMessageContent::OutputText(text_content) = message_content {
                    for annotation in &text_content.annotations {
                        if let rs::Annotation::UrlCitation(url_citation) = annotation
                            && let Ok(json) = serde_json::to_value(url_citation)
                            && let Some(url) = json.get("url").and_then(|v| v.as_str())
                        {
                            citations.push(url.to_string());
                        }
                    }
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    citations.retain(|url| seen.insert(url.clone()));
    citations
}
/// Extract `(title, url)` pairs from the Responses API annotations.
///
/// `title` may be an empty string when upstream doesn't supply one. URLs
/// are deduplicated while preserving the first-seen order so the rendered
/// `Links:` list is stable and free of duplicates.
fn extract_citation_pairs(response: &rs::Response) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for output_item in &response.output {
        if let rs::OutputItem::Message(output_message) = output_item {
            for message_content in &output_message.content {
                if let rs::OutputMessageContent::OutputText(text_content) = message_content {
                    for annotation in &text_content.annotations {
                        if let rs::Annotation::UrlCitation(url_citation) = annotation
                            && let Ok(json) = serde_json::to_value(url_citation)
                        {
                            let url = json.get("url").and_then(|v| v.as_str()).unwrap_or("");
                            if url.is_empty() {
                                continue;
                            }
                            let title = json
                                .get("title")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            pairs.push((title, url.to_string()));
                        }
                    }
                }
            }
        }
    }
    let mut seen = std::collections::HashSet::new();
    pairs.retain(|(_t, url)| seen.insert(url.clone()));
    pairs
}
#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;
    /// Helper to create a Response from JSON for testing.
    fn response_from_json(json: serde_json::Value) -> rs::Response {
        serde_json::from_value(json).expect("Failed to parse test Response JSON")
    }
    #[test]
    fn test_new_client_uses_configured_model() {
        let config = WebSearchConfig::Enabled {
            api_key: "test-key".to_string(),
            base_url: "https://api.x.ai/v1".to_string(),
            model: "custom-enterprise-model".to_string(),
            extra_headers: IndexMap::new(),
            alpha_test_key: None,
        };
        let client = WebSearchClient::new(&config, None).expect("client should build");
        assert_eq!(client.model, "custom-enterprise-model");
    }
    /// Counts attribution callback invocations for the test below.
    #[derive(Default, Debug)]
    struct CountingCallback {
        invocations: std::sync::Mutex<Vec<(ToolConsumer, Option<String>)>>,
    }
    impl crate::attribution::Auth401AttributionCallback for CountingCallback {
        fn record_401(&self, consumer: ToolConsumer, sent_bearer_suffix: Option<&str>) {
            self.invocations
                .lock()
                .unwrap()
                .push((consumer, sent_bearer_suffix.map(|s| s.to_string())));
        }
    }
    /// `record_401_attribution` invokes the wired callback with
    /// `ToolConsumer::WebSearch` and the truncated bearer prefix.
    /// The full bearer never crosses the trait boundary.
    #[test]
    fn record_401_attribution_passes_truncated_prefix_to_callback() {
        let cb = std::sync::Arc::new(CountingCallback::default());
        let cb_dyn: crate::attribution::SharedAttributionCallback = cb.clone();
        let config = WebSearchConfig::Enabled {
            api_key: "ignored".to_string(),
            base_url: "https://api.x.ai/v1".to_string(),
            model: "test-model".to_string(),
            extra_headers: IndexMap::new(),
            alpha_test_key: None,
        };
        let client = WebSearchClient::new(&config, None)
            .expect("client should build")
            .with_attribution_callback(Some(cb_dyn));
        client.record_401_attribution(Some("bearer-with-long-tail-aaaadistinct"));
        let calls = cb.invocations.lock().unwrap();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].0, ToolConsumer::WebSearch);
        assert_eq!(calls[0].1.as_deref(), Some("aaaadistinct"));
        assert_eq!(
            calls[0].1.as_deref().map(str::len),
            Some(crate::attribution::BEARER_SUFFIX_LEN),
        );
    }
    /// `record_401_attribution` is a no-op when no callback is wired
    /// -- the BYOK / standalone case must not panic or allocate.
    #[test]
    fn record_401_attribution_is_noop_without_callback() {
        let config = WebSearchConfig::Enabled {
            api_key: "test-key".to_string(),
            base_url: "https://api.x.ai/v1".to_string(),
            model: "test-model".to_string(),
            extra_headers: IndexMap::new(),
            alpha_test_key: None,
        };
        let client = WebSearchClient::new(&config, None).expect("client should build");
        client.record_401_attribution(Some("any-bearer"));
        client.record_401_attribution(None);
    }
    #[test]
    fn test_extract_citations_empty_response() {
        let response = response_from_json(serde_json::json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 1234567890,
            "status": "completed",
            "output": [],
            "model": "test-model"
        }));
        let citations = extract_citations(&response);
        assert!(citations.is_empty());
    }
    #[test]
    fn test_extract_citations_with_url_citations() {
        let response = response_from_json(serde_json::json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 1234567890,
            "status": "completed",
            "model": "test-model",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Here is some info about Rust.",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "url": "https://www.rust-lang.org/",
                                    "title": "Rust Programming Language",
                                    "start_index": 0,
                                    "end_index": 10
                                },
                                {
                                    "type": "url_citation",
                                    "url": "https://docs.rs/",
                                    "title": "Docs.rs",
                                    "start_index": 11,
                                    "end_index": 20
                                }
                            ]
                        }
                    ]
                }
            ]
        }));
        let citations = extract_citations(&response);
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0], "https://www.rust-lang.org/");
        assert_eq!(citations[1], "https://docs.rs/");
    }
    #[test]
    fn test_extract_citations_deduplicates() {
        let response = response_from_json(serde_json::json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 1234567890,
            "status": "completed",
            "model": "test-model",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Info with duplicate citations.",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "url": "https://example.com/page1",
                                    "title": "Page 1",
                                    "start_index": 0,
                                    "end_index": 5
                                },
                                {
                                    "type": "url_citation",
                                    "url": "https://example.com/page2",
                                    "title": "Page 2",
                                    "start_index": 6,
                                    "end_index": 10
                                },
                                {
                                    "type": "url_citation",
                                    "url": "https://example.com/page1",
                                    "title": "Page 1 Again",
                                    "start_index": 11,
                                    "end_index": 15
                                }
                            ]
                        }
                    ]
                }
            ]
        }));
        let citations = extract_citations(&response);
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0], "https://example.com/page1");
        assert_eq!(citations[1], "https://example.com/page2");
    }
    #[test]
    fn test_extract_citations_multiple_messages() {
        let response = response_from_json(serde_json::json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 1234567890,
            "status": "completed",
            "model": "test-model",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "First message",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "url": "https://first.com/",
                                    "title": "First",
                                    "start_index": 0,
                                    "end_index": 5
                                }
                            ]
                        }
                    ]
                },
                {
                    "type": "message",
                    "id": "msg_2",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Second message",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "url": "https://second.com/",
                                    "title": "Second",
                                    "start_index": 0,
                                    "end_index": 6
                                }
                            ]
                        }
                    ]
                }
            ]
        }));
        let citations = extract_citations(&response);
        assert_eq!(citations.len(), 2);
        assert_eq!(citations[0], "https://first.com/");
        assert_eq!(citations[1], "https://second.com/");
    }
    #[test]
    fn test_extract_citations_ignores_non_url_annotations() {
        let response = response_from_json(serde_json::json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 1234567890,
            "status": "completed",
            "model": "test-model",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Some text",
                            "annotations": [
                                {
                                    "type": "url_citation",
                                    "url": "https://valid.com/",
                                    "title": "Valid",
                                    "start_index": 0,
                                    "end_index": 4
                                }
                            ]
                        }
                    ]
                }
            ]
        }));
        let citations = extract_citations(&response);
        assert_eq!(citations.len(), 1);
        assert_eq!(citations[0], "https://valid.com/");
    }
    /// A provider that always returns `None`, simulating an API-key user
    /// whose token has aged past the client-side TTL.
    struct NoneProvider;
    impl crate::types::ApiKeyProvider for NoneProvider {
        fn current_api_key(&self) -> Option<String> {
            None
        }
    }

    #[tokio::test]
    async fn profiled_brave_search_uses_query_auth_and_normalizes_results() {
        use wiremock::matchers::{header, method, path, query_param};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/res/v1/web/search"))
            .and(query_param("q", "rust"))
            .and(header("X-Subscription-Token", "brave-key"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "web": {"results": [{
                    "title": "Rust",
                    "url": "https://www.rust-lang.org",
                    "description": "A language empowering everyone"
                }]}
            })))
            .mount(&server)
            .await;

        let profile = xai_grok_provider::CapabilityProviderConfig {
            base_url: Some(server.uri()),
            api_key: Some("brave-key".into()),
            auth: xai_grok_provider::ProviderAuthConfig {
                name: "X-Subscription-Token".into(),
                prefix: String::new(),
                ..Default::default()
            },
            operations: [(
                "search".into(),
                xai_grok_provider::CapabilityOperationConfig {
                    method: "GET".into(),
                    path: "/res/v1/web/search".into(),
                    request: xai_grok_provider::RequestMapping {
                        body: xai_grok_provider::BodyCodec::Query,
                        query: [("query".into(), "q".into())].into_iter().collect(),
                        ..Default::default()
                    },
                    response: xai_grok_provider::ResponseMapping {
                        items: Some("/web/results".into()),
                        title: Some("/title".into()),
                        url: Some("/url".into()),
                        content: Some("/description".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };
        let client = WebSearchClient::new(&WebSearchConfig::Profiled { profile }, None).unwrap();
        let (content, citations) = client.search("rust", None).await.unwrap();
        assert!(content.contains("Rust"));
        assert!(content.contains("A language empowering everyone"));
        assert_eq!(citations, vec!["https://www.rust-lang.org"]);
    }

    #[tokio::test]
    async fn profiled_tavily_search_uses_post_bearer_and_nested_results() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/search"))
            .and(header("Authorization", "Bearer tavily-key"))
            .and(body_json(serde_json::json!({
                "query": "rust async",
                "max_results": 10,
                "include_answer": true,
                "include_domains": ["rust-lang.org"]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "results": [{
                    "title": "Async Rust",
                    "url": "https://www.rust-lang.org/async",
                    "content": "Async Rust uses futures and executors."
                }]
            })))
            .mount(&server)
            .await;

        let profile = xai_grok_provider::CapabilityProviderConfig {
            protocol: "tavily".into(),
            base_url: Some(server.uri()),
            api_key: Some("tavily-key".into()),
            operations: [(
                "search".into(),
                xai_grok_provider::CapabilityOperationConfig {
                    method: "POST".into(),
                    path: "/search".into(),
                    request: xai_grok_provider::RequestMapping {
                        fields: [
                            ("query".into(), "query".into()),
                            ("max_results".into(), "max_results".into()),
                            ("allowed_domains".into(), "include_domains".into()),
                        ]
                        .into_iter()
                        .collect(),
                        defaults: [("include_answer".into(), serde_json::json!(true))]
                            .into_iter()
                            .collect(),
                        ..Default::default()
                    },
                    response: xai_grok_provider::ResponseMapping {
                        items: Some("/results".into()),
                        title: Some("/title".into()),
                        url: Some("/url".into()),
                        content: Some("/content".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        let client = WebSearchClient::new(&WebSearchConfig::Profiled { profile }, None).unwrap();
        let (content, citations) = client
            .search("rust async", Some(vec!["rust-lang.org".into()]))
            .await
            .unwrap();
        assert!(content.contains("Async Rust"));
        assert!(content.contains("Async Rust uses futures"));
        assert_eq!(citations, vec!["https://www.rust-lang.org/async"]);
    }

    #[tokio::test]
    async fn profiled_firecrawl_search_uses_v2_shape_and_data_web_results() {
        use wiremock::matchers::{body_json, header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/v2/search"))
            .and(header("Authorization", "Bearer firecrawl-key"))
            .and(body_json(serde_json::json!({
                "query": "rust async",
                "limit": 10,
                "includeDomains": ["rust-lang.org"]
            })))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "success": true,
                "data": {
                    "web": [{
                        "title": "Async Rust",
                        "url": "https://www.rust-lang.org/async",
                        "description": "Async Rust uses futures and executors."
                    }]
                }
            })))
            .mount(&server)
            .await;

        let profile = xai_grok_provider::CapabilityProviderConfig {
            protocol: "firecrawl".into(),
            base_url: Some(format!("{}/v2", server.uri())),
            api_key: Some("firecrawl-key".into()),
            operations: [(
                "search".into(),
                xai_grok_provider::CapabilityOperationConfig {
                    method: "POST".into(),
                    path: "/search".into(),
                    request: xai_grok_provider::RequestMapping {
                        body: xai_grok_provider::BodyCodec::Json,
                        fields: [
                            ("query".into(), "query".into()),
                            ("max_results".into(), "limit".into()),
                            ("allowed_domains".into(), "includeDomains".into()),
                        ]
                        .into_iter()
                        .collect(),
                        ..Default::default()
                    },
                    response: xai_grok_provider::ResponseMapping {
                        items: Some("/data/web".into()),
                        title: Some("/title".into()),
                        url: Some("/url".into()),
                        content: Some("/description".into()),
                        ..Default::default()
                    },
                    ..Default::default()
                },
            )]
            .into_iter()
            .collect(),
            ..Default::default()
        };

        let client = WebSearchClient::new(&WebSearchConfig::Profiled { profile }, None).unwrap();
        let (content, citations) = client
            .search("rust async", Some(vec!["rust-lang.org".into()]))
            .await
            .unwrap();
        assert!(content.contains("Async Rust"));
        assert!(content.contains("Async Rust uses futures"));
        assert_eq!(citations, vec!["https://www.rust-lang.org/async"]);
    }

    /// When the dynamic provider returns `None`, the static `api_key`
    /// from config must still be sent as the Authorization header.
    /// This is a regression scenario: API-key users
    /// past the 30-day client TTL saw 401 because no auth was sent.
    #[tokio::test]
    async fn static_api_key_is_fallback_when_provider_returns_none() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(header("Authorization", "Bearer static-key-from-config"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "resp_test",
                "object": "response",
                "created_at": 1234567890,
                "status": "completed",
                "model": "test-model",
                "output": [{
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "search result",
                        "annotations": []
                    }]
                }]
            })))
            .mount(&server)
            .await;
        let config = WebSearchConfig::Enabled {
            api_key: "static-key-from-config".to_string(),
            base_url: server.uri(),
            model: "test-model".to_string(),
            extra_headers: IndexMap::new(),
            alpha_test_key: None,
        };
        let provider: SharedApiKeyProvider = std::sync::Arc::new(NoneProvider);
        let client = WebSearchClient::new(&config, Some(provider)).expect("client should build");
        let (content, _citations) = client
            .search("test query", None)
            .await
            .expect("search must succeed with static key fallback");
        assert_eq!(content, "search result");
    }
    /// When the provider returns a fresh key, it overrides the static one.
    #[tokio::test]
    async fn provider_key_overrides_static_key() {
        use wiremock::matchers::{header, method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};
        struct FreshProvider;
        impl crate::types::ApiKeyProvider for FreshProvider {
            fn current_api_key(&self) -> Option<String> {
                Some("fresh-key-from-provider".to_string())
            }
        }
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/responses"))
            .and(header("Authorization", "Bearer fresh-key-from-provider"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "resp_test",
                "object": "response",
                "created_at": 1234567890,
                "status": "completed",
                "model": "test-model",
                "output": [{
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [{
                        "type": "output_text",
                        "text": "fresh result",
                        "annotations": []
                    }]
                }]
            })))
            .mount(&server)
            .await;
        let config = WebSearchConfig::Enabled {
            api_key: "stale-static-key".to_string(),
            base_url: server.uri(),
            model: "test-model".to_string(),
            extra_headers: IndexMap::new(),
            alpha_test_key: None,
        };
        let provider: SharedApiKeyProvider = std::sync::Arc::new(FreshProvider);
        let client = WebSearchClient::new(&config, Some(provider)).expect("client should build");
        let (content, _citations) = client
            .search("test query", None)
            .await
            .expect("search must succeed with provider key");
        assert_eq!(content, "fresh result");
    }
    #[test]
    fn test_extract_citations_no_annotations() {
        let response = response_from_json(serde_json::json!({
            "id": "resp_test",
            "object": "response",
            "created_at": 1234567890,
            "status": "completed",
            "model": "test-model",
            "output": [
                {
                    "type": "message",
                    "id": "msg_1",
                    "status": "completed",
                    "role": "assistant",
                    "content": [
                        {
                            "type": "output_text",
                            "text": "Plain text with no annotations",
                            "annotations": []
                        }
                    ]
                }
            ]
        }));
        let citations = extract_citations(&response);
        assert!(citations.is_empty());
    }
}
