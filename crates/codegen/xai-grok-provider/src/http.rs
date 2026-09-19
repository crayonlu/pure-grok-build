//! Generic capability-provider HTTP runtime.
//!
//! Capability-specific code supplies normalized fields and a profile maps
//! them to the provider wire shape. Adding another HTTP provider should be a
//! data-only profile change whenever its transport semantics are already
//! supported.

use crate::{BodyCodec, CapabilityOperationConfig, CapabilityProviderConfig};
use bytes::Bytes;
use indexmap::IndexMap;
use reqwest::{Method, Request, RequestBuilder, Url};
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BinaryPart {
    pub bytes: Bytes,
    pub filename: Option<String>,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ProviderRequestInput {
    pub values: IndexMap<String, Value>,
    pub binary: IndexMap<String, BinaryPart>,
}

impl ProviderRequestInput {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn value(mut self, name: impl Into<String>, value: impl Into<Value>) -> Self {
        self.values.insert(name.into(), value.into());
        self
    }

    pub fn binary(mut self, name: impl Into<String>, part: BinaryPart) -> Self {
        self.binary.insert(name.into(), part);
        self
    }
}

#[derive(Debug)]
pub struct ProviderHttpRequest {
    pub request: Request,
    pub endpoint: Url,
    pub used_query_credential: bool,
}

#[derive(Debug, Error)]
pub enum ProviderRuntimeError {
    #[error("invalid provider configuration: {0}")]
    Config(String),
    #[error("missing normalized request field `{0}`")]
    MissingField(String),
    #[error("invalid request field mapping `{0}`: {1}")]
    Mapping(String, String),
    #[error("invalid JSON pointer `{0}`")]
    JsonPointer(String),
    #[error("request builder error: {0}")]
    Request(#[from] reqwest::Error),
}

#[derive(Clone)]
pub struct ProviderHttpRuntime {
    client: reqwest::Client,
}

impl ProviderHttpRuntime {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }

    pub fn shared() -> Self {
        Self::new(reqwest::Client::new())
    }

    pub fn build(
        &self,
        profile: &CapabilityProviderConfig,
        operation_name: &str,
        input: &ProviderRequestInput,
        mut getenv: impl FnMut(&str) -> Option<String>,
    ) -> Result<ProviderHttpRequest, ProviderRuntimeError> {
        profile.validate().map_err(ProviderRuntimeError::Config)?;
        let base_url = profile
            .base_url
            .as_deref()
            .ok_or_else(|| ProviderRuntimeError::Config("base_url is required".to_owned()))?;
        let operation = profile.operation(operation_name).ok_or_else(|| {
            ProviderRuntimeError::Config(format!("operation `{operation_name}` is not configured"))
        })?;
        let endpoint = join_endpoint(base_url, &operation.path)?;
        let method = operation
            .effective_method()
            .parse::<Method>()
            .map_err(|error| ProviderRuntimeError::Config(format!("invalid method: {error}")))?;

        let mut url = endpoint.clone();
        let mut query = profile.query_params.clone();
        for (normalized, wire_name) in &operation.request.query {
            let value = input
                .values
                .get(normalized)
                .ok_or_else(|| ProviderRuntimeError::MissingField(normalized.clone()))?;
            query.insert(wire_name.clone(), value_to_query(value)?);
        }
        {
            let mut pairs = url.query_pairs_mut();
            for (name, value) in query {
                pairs.append_pair(&name, &value);
            }
        }

        let mut request = self.client.request(method.clone(), url.clone());
        for (name, value) in &profile.extra_headers {
            request = request.header(name, value);
        }
        for (header_name, env_name) in &profile.env_headers {
            if let Some(value) = getenv(env_name).filter(|value| !value.trim().is_empty()) {
                request = request.header(header_name, value);
            }
        }
        let mut used_query_credential = false;
        if let Some(key) = profile.resolve_api_key(&mut getenv) {
            profile
                .auth
                .validate()
                .map_err(ProviderRuntimeError::Config)?;
            let value = profile.auth.rendered_value(&key);
            if profile.auth.location == "query" {
                tracing::warn!(
                    endpoint = %redact_url(&url),
                    parameter = %profile.auth.name,
                    "provider API credential is being sent in the query string"
                );
                url.query_pairs_mut()
                    .append_pair(&profile.auth.name, &value);
                used_query_credential = true;
                request = self.client.request(method, url.clone());
                for (name, value) in &profile.extra_headers {
                    request = request.header(name, value);
                }
                for (header_name, env_name) in &profile.env_headers {
                    if let Some(value) = getenv(env_name).filter(|value| !value.trim().is_empty()) {
                        request = request.header(header_name, value);
                    }
                }
            } else {
                request = request.header(&profile.auth.name, value);
            }
        }

        request = match operation.request.body {
            BodyCodec::Json => request.json(&build_json_body(operation, profile, input)?),
            BodyCodec::Query => request,
            BodyCodec::Multipart => build_multipart(request, profile, operation, input)?,
            BodyCodec::Binary => {
                let part = input
                    .binary
                    .get("body")
                    .ok_or_else(|| ProviderRuntimeError::MissingField("body".to_owned()))?;
                let mut body = request.body(part.bytes.clone());
                if let Some(content_type) = &part.content_type {
                    body = body.header(reqwest::header::CONTENT_TYPE, content_type);
                }
                body
            }
        };

        let request = request.build()?;
        let endpoint = request.url().clone();
        Ok(ProviderHttpRequest {
            request,
            endpoint,
            used_query_credential,
        })
    }
}

fn join_endpoint(base_url: &str, path: &str) -> Result<Url, ProviderRuntimeError> {
    let mut base = Url::parse(base_url.trim_end_matches('/'))
        .map_err(|error| ProviderRuntimeError::Config(format!("invalid base_url: {error}")))?;
    if !matches!(base.scheme(), "http" | "https") {
        return Err(ProviderRuntimeError::Config(
            "base_url must use http or https".to_owned(),
        ));
    }
    if path.contains("..") {
        return Err(ProviderRuntimeError::Config(
            "operation path must not contain `..`".to_owned(),
        ));
    }
    let path = path.trim();
    if !path.is_empty() {
        let joined = format!(
            "{}/{}",
            base.as_str().trim_end_matches('/'),
            path.trim_start_matches('/')
        );
        base = Url::parse(&joined).map_err(|error| {
            ProviderRuntimeError::Config(format!("invalid operation endpoint: {error}"))
        })?;
    }
    Ok(base)
}

fn value_to_query(value: &Value) -> Result<String, ProviderRuntimeError> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Null => Ok(String::new()),
        _ => serde_json::to_string(value)
            .map_err(|error| ProviderRuntimeError::Mapping("query".to_owned(), error.to_string())),
    }
}

