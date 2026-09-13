# Fork Configuration Guide

This fork is designed for self-hosting and bring-your-own-key (BYOK) use.
The default operating mode is provider-neutral `open`: the runtime does not
infer a provider from a hostname, an API key, or a login credential.

The runtime configuration file is `~/.grok/config.toml`.

## What this fork changes

Compared with upstream `grok-build`, this fork:

- defaults to `[fork].mode = "open"`;
- treats model `base_url`, `api_backend`, and credentials as explicit user
  configuration;
- supports per-model API keys and environment-variable key references;
- keeps upstream OAuth, cloud, and auxiliary implementations only as merge
  compatibility code;
- does not automatically bind an external key to a first-party provider;
- enables auxiliary capabilities only through an explicit provider profile;
- keeps xAI cloud/sandbox/deploy surfaces behind `xai_compat` or disabled when
  the corresponding upstream implementation is not built.

The Open path is therefore suitable for any endpoint that implements one of
the supported request protocols. The endpoint must be configured explicitly.

## Minimal Open-mode BYOK setup

Create or edit `~/.grok/config.toml`:

```toml
[fork]
mode = "open"

[auth]
# Keep authentication key-based and avoid interactive OAuth.
preferred_method = "api_key"

[features]
# Open mode already defaults this to false; keeping it explicit makes the
# configuration auditable and prevents remote settings/feature fetches.
remote_fetch = false

[models]
default = "my-model"

[model.my-model]
# Catalog ID is `my-model`; this is the routing name sent to the endpoint.
model = "provider/model-name"
base_url = "https://gateway.example/v1"
api_backend = "chat_completions"
env_key = "MY_PROVIDER_API_KEY"
context_window = 128000
```

Export the key and start the CLI:

```sh
export MY_PROVIDER_API_KEY="sk-..."
grok models
grok -m my-model
```

Use `api_key = "..."` instead of `env_key` only when a literal key is
acceptable in the local configuration. Environment variables are preferred.

## Selecting the request protocol

`api_backend` is explicit and is not inferred from the hostname:

| Value | Request path | Typical use |
|---|---|---|
| `chat_completions` | `{base_url}/chat/completions` | OpenAI-compatible chat endpoints |
| `responses` | `{base_url}/responses` | Responses-compatible endpoints |
| `messages` | `{base_url}/messages` | Messages-compatible endpoints |

For example:

```toml
[model.responses-model]
model = "reasoning-model"
base_url = "https://gateway.example/v1"
api_backend = "responses"
env_key = "GATEWAY_API_KEY"
context_window = 200000
reasoning_effort = "high"
reasoning_efforts = ["low", "medium", "high"]
```

`reasoning_effort` and `reasoning_efforts` are model metadata. The model
endpoint still determines which effort values are actually accepted.

## Credential resolution

For a model request, the effective key is resolved in this order:

1. non-empty `api_key` on `[model.<id>]`;
2. the first non-empty variable listed by `env_key`;
3. an explicitly configured named auth provider token;
4. an active local API-key session;
5. `XAI_API_KEY`, then the legacy `GROK_CODE_XAI_API_KEY` fallback.

When a model has its own `api_key` or `env_key`, it does not silently inherit a
different session key. This makes the selected model's credential auditable.

`env_key` may be a string or an ordered list:

```toml
env_key = ["MY_PROVIDER_API_KEY", "LC_MY_PROVIDER_API_KEY"]
```

You can also store a key in the local auth store:

```sh
grok login --api-key "$MY_PROVIDER_API_KEY"
```

This stores the key locally under the API-key credential scope. It does not
start an OAuth flow.

## Memory and embeddings

Memory is disabled unless you explicitly enable it with `[memory].enabled`,
`GROK_MEMORY=1`, or `--experimental-memory`:

```toml
[memory]
enabled = true

[memory.embedding]
provider = "api"
base_url = "https://embedding.example/v1"
model = "your-embedding-model"
env_key = "EMBEDDING_API_KEY"
# api_key = "..." # optional; takes precedence over env_key
auth_scheme = "bearer" # bearer or x_api_key
dimensions = 1024

[memory.embedding.extra_headers]
# X-Provider-Project = "project-id"

[memory.search]
max_results = 6
min_score = 0.35
```

The embedding client calls the OpenAI-compatible `POST {base_url}/embeddings`
endpoint. `base_url`, credentials, authentication scheme, and extra headers
are independent from the chat model. The endpoint must return the standard
embedding response shape and the configured `dimensions` must match the
provider's vector size.

For Cohere Embed v2, set `protocol = "cohere_v2"`; the resolver switches the
path to `/v2/embed`, maps `input` to `texts`, adds `input_type =
"search_document"` and `embedding_types = ["float"]`, and maps `dimensions` to
`output_dimension`. Cohere v1/legacy is also supported with
`protocol = "cohere"` (or `"cohere_v1"`), using `/embed` and the plain
`embeddings` array response. A custom `path`, request mapping, or response
mapping can override those defaults for a gateway.

`api_key` takes precedence over `env_key`. `env_key` may be a string or an
ordered list; the first set, non-blank variable wins. If `base_url` or a
credential is omitted, the active model's endpoint or static BYOK key is used
as a compatibility fallback. A session/OAuth token is never forwarded to a
different custom endpoint. If no safe key is available, vector search falls
back to FTS-only.

If `[memory.embedding].model` is omitted, memory remains FTS-only and does not
send an embedding request. The current runtime supports `provider = "api"`;
`local` and `auto` are retained for config compatibility but warn and use
FTS-only because no local model runtime is bundled.

In Open mode, embedding requests are allowed only for an explicit non-xAI
endpoint. xAI and cli-chat-proxy embedding endpoints require
`[fork].mode = "xai_compat"`.

## Other optional capabilities

### Web search

Web search is independent from the chat model. Configure a provider profile;
the profile owns its endpoint, credentials, request mapping, and response
mapping. This supports APIs such as Brave and Tavily directly, as well as
OpenAI-compatible search gateways:

```toml
[capabilities.search]
protocol = "generic_http"
base_url = "https://api.search.example"
env_key = "SEARCH_API_KEY"

[capabilities.search.auth]
location = "header"
name = "Authorization"
prefix = "Bearer "

[capabilities.search.operations.default]
method = "POST"
path = "/search"
[capabilities.search.operations.default.request]
body = "json"
[capabilities.search.operations.default.request.fields]
query = "query"
count = "max_results"
[capabilities.search.operations.default.response]
items = "/results"
title = "/title"
url = "/url"
content = "/content"
```

For a provider that uses query authentication, set `auth.location = "query"`;
for a Responses-compatible model gateway, map the operation to
`/responses` and supply the gateway's response pointers. Open mode accepts only
an explicit non-xAI endpoint.

### Image generation

Use a provider profile for a generic image endpoint. The operation mapping
supports JSON, multipart, binary, base64, and URL responses; OpenAI-compatible
`/images/generations` and multipart `/images/edits` are common examples.
Set `response.bytes` (or `response.value`) for base64 data and `response.url`
for a provider-returned URL. URL downloads deliberately do not forward the
image API credential to the object/download origin.

```toml
[capabilities.image]
protocol = "openai_images"
base_url = "https://api.openai.com/v1"
model = "gpt-image-1"
env_key = "OPENAI_API_KEY"

[capabilities.image.operations.generate]
method = "POST"
path = "/images/generations"
[capabilities.image.operations.generate.request.fields]
model = "model"
prompt = "prompt"
size = "size"
[capabilities.image.operations.generate.response]
bytes = "/data/0/b64_json"
```

The legacy `[image_gen]` block remains compatible for simple JSON image
providers. Open mode never falls back to an implicit xAI image endpoint.

### Video generation

Video profiles support the common asynchronous shape used by Runway and
Replicate: create a job, poll a URL or `{job_id}` operation until a configured
success/failure status, then download the output URL.

