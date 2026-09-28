# Environment variables reference

Environment variables that toggle a registered feature flag. The registry
(`xai-grok-config-types::registry::FEATURES`) is the source of truth; this
table is a hand-maintained mirror kept honest by
`tests/registered_features_are_documented.rs`.

| Variable | Feature |
| --- | --- |
| `GROK_SESSION_SEARCH` | `session_search` |
| `GROK_LSP_TOOLS` | `lsp_tools` |
| `GROK_WEB_FETCH` | `web_fetch` |
| `GROK_SESSION_RECAP` | `session_recap` |
| `GROK_ASK_USER_QUESTION` | `ask_user_question` |
| `GROK_VOICE_MODE` | `voice_mode` |
| `GROK_WRITE_FILE` | `write_file` |
| `GROK_FEEDBACK_ENABLED` | `feedback` |
| `GROK_FEEDBACK_TRACE_CARD` | `feedback_trace_card` |
| `GROK_TURN_SUMMARY` | `turn_summary` |
| `GROK_CANCEL_REWIND` | `cancel_rewind` |
| `GROK_COMPACTION_VERBATIM_INPUT` | `compaction_verbatim_input` |
| `GROK_TWO_PASS_COMPACTION` | `two_pass_compaction` |
| `GROK_BACKEND_SEARCH` | `backend_tools` |
| `GROK_AUTO_WAKE` | `auto_wake` |
| `GROK_SUBAGENT_WORKTREE_SNAPSHOT` | `subagent_worktree_snapshot` |
| `GROK_ACTIVE_AGENT_MESSAGES` | `active_agent_messages` |