fn build_json_body(
    operation: &CapabilityOperationConfig,
    profile: &CapabilityProviderConfig,
    input: &ProviderRequestInput,
) -> Result<Value, ProviderRuntimeError> {
    let mut body = Value::Object(Map::new());
    for (name, value) in &operation.request.defaults {
        set_wire_value(&mut body, name, value.clone())?;
    }
    for (normalized, wire_name) in &operation.request.fields {
        let value = match input.values.get(normalized) {
            Some(value) => value.clone(),
            None if normalized == "model" => profile
                .model
                .as_ref()
                .map(|model| Value::String(model.clone()))
                .ok_or_else(|| ProviderRuntimeError::MissingField(normalized.clone()))?,
            None => return Err(ProviderRuntimeError::MissingField(normalized.clone())),
        };
        set_wire_value(&mut body, wire_name, value)?;
    }
    Ok(body)
}

fn build_multipart(
    request: RequestBuilder,
    profile: &CapabilityProviderConfig,
    operation: &CapabilityOperationConfig,
    input: &ProviderRequestInput,
) -> Result<RequestBuilder, ProviderRuntimeError> {
    let mut form = reqwest::multipart::Form::new();
    for (normalized, wire_name) in &operation.request.fields {
        if let Some(value) = input.values.get(normalized) {
            form = form.text(wire_name.clone(), value_to_query(value)?);
        } else if normalized == "model" {
            if let Some(model) = profile.model.as_deref() {
                form = form.text(wire_name.clone(), model.to_owned());
            } else if let Some(value) = operation.request.defaults.get(normalized) {
                form = form.text(wire_name.clone(), value_to_query(value)?);
            } else if let Some(value) = operation.request.defaults.get(wire_name) {
                form = form.text(wire_name.clone(), value_to_query(value)?);
            } else {
                return Err(ProviderRuntimeError::MissingField(normalized.clone()));
            }
        } else if let Some(value) = operation.request.defaults.get(normalized) {
            form = form.text(wire_name.clone(), value_to_query(value)?);
        } else if let Some(value) = operation.request.defaults.get(wire_name) {
            form = form.text(wire_name.clone(), value_to_query(value)?);
        } else {
            return Err(ProviderRuntimeError::MissingField(normalized.clone()));
        }
    }
    for (normalized, part_config) in &operation.request.files {
        let input_part = input
            .binary
            .get(normalized)
            .ok_or_else(|| ProviderRuntimeError::MissingField(normalized.clone()))?;
        let mut part = reqwest::multipart::Part::bytes(input_part.bytes.clone().to_vec());
        if let Some(filename) = part_config
            .filename
            .as_deref()
            .or(input_part.filename.as_deref())
        {
            part = part.file_name(filename.to_owned());
        }
        if let Some(content_type) = part_config
            .content_type
            .as_deref()
            .or(input_part.content_type.as_deref())
        {
            part = part.mime_str(content_type).map_err(|error| {
                ProviderRuntimeError::Mapping(normalized.clone(), error.to_string())
            })?;
        }
        form = form.part(part_config.field.clone(), part);
    }
    Ok(request.multipart(form))
}

