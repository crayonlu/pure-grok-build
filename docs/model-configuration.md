# Grok Build Model Configuration

This document describes the model configuration implemented by the current
source tree. The runtime file is `~/.grok/config.toml`. Configuration is
parsed by `xai-grok-shell`, resolved into a catalog, and converted into a
`SamplerConfig` for each request.

The fork defaults to provider-neutral Open mode. A model is routed only by
its explicit `base_url`, `api_backend`, and credential fields. No provider is
selected from a hostname or inferred from the key. See
`docs/fork-configuration.md` for fork-wide service policy and compatibility
mode details.

## Resolution order

The catalog is assembled from these layers, from lowest to highest priority:

1. Compiled-in entries in `xai-grok-models/default_models.json`.
2. Models fetched from a configured models endpoint.
3. `[model.<id>]` overrides from user or managed configuration.
4. Global `[models]` values, which fill only fields left unset.

Per-model values win over lower layers. A model table whose ID is not in the
built-in or fetched catalog creates a new entry. New entries use a fallback
context window of `200000` tokens; providers should set `context_window`.

When `GROK_MODELS_BASE_URL` or `[endpoints].models_base_url` enables custom
endpoint mode, built-in defaults are skipped. The model list URL is selected as
`GROK_MODELS_LIST_URL`, then `{models_base_url}/models`, then the proxy's
`/models` endpoint.

## Minimal configuration

The table key is the catalog ID used by `/model`, the picker, and `-m`. The
`model` value is the provider routing slug sent in the request.

```toml
[models]
default = "my-model"

[model.my-model]
model = "provider/model-name"
base_url = "https://gateway.example/v1"
env_key = "PROVIDER_API_KEY"
context_window = 128000
```

All fields in `[model.<id>]` are optional when overriding a built-in model;
unspecified fields are inherited.

For a new provider-neutral model, configure all four routing essentials:
`model`, `base_url`, `api_backend`, and either `env_key` or `api_key`.

## Per-model fields

| Field | Type | Meaning |
|---|---|---|
| `id` | string | Stable catalog ID; normally the table key is enough. |
| `model` | string | Provider model/routing slug. |
| `base_url` | string | Session/inference base URL. |
| `api_base_url` | string | Optional base URL used only for API-key requests. |
| `name`, `description` | string | Picker label and catalog description. |
| `api_key` | string | Literal key; prefer `env_key` for secrets. |
| `env_key` | string/array | Ordered names; first non-blank environment value wins. |
| `api_backend` | enum | `chat_completions`, `responses`, or `messages`. |
| `auth_scheme` | enum | `bearer` (default) or `x_api_key`. |
| `extra_headers` | table | Static inference headers. |
| `query_params` | table | Query parameters appended to inference URLs. |
| `env_http_headers` | table | Header-to-environment-variable mappings. |
| `context_window` | integer | Token window used by auto-compaction. |
| `temperature`, `top_p` | number | Sampling controls. |
| `max_completion_tokens` | integer | Maximum generated tokens. |
| `max_retries` | integer | Per-model retry limit. |
| `inference_idle_timeout_secs` | integer | Idle streaming timeout. |
| `stream_tool_calls` | boolean | Enables streamed tool-call request metadata. |
| `reasoning_effort` | enum | Default reasoning effort. |
| `supports_reasoning_effort` | boolean | Enables effort controls in the UI. |
| `reasoning_efforts` | array | Per-model effort menu. |
| `supports_backend_search` | boolean | Declares server-side search support. |
| `system_prompt_label` | string | Prompt identity, separate from `name`. |
| `agent_type` | string | Agent definition; default `grok-build-plan`. |
| `use_concise` | boolean | Concise prompt/tool-output mode. |
| `hidden` | boolean | Hides the model from the picker. |
| `supported_in_api` | boolean | API-key users cannot select it when false. |
| `auto_compact_threshold_percent` | integer | Per-model threshold, 0–100. |
| `compactions_remaining` | value | Controls compaction-remaining header. |
| `compaction_at_tokens` | value | Controls compaction-at header. |

Unknown or invalid fields normally keep valid fields and generate warnings;
they do not silently create new model properties. Use `grok inspect` to view
the warnings.

## Credential resolution

For each request, credentials are resolved in this order:

1. Non-blank per-model `api_key`.
2. The first non-blank variable named by per-model `env_key`.
3. A configured named `auth_provider` token.
4. The active session token from `grok login`.
5. `XAI_API_KEY`, then legacy `GROK_CODE_XAI_API_KEY`.

A model with its own `api_key` or `env_key` does not inherit a session token.
If a static key and `auth_provider` are both configured, the static key wins.

