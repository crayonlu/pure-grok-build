# Registered feature flags

Every registered feature key and where it is pinned. The registry
(`xai-grok-config-types::registry::FEATURES`) is the source of truth; this
table is a hand-maintained mirror kept honest by
`tests/registered_features_are_documented.rs`.

| Key | Env var |
| --- | --- |
| `session_search` | `GROK_SESSION_SEARCH` |
| `lsp_tools` | `GROK_LSP_TOOLS` |
| `web_fetch` | `GROK_WEB_FETCH` |
| `session_recap` | `GROK_SESSION_RECAP` |
| `ask_user_question` | `GROK_ASK_USER_QUESTION` |
| `voice_mode` | `GROK_VOICE_MODE` |
| `write_file` | `GROK_WRITE_FILE` |
| `feedback` | `GROK_FEEDBACK_ENABLED` |
| `feedback_trace_card` | `GROK_FEEDBACK_TRACE_CARD` |
| `turn_summary` | `GROK_TURN_SUMMARY` |
| `cancel_rewind` | `GROK_CANCEL_REWIND` |
| `compaction_verbatim_input` | `GROK_COMPACTION_VERBATIM_INPUT` |
| `two_pass_compaction` | `GROK_TWO_PASS_COMPACTION` |
| `backend_tools` | `GROK_BACKEND_SEARCH` |
| `auto_wake` | `GROK_AUTO_WAKE` |
| `subagent_worktree_snapshot` | `GROK_SUBAGENT_WORKTREE_SNAPSHOT` |
| `active_agent_messages` | `GROK_ACTIVE_AGENT_MESSAGES` |