fn set_wire_value(root: &mut Value, path: &str, value: Value) -> Result<(), ProviderRuntimeError> {
    if path.is_empty() {
        return Err(ProviderRuntimeError::JsonPointer(path.to_owned()));
    }
    if !path.starts_with('/') {
        let object = root
            .as_object_mut()
            .ok_or_else(|| ProviderRuntimeError::JsonPointer(path.to_owned()))?;
        object.insert(path.to_owned(), value);
        return Ok(());
    }

    // `Value::pointer_mut` only addresses nodes that already exist. Provider
    // payloads frequently introduce a nested object (for example
    // `/input/prompt`) through a mapping, so create missing object segments as
    // we walk. Existing arrays remain addressable with numeric segments.
    let tokens = path
        .split('/')
        .skip(1)
        .map(decode_pointer_token)
        .collect::<Result<Vec<_>, _>>()?;
    if tokens.is_empty() {
        return Err(ProviderRuntimeError::JsonPointer(path.to_owned()));
    }
    let mut cursor = root;
    for (index, token) in tokens.iter().enumerate() {
        let last = index + 1 == tokens.len();
        match cursor {
            Value::Object(object) => {
                if last {
                    object.insert(token.clone(), value);
                    return Ok(());
                }
                cursor = object
                    .entry(token.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
            }
            Value::Array(array) => {
                let position = token
                    .parse::<usize>()
                    .map_err(|_| ProviderRuntimeError::JsonPointer(path.to_owned()))?;
                if position == array.len() {
                    array.push(Value::Null);
                }
                let slot = array
                    .get_mut(position)
                    .ok_or_else(|| ProviderRuntimeError::JsonPointer(path.to_owned()))?;
                if last {
                    *slot = value;
                    return Ok(());
                }
                if slot.is_null() {
                    *slot = Value::Object(Map::new());
                }
                cursor = slot;
            }
            _ => return Err(ProviderRuntimeError::JsonPointer(path.to_owned())),
        }
    }
    Err(ProviderRuntimeError::JsonPointer(path.to_owned()))
}

fn decode_pointer_token(token: &str) -> Result<String, ProviderRuntimeError> {
    let mut decoded = String::with_capacity(token.len());
    let mut chars = token.chars();
    while let Some(ch) = chars.next() {
        if ch != '~' {
            decoded.push(ch);
            continue;
        }
        match chars.next() {
            Some('0') => decoded.push('~'),
            Some('1') => decoded.push('/'),
            _ => return Err(ProviderRuntimeError::JsonPointer(token.to_owned())),
        }
    }
    Ok(decoded)
}

/// Extract a value from a JSON response using an RFC 6901 pointer.
pub fn json_pointer<'a>(value: &'a Value, pointer: &str) -> Option<&'a Value> {
    if pointer.is_empty() {
        Some(value)
    } else {
        value.pointer(pointer)
    }
}