```toml
[model.openai]
model = "gpt-5"
base_url = "https://api.openai.com/v1"
env_key = "OPENAI_API_KEY"
context_window = 400000
```

`env_key` also accepts an ordered array, useful for forwarded SSH variables:

```toml
env_key = ["PROVIDER_API_KEY", "LC_PROVIDER_API_KEY"]
```

## API backends and headers

`chat_completions` calls `{base_url}/chat/completions`; `responses` calls
`{base_url}/responses`; `messages` calls `{base_url}/messages`.

```toml
[model.openai-responses]
model = "gpt-5"
base_url = "https://api.openai.com/v1"
api_backend = "responses"
env_key = "OPENAI_API_KEY"
context_window = 400000

[model.claude]
model = "claude-sonnet-4-5"
base_url = "https://api.anthropic.com/v1"
api_backend = "messages"
auth_scheme = "x_api_key"
env_key = "ANTHROPIC_API_KEY"
context_window = 200000
```

For `messages`, the sampler automatically sends Anthropic's required
`anthropic-version = "2023-06-01"`. Add the header explicitly in
`extra_headers` only when a provider requires a different API version or beta
contract; explicit configuration takes precedence.

Use `env_http_headers` for secret headers that must not be written into the
config file:

```toml
[model.gateway]
model = "gateway-model"
base_url = "https://gateway.example/v1"
env_key = "GATEWAY_API_KEY"
env_http_headers = { "X-Tenant-Token" = "GATEWAY_TENANT_TOKEN" }
```

## Shared provider blocks

`[model_providers.<id>]` stores reusable connection and credential defaults.
Models opt in with `model_provider`:

The block is only a reusable configuration namespace; it does not bind the
runtime to a particular vendor. Open mode never selects a provider by
inspecting a hostname or key. Choose the endpoint, protocol, and credentials
explicitly per model.

```toml
[model_providers.gateway]
base_url = "https://gateway.example/v1"
env_key = "GATEWAY_API_KEY"
api_backend = "chat_completions"
context_window = 128000

[model.gateway-model]
model = "provider/model-name"
model_provider = "gateway"
max_completion_tokens = 8192
```

Provider defaults are used only when the model does not set its own value.
The `extra_headers`, `query_params`, and `env_http_headers` tables are
inherited as a whole when the model's corresponding table is empty. Model
credentials take precedence over provider credentials.

## Global defaults and filters

Supported global defaults are sampling/retry settings and headers:

```toml
[models]
temperature = 0.7
top_p = 0.95
max_completion_tokens = 8192
max_retries = 8
inference_idle_timeout_secs = 600
stream_tool_calls = false
extra_headers = { "X-Request-Tags" = "team=example" }
```

Catalog visibility can be filtered with case-sensitive glob patterns:

```toml
[models]
allowed_models = ["gateway-*", "grok-4.5"]
hidden_models = ["legacy-model"]
disabled_models = ["broken-model"]
```

`disabled_models` removes entries; `hidden_models` only hides them from the
picker. `allowed_models` also prevents excluded models from being defaulted or
selected.

## Built-in xAI API key

This section describes an explicit first-party compatibility configuration. It
is not required for Open-mode BYOK and is not selected automatically for a
custom model.

```bash
export XAI_API_KEY="xai-..."
grok models
grok -m grok-4.5
```

To override only the key for a built-in model, preserve its compiled endpoint
and metadata:

```toml
[model.grok-4.5]
env_key = "XAI_API_KEY"
```

`GROK_XAI_API_BASE_URL` changes the public xAI API base URL. A deployment
`disable_api_key_auth` policy can replace first-party API-key auth with a
session token; it does not replace credentials for non-xAI BYOK endpoints.

## Validation and source of truth

Run `grok inspect --json` and `grok models` to inspect the effective catalog.
Check that the default exactly matches a catalog key, new models have a
realistic `context_window`, the backend matches the provider path, and every
`env_key` variable is set and non-blank. Do not put secrets in `query_params`
or commit literal keys.

Auxiliary services use the separate `[capabilities.*]` profiles documented in
`docs/fork-configuration.md`; they do not inherit the model endpoint or
credential unless the specific subsystem explicitly documents a compatibility
fallback. Memory embeddings may inherit a static BYOK key on a trusted
endpoint, while image, video, search, and voice profiles remain scoped to
their configured endpoint.

The implementation lives primarily in:

- `crates/codegen/xai-grok-shell/src/agent/config.rs`
- `crates/codegen/xai-grok-shell/src/agent/model_providers.rs`
- `crates/codegen/xai-grok-shell/src/agent/config_model_override_parse.rs`
- `crates/codegen/xai-grok-sampler/src/config.rs`
- `crates/codegen/xai-grok-models/default_models.json`
