pub mod command;
pub mod http;

use std::time::Duration;

use crate::config::HookSpec;
use crate::event::HookEventEnvelope;
use serde::Deserialize;

use crate::result::{HookDecision, HttpInfo, StopHookOutcome};

/// How a hook's output is interpreted, per the event's [`GateKind`]: `Observe`
/// ignores output, `Tool` parses the allow/deny vocabulary, `Stop` the stop
/// vocabulary.
pub use crate::event::GateKind;

pub struct RunContext<'a> {
    pub session_id: &'a str,
    pub workspace_root: &'a str,
    pub process_scope: Option<xai_grok_tools::util::ProcessScope>,
}

/// Result of running a single hook (any handler type).
#[derive(Debug)]
pub enum HookRunnerResult {
    Decision {
        decision: HookDecision,
        /// Claude Code-compatible `hookSpecificOutput.updatedInput`: a shallow
        /// merge map applied to the tool call's input before execution.
        /// Only meaningful for `PreToolUse` (`GateKind::Tool`) hooks; empty on
        /// other gates.
        updated_input: Option<serde_json::Map<String, serde_json::Value>>,
    },
    Stop(StopHookOutcome),
    Success,
    /// Failed: the caller fails open.
    Failed(String),
}

/// JSON from `PreToolUse` gate hooks:
/// `{"decision": "allow" | "deny", "reason": "…", "hookSpecificOutput": {"updatedInput": {…}}}`.
///
/// `hookSpecificOutput.updatedInput` is the Claude Code protocol for a hook to
/// rewrite the tool call's input (e.g. RTK prefixing a bash command). It is a
/// shallow merge: keys present in the map replace the corresponding keys of the
/// tool's parsed input object.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct GateHookJson {
    pub decision: String,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, rename = "hookSpecificOutput")]
    pub hook_specific_output: Option<GateHookSpecificOutputJson>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct GateHookSpecificOutputJson {
    #[serde(default, rename = "updatedInput")]
    pub updated_input: Option<serde_json::Map<String, serde_json::Value>>,
}

/// Interpret a [`GateHookJson`] as a [`HookDecision`], plus the optional
/// `hookSpecificOutput.updatedInput` rewrite map. An unknown decision value is
/// an error so typos surface instead of failing open.
///
/// `fallback_reason` supplies the deny message when the JSON carries none
/// (command hooks pass the first stderr line — the hook's feedback channel;
/// HTTP hooks have no stderr and pass `None`).
pub(crate) fn gate_json_to_decision(
    json: GateHookJson,
    hook_name: &str,
    fallback_reason: Option<&str>,
) -> Result<
    (
        HookDecision,
        Option<serde_json::Map<String, serde_json::Value>>,
    ),
    String,
> {
    let updated_input = json.hook_specific_output.and_then(|out| out.updated_input);
    let decision = match json.decision.as_str() {
        "deny" => HookDecision::Deny {
            reason: json
                .reason
                .filter(|r| !r.trim().is_empty())
                .or_else(|| fallback_reason.map(str::to_string))
                .unwrap_or_else(|| format!("denied by hook '{hook_name}'")),
            hook_name: hook_name.to_string(),
        },
        "allow" => HookDecision::Allow,
        other => {
            return Err(format!(
                "unknown decision value '{other}' from hook '{hook_name}'"
            ));
        }
    };
    Ok((decision, updated_input))
}

/// JSON from `Stop`/`SubagentStop` gate hooks. All fields optional; one output
/// can combine several signals.
#[derive(Debug, Default, Deserialize)]
pub(crate) struct StopHookJson {
    #[serde(default)]
    pub decision: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default, rename = "continue")]
    pub continue_: Option<bool>,
    #[serde(default, rename = "stopReason")]
    pub stop_reason: Option<String>,
    #[serde(default, rename = "hookSpecificOutput")]
    pub hook_specific_output: Option<StopHookSpecificOutputJson>,
}

#[derive(Debug, Default, Deserialize)]
pub(crate) struct StopHookSpecificOutputJson {
    #[serde(default, rename = "additionalContext")]
    pub additional_context: Option<String>,
}

/// Interpret a [`StopHookJson`] as a [`StopHookOutcome`].
///
/// `decision: "block"` requires a reason (a missing one falls back to a generic
/// message). `decision: "approve"` is a no-op; any other value is an error so
/// typos surface.
pub(crate) fn stop_json_to_outcome(
    json: StopHookJson,
    hook_name: &str,
) -> Result<StopHookOutcome, String> {
    let block_reason = match json.decision.as_deref() {
        Some("block") => Some(
            json.reason
                .filter(|reason| !reason.trim().is_empty())
                .unwrap_or_else(|| format!("Blocked by stop hook '{hook_name}'")),
        ),
        Some("approve") | None => None,
        Some(other) => {
            return Err(format!(
                "unknown decision value '{other}' from hook '{hook_name}'"
            ));
        }
    };
    Ok(StopHookOutcome {
        block_reason,
        additional_context: json
            .hook_specific_output
            .and_then(|output| output.additional_context)
            .filter(|context| !context.trim().is_empty()),
        force_stop: (json.continue_ == Some(false)).then_some(crate::result::StopOverride {
            reason: json.stop_reason,
        }),
    })
}

/// Each runner returns the result, wall-clock duration, and optional HTTP
/// metadata for enriched scrollback logging.
pub type HookRunOutput = (HookRunnerResult, Duration, Option<HttpInfo>);

pub async fn run_hook(
    spec: &HookSpec,
    envelope: &HookEventEnvelope,
    ctx: &RunContext<'_>,
    mode: GateKind,
) -> HookRunOutput {
    match spec.handler_type {
        crate::config::HandlerType::Command => {
            let (result, elapsed) = command::run_command_hook(spec, envelope, ctx, mode).await;
            (result, elapsed, None)
        }
        crate::config::HandlerType::Http => http::run_http_hook(spec, envelope, ctx, mode).await,
    }
}