fn redact_url(url: &Url) -> String {
    let mut safe = url.clone();
    if safe.query().is_some() {
        safe.set_query(Some("<redacted>"));
    }
    safe.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CapabilityOperationConfig, ProviderAuthConfig, RequestMapping, ResponseMapping};

    fn profile(operation: CapabilityOperationConfig) -> CapabilityProviderConfig {
        CapabilityProviderConfig {
            base_url: Some("https://provider.example/v1".into()),
            api_key: Some("secret".into()),
            operations: [("default".into(), operation)].into_iter().collect(),
            ..Default::default()
        }
    }

    #[test]
    fn builds_brave_style_get_query_and_custom_header() {
        let operation = CapabilityOperationConfig {
            method: "GET".into(),
            path: "/web/search".into(),
            request: RequestMapping {
                body: BodyCodec::Query,
                query: [
                    ("query".into(), "q".into()),
                    ("count".into(), "count".into()),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut profile = profile(operation);
        profile.auth = ProviderAuthConfig {
            location: "header".into(),
            name: "X-Subscription-Token".into(),
            prefix: String::new(),
        };
        let built = ProviderHttpRuntime::new(reqwest::Client::new())
            .build(
                &profile,
                "default",
                &ProviderRequestInput::new()
                    .value("query", "rust")
                    .value("count", 10),
                |_| None,
            )
            .unwrap();
        assert_eq!(built.request.method(), Method::GET);
        assert_eq!(built.request.url().path(), "/v1/web/search");
        assert_eq!(built.request.url().query(), Some("q=rust&count=10"));
        assert_eq!(built.request.headers()["X-Subscription-Token"], "secret");
    }

    #[test]
    fn builds_cohere_style_nested_json_mapping() {
        let operation = CapabilityOperationConfig {
            method: "POST".into(),
            path: "/embed".into(),
            request: RequestMapping {
                fields: [
                    ("model".into(), "model".into()),
                    ("inputs".into(), "texts".into()),
                    ("input_type".into(), "input_type".into()),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            response: ResponseMapping {
                value: Some("/embeddings/float".into()),
                ..Default::default()
            },
            ..Default::default()
        };
        let built = ProviderHttpRuntime::new(reqwest::Client::new())
            .build(
                &profile(operation),
                "default",
                &ProviderRequestInput::new()
                    .value("model", "embed-v4.0")
                    .value("inputs", serde_json::json!(["hello"]))
                    .value("input_type", "search_document"),
                |_| None,
            )
            .unwrap();
        let body: Value =
            serde_json::from_slice(built.request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["texts"][0], "hello");
        assert_eq!(body["input_type"], "search_document");
    }

    #[test]
    fn creates_missing_nested_json_objects_for_provider_mappings() {
        let operation = CapabilityOperationConfig {
            request: RequestMapping {
                fields: [
                    ("prompt".into(), "/input/prompt".into()),
                    ("width".into(), "/parameters/width".into()),
                ]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        let built = ProviderHttpRuntime::new(reqwest::Client::new())
            .build(
                &profile(operation),
                "default",
                &ProviderRequestInput::new()
                    .value("prompt", "a lighthouse")
                    .value("width", 1024),
                |_| None,
            )
            .unwrap();
        let body: Value =
            serde_json::from_slice(built.request.body().unwrap().as_bytes().unwrap()).unwrap();
        assert_eq!(body["input"]["prompt"], "a lighthouse");
        assert_eq!(body["parameters"]["width"], 1024);
    }

    #[test]
    fn builds_stability_style_multipart_with_binary_part() {
        let operation = CapabilityOperationConfig {
            method: "POST".into(),
            path: "/generate/core".into(),
            request: RequestMapping {
                body: BodyCodec::Multipart,
                fields: [("prompt".into(), "prompt".into())].into_iter().collect(),
                files: [(
                    "image".into(),
                    crate::MultipartPartConfig {
                        field: "image".into(),
                        filename: Some("input.png".into()),
                        content_type: Some("image/png".into()),
                    },
                )]
                .into_iter()
                .collect(),
                ..Default::default()
            },
            ..Default::default()
        };
        let built = ProviderHttpRuntime::new(reqwest::Client::new())
            .build(
                &profile(operation),
                "default",
                &ProviderRequestInput::new()
                    .value("prompt", "a lighthouse")
                    .binary(
                        "image",
                        BinaryPart {
                            bytes: Bytes::from_static(b"png"),
                            filename: None,
                            content_type: None,
                        },
                    ),
                |_| None,
            )
            .unwrap();
        assert!(
            built
                .request
                .headers()
                .get("content-type")
                .unwrap()
                .to_str()
                .unwrap()
                .starts_with("multipart/form-data")
        );
    }

    #[test]
    fn query_credentials_are_explicitly_marked() {
        let mut profile = profile(CapabilityOperationConfig::default());
        profile.auth.location = "query".into();
        profile.auth.name = "api_key".into();
        profile.auth.prefix.clear();
        let built = ProviderHttpRuntime::new(reqwest::Client::new())
            .build(
                &profile,
                "default",
                &ProviderRequestInput::default(),
                |_| None,
            )
            .unwrap();
        assert!(built.used_query_credential);
        assert_eq!(built.request.url().query(), Some("api_key=secret"));
    }

    #[test]
    fn json_pointer_reads_nested_response() {
        let value = serde_json::json!({"data": [{"embedding": [1, 2]}]});
        assert_eq!(json_pointer(&value, "/data/0/embedding").unwrap()[0], 1);
    }
}