```toml
[capabilities.video]
protocol = "generic_async"
base_url = "https://video.example/v1"
model = "video-model"
env_key = "VIDEO_API_KEY"

[capabilities.video.operations.create]
method = "POST"
path = "/image_to_video"
[capabilities.video.operations.create.request.fields]
model = "model"
prompt = "promptText"
image = "promptImage"
[capabilities.video.operations.create.response]
job_id = "/id"
poll_url = "/status_url"

[capabilities.video.operations.poll]
method = "GET"
path = "/tasks/{job_id}"
[capabilities.video.operations.poll.response]
status = "/status"
url = "/output/0"
```

The existing xAI video path remains available only in `xai_compat`.

### Voice / streaming STT

`[capabilities.voice]` maps common WebSocket STT providers. The built-in
drivers understand the xAI, Deepgram, and ElevenLabs event families while
sharing `base_url`, operation path, API key/env key, auth placement, query
parameters, and extra headers. For Deepgram, use `protocol = "deepgram"`,
`base_url = "https://api.deepgram.com"`, and `env_key = "DEEPGRAM_API_KEY"`;
the default path is `/v1/listen` and the default auth prefix is `Token `.
Open mode rejects implicit xAI voice endpoints.

### Sandbox and deploy

The upstream `x.ai/cloud/*` sandbox extensions and the deploy-app tool are
not provider-neutral APIs. They are blocked in Open mode and are retained only
for `xai_compat` merge compatibility. A `[capabilities.sandbox]` or
`[capabilities.deploy]` profile is parsed as configuration data, but no generic
adapter is enabled until a concrete provider contract and tool implementation
are added; this is intentional rather than silently sending a key to xAI.

## Provider profile field contract

All `[capabilities.<name>]` sections share these fields:

| Field | Meaning |
|---|---|
| `protocol` | Driver hint (`openai_compatible`, `cohere_v2`, `generic_http`, `deepgram`, etc.). |
| `base_url` | Explicit HTTPS/HTTP(S) endpoint root; no embedded credentials. |
| `model` | Provider model/deployment slug, when required. |
| `api_key`, `env_key` | Static key or ordered environment-key fallback. |
| `[auth]` | `location = "header"`/`"query"`, `name`, and `prefix`. |
| `extra_headers`, `env_headers`, `query_params` | Static metadata, secret header indirection, and fixed query parameters. |
| `[operations.*]` | Method/path plus normalized request and response mappings. |
| `[operations.*.request]` | `body = "json"`, `"query"`, `"multipart"`, or `"binary"`; field/file mappings and defaults. |
| `[operations.*.response]` | RFC 6901 JSON pointers for items, values, bytes, job IDs, statuses, and output URLs. |
| `[operations.*.async_config]` | Poll interval, timeout, success statuses, and failure statuses. |
| `[stream]`, `[artifact]` | Streaming framing and artifact transfer hints for drivers that support them. |

The contract is intentionally a transport/mapping layer rather than a vendor
registry. It covers the common shapes documented by OpenAI, Cohere, Tavily,
Brave, Deepgram, Runway, and Replicate; provider-specific streaming events or
job semantics still require a small protocol driver.

### Web fetch

Web fetch is a separate direct-fetch capability and does not require a model
provider key. It can be disabled with `GROK_DISABLE_WEB_FETCH=1`; an optional
proxy can be supplied with `GROK_WEB_FETCH_PROXY`.

## Shared model-provider blocks

`[model_providers.<id>]` is only a reusable configuration block. It does not
select or bind a vendor at runtime:

```toml
[model_providers.gateway]
base_url = "https://gateway.example/v1"
api_backend = "chat_completions"
env_key = "GATEWAY_API_KEY"

[model.gateway-model]
model = "provider/model-name"
model_provider = "gateway"
context_window = 128000
```

The model's own fields override provider-block defaults. `extra_headers`,
`query_params`, and `env_http_headers` are inherited when the model does not
provide them.

## Headers and custom gateways

Use static headers for non-secret routing metadata:

```toml
[model.gateway-model]
model = "provider/model-name"
base_url = "https://gateway.example/v1"
api_backend = "chat_completions"
env_key = "GATEWAY_API_KEY"
extra_headers = { "X-Tenant" = "team-a" }
```

Use `env_http_headers` for secret headers:

```toml
env_http_headers = { "X-Tenant-Token" = "GATEWAY_TENANT_TOKEN" }
```

Do not put secrets in `query_params` or commit literal keys.

## Model catalog and filters

The catalog is resolved from these layers, from lowest to highest priority:

1. compiled-in `default_models.json` entries;
2. an explicitly configured/fetched model list;
3. `[model.<id>]` overrides;
4. `[models]` global defaults.

Set a custom model-list endpoint with `GROK_MODELS_BASE_URL` or
`[endpoints].models_base_url`. In custom endpoint mode, built-in defaults are
skipped. The list URL is selected from `GROK_MODELS_LIST_URL`, then
`{models_base_url}/models`, then the configured models endpoint.

```toml
[models]
default = "gateway-model"
allowed_models = ["gateway-*"]
hidden_models = ["legacy-model"]
disabled_models = ["broken-model"]
```

`allowed_models` is an allowlist: excluded models cannot be selected or used
as the default. `hidden_models` only affects the picker. `disabled_models`
removes entries from the effective catalog.

## Open mode and auxiliary services

Open mode blocks implicit first-party cloud paths, including remote settings,
managed configuration, telemetry, feedback, trace upload, cloud product
surfaces, and other upstream-only integrations.

The core model request remains available through the explicitly configured
model endpoint. Workspace image/video/search adapters are disabled by default
even when a workspace token exists. This prevents a generic key from being
sent to a provider-specific endpoint.

### Explicit xAI compatibility mode

Users who intentionally need upstream xAI auxiliary behavior may opt in:

```toml
[fork]
mode = "xai_compat"
```

or for a process-level override:

```sh
GROK_FORK_MODE=xai_compat grok
```

This is an explicit compatibility choice. It is not required for normal BYOK
inference and should not be enabled when the goal is a provider-neutral
deployment.

## Optional endpoint overrides

The following upstream-compatible settings remain available for deployments
that intentionally use them:

| Setting | Purpose |
|---|---|
| `GROK_TRUSTED_API_HOSTS` | Additional exact hostnames/subdomains allowed to receive session credentials |
| `GROK_MODELS_BASE_URL` / `[endpoints].models_base_url` | Custom model catalog endpoint |
| `GROK_MODELS_LIST_URL` | Exact model-list URL |
| `GROK_XAI_API_BASE_URL` / `[endpoints].xai_api_base_url` | Explicit public API endpoint override |
| `GROK_CHANGELOG_BASE_URL` | Changelog mirror |
| `GROK_CLI_BASE_URL` | CLI/update mirror |
| `GROK_UPDATE_REPO` | Release repository override |
| `GROK_OAUTH2_ISSUER` and `GROK_OAUTH2_CLIENT_ID` | Explicit self-hosted OIDC configuration |

These settings do not change the model protocol. Configure `base_url` and
`api_backend` on the model itself.

## Validation checklist

Run:

```sh
grok inspect --json
grok models
```

Verify that:

- the selected default exactly matches a catalog ID;
- the model `base_url` is the intended endpoint;
- `api_backend` matches the endpoint's protocol;
- the expected `env_key` is set and non-empty;
- Open mode is active unless `xai_compat` was intentionally selected;
- no external key is being sent to an implicit first-party endpoint.

The implementation is primarily in:

- `crates/codegen/xai-grok-shell/src/agent/service_policy.rs`
- `crates/codegen/xai-grok-shell/src/agent/config.rs`
- `crates/codegen/xai-grok-shell/src/agent/model_providers.rs`
- `crates/codegen/xai-grok-shell/src/config_model_override_parse.rs`
- `crates/codegen/xai-grok-sampler/src/config.rs`
- `crates/codegen/xai-grok-workspace/src/session/tool_config.rs`
