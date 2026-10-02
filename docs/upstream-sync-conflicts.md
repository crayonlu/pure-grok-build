# Upstream sync conflicts: why they happen, and what the overlay does not cover

Analysis of the 2026-10-02 sync (`xai-org/grok-build@559751fd`, PR #43), reproducible
from committed refs.

## Summary

- 13 files conflicted. Every one is an **upstream-owned file the fork edited in
  place**; no fork-only crate (`xai-grok-overlay-api`, `pure-grok-overlay`,
  `xai-grok-provider`) appears in the set.
- The overlay isolates **policy values**, not enforcement. It owns 1,224 lines
  that never conflict, but overlay enforcement is spread over **143 call sites in
  31 upstream files**, and overlay code is only **259 of the 6,633 lines (3.9%)**
  the fork inserts into upstream-owned files. The remaining 96% is fork feature
  work done in place: memory embeddings, image/video/web-search providers,
  the updater, installer scripts, pager changes.
- So the stated goal of small, reviewable fork diffs holds for the policy layer
  and does not hold for the fork as a whole. The conflicts come from in-place
  feature edits, not from the overlay's design failing at what it actually does.
- The same files recur every sync: `xai-grok-update/src/auto_update.rs`,
  `xai-grok-update/src/version.rs`, and
  `xai-grok-shell/src/agent/mvp_agent/agent_ops.rs` conflicted in each of the
  last three syncs.

## Reproducing the conflict surface

From a clean checkout (no working-tree state needed):

```sh
git merge-base main upstream/main          # f0e3be11 (last synced upstream commit)
git merge-tree --write-tree --messages main upstream/main | grep '^CONFLICT'
```

13 conflicts: `Cargo.lock`, `Cargo.toml`,
`crates/codegen/xai-grok-config-types/{Cargo.toml,src/lib.rs}`,
`crates/codegen/xai-grok-shell/benches/session_list.rs`,
`crates/codegen/xai-grok-shell/src/agent/{app.rs,config_tests.rs,init.rs,remote_config/manager/mod.rs,server.rs}`,
`crates/codegen/xai-grok-shell/src/agent/mvp_agent/agent_ops.rs`,
`crates/codegen/xai-grok-update/src/{auto_update.rs,version.rs}`.

All 13 exist in `upstream/main` and all 13 carry fork edits against the merge
base:

| Conflicted file | Fork vs merge base | Upstream vs merge base | Overlay refs in file |
| --- | --- | --- | --- |
| `Cargo.lock` | +55/-2 | +240/-57 | – |
| `Cargo.toml` | +6 | +14/-2 | – |
| `xai-grok-config-types/Cargo.toml` | +1 | +1/-4 | – |
| `xai-grok-config-types/src/lib.rs` | +4 | +13/-1699 | – |
| `xai-grok-shell/benches/session_list.rs` | +5/-3 | +21/-48 | 0 |
| `xai-grok-shell/src/agent/app.rs` | +86/-24 | +7/-3 | 12 |
| `xai-grok-shell/src/agent/config_tests.rs` | +32/-8 | +352/-548 | 6 |
| `xai-grok-shell/src/agent/init.rs` | +73/-5 | +49/-24 | 12 |
| `xai-grok-shell/src/agent/mvp_agent/agent_ops.rs` | +297/-7 | +80/-37 | 51 |
| `xai-grok-shell/src/agent/remote_config/manager/mod.rs` | +52/-4 | +102/-21 | 6 |
| `xai-grok-shell/src/agent/server.rs` | +8/-3 | +1/-1 | 2 |
| `xai-grok-update/src/auto_update.rs` | +617/-330 | +55/-26 | 2 |
| `xai-grok-update/src/version.rs` | +260/-77 | +15/-8 | 3 |

A conflict needs both sides to touch the same region, which is why the list is
not simply "the files the fork edits most": `xai-grok-memory/src/embedding.rs`
(+639 lines) and `xai-grok-tools/.../image_gen/mod.rs` (+1,012 lines) did not
conflict this time because upstream left those regions alone. The 13 above are
where the fork's in-place edits and upstream's edits overlap.

## Why the fork's divergence is in-place, not overlay-shaped

Across the 137 files the fork changes against the merge base:

| | files | lines |
| --- | --- | --- |
| Fork-only files (added) | 26 | – |
| Fork-only overlay crates (`xai-grok-overlay-api`, `pure-grok-overlay`) | 8 | 1,224 |
| **In-place edits to files that exist upstream** | **110** | **6,633 inserted** |
| of which overlay-enforcement related | 44 | 259 (3.9%) |

The overlay crates are clean by construction: upstream does not have those files,
so they cannot conflict. The conflict surface is entirely the 110 upstream files
the fork edits, and only a small slice of that is the overlay.

## What the overlay actually isolates

`xai-grok-overlay-api` is a policy value API:

- `OverlayMode` (`upstream` / `open` / `xai_compat`), `AuthPolicy`, and a
  `ServicePolicy` (`Disabled` / `ExplicitOnly` / `Enabled`) per `ServiceKind` —
  ten services: `Auth`, `RemoteSettings`, `ManagedConfig`, `Telemetry`,
  `Feedback`, `TraceUpload`, `Relay`, `Billing`, `Subscription`, `Updates`.
- `EntitlementPolicy`, `CapabilitySet`/`CapabilityAvailability` (per-capability
  provider references), and `UpdateSourceRef`/`UpdateChannel`.
- `OverlayRuntime` accessors: `policy()`, `auth_policy()`, `allows_implicit(kind)`,
  `allows_explicit(kind)`, `entitlement()`, `capability()`, `update_source()`.

`pure-grok-overlay` resolves config (mode, auth, service policies, capability
profiles, update source) into that snapshot and adds `grok-overlay-inspect`.

That is a genuine isolation of **what the policy is**: the fork's distribution
choices live in fork-owned files and upstream has no opinion about them.

## Where enforcement actually lives

Resolution is centralised; enforcement is not. 143 overlay API references sit
outside the two overlay crates, in 31 upstream files:

| Upstream file | overlay refs |
| --- | --- |
| `xai-grok-shell/src/agent/mvp_agent/agent_ops.rs` | 51 |
| `xai-grok-shell/src/agent/init.rs` | 12 |
| `xai-grok-shell/src/agent/app.rs` | 12 |
| `xai-grok-shell/src/agent/remote_config/manager/mod.rs` | 6 |
| `xai-grok-shell/src/agent/config_tests.rs` | 6 |
| `xai-grok-pager/src/acp/mod.rs` | 6 |
| `xai-grok-shell/src/session/acp_session_impl/spawn.rs` | 5 |
| `xai-grok-shell/src/agent/config.rs` | 3 |
| `xai-grok-pager/src/headless.rs`, `xai-grok-update/src/version.rs` | 3 each |
| `xai-grok-update/src/auto_update.rs`, `xai-grok-shell/src/agent/server.rs`, and 20 more | 1–2 each |

These are gates woven into upstream functions, e.g.
`if agent_config.overlay_runtime.policy().allows_session_auth() { auth_manager.start_proactive_refresh(..) }`
in `app.rs`, the `allows_implicit(ServiceKind::ManagedConfig)` guard around
`start_refresh_supervisor` in `init.rs`, and the `TraceUpload`/data-collection
gates in `agent_ops.rs`. `xai-grok-overlay-api/src/lib.rs` states the intent —
"keeps distribution-specific behavior out of upstream application control flow" —
and these call sites are exactly distribution-specific behavior inside upstream
control flow.

**Verdict.** The overlay met its goal for policy values and provider capability
configuration: those are fork-owned, reviewable, and conflict-free. It did not
meet the goal for the fork's overall diff size, because the fork's feature work
(and the enforcement of the policy it resolves) is written into upstream files.
That is the driver of the conflict count, not a defect in the overlay API.

## The pattern recurs

Replaying the previous resolution merges with the same `merge-tree` command:

| Sync | Date | Conflicts |
| --- | --- | --- |
| `28439e8a` | 2026-08-26 | 0 |
| `d5a0335a` | 2026-08-29 | 136 |
| `c4ea71cf` | 2026-09-19 | 25 |
| `036a5d83` | 2026-09-28 | 14 |
| `559751fd` | 2026-10-02 (PR #43) | 13 |

The count has fallen as the fork's in-place churn settled, but the same files
keep reappearing: `auto_update.rs`, `version.rs`, and `agent_ops.rs` conflicted
in all three of the most recent syncs; `Cargo.lock`, `config_tests.rs`, and
`init.rs` in two of three.

## Remediation

### Applied

1. **Zero fork edits in `xai-grok-config-types/src/lib.rs`.** The fork's four
   added lines (`mod pool`/`pub use pool::*`, `mod provider`/`pub use provider::*`)
   collided with upstream's whole-file rewrite (−1,699 lines) for no benefit:
   `pool.rs`'s `PoolConfig` had no consumers, and `provider.rs` was a pure
   re-export shim whose types were used in three files. `pool.rs` and `provider.rs`
   are deleted and those sites now name `xai_grok_provider` directly (as other shell and tools
   code already did). `lib.rs` is byte-identical to `upstream/main`.
   *Proof:* a counterfactual commit — fork `main` with only that four-line edit
   reverted — produces 12 conflicts instead of 13; `config-types/src/lib.rs` is
   absent from the list and has no conflict stage entries.
2. **`xai-grok-shell/benches/session_list.rs` is now byte-identical to
   `upstream/main`** as a side effect of resolving PR #43: the fork's struct-literal
   fixture was replaced by upstream's `serde_json::from_value` fixture, so the
   fork's edit there is gone and the file leaves the conflict set.
3. **CI job budget** (process): the check job was cancelled at its 30-minute
   timeout inside the last, informational `clippy` step although `cargo fmt`, the
   `-D warnings` check, and both test suites had passed; the previous sync
   finished the same job in 29m26s, so the added ~130k lines left no headroom.
   `timeout-minutes` is now 45. A cheaper long-term fix is a cargo cache
   (`Swatinem/rust-cache`) or splitting `clippy` into its own job so the gate
   does not share its budget with an informational step.

### Deferred, with reasons

4. **Consolidating the overlay gates in `agent_ops.rs` (51 refs), `app.rs`,
   `init.rs`, `remote_config/manager/mod.rs`.** Moving the gate bodies into a
   fork-owned helper module shrinks the inserted lines per site but leaves a call
   inside each upstream function, so the conflict returns whenever upstream edits
   that function. A real fix is a seam upstream does not edit — e.g. one
   composition-root wrapper per service (the `ensure_managed_policy_present_for_overlay`
   shape that already exists in `xai-grok-cloud-config`) instead of inline
   conditions. That is a design change across ~20 call sites, not a mechanical
   move, so it is out of scope here.
5. **Relocating `effective_release_repo` / `effective_cli_base_urls` out of
   `xai-grok-update/src/version.rs`.** The insertion point is what conflicted, and
   the functions only read the overlay runtime, so a move into `pure-grok-overlay`
   would remove that hunk. `effective_cli_base_urls` also calls upstream's
   private `is_loopback_base` guard, which would have to be duplicated in the
   overlay crate and then drift, so this was left alone rather than half-moved.
6. **`Cargo.toml` / `Cargo.lock` / `config-types/Cargo.toml` conflicts.** These
   are inherent to adding fork crates and dependencies to sorted lists upstream
   also edits. Not removable without removing the fork's crates.

### What would shrink the surface most

The largest in-place fork edits that upstream also touches are
`auto_update.rs` (+617) and `version.rs` (+260) — a fork rewrite of updater
internals, and `agent_ops.rs` (+297) — fork gates and trace-upload policy inside
upstream methods. Those three files alone account for three of the last three
syncs' conflicts. Extracting each into a fork-owned module behind a small seam
(same shape as the overlay's own `ensure_managed_policy_present_for_overlay`)
would remove them from future conflict sets; doing it for the overlay call sites
listed in section "Where enforcement actually lives" would additionally stop
overlay policy from conflicting.

## Reproducing this analysis

```sh
BASE=$(git merge-base main upstream/main)
git merge-tree --write-tree --messages main upstream/main | grep '^CONFLICT'
git diff --name-status "$BASE" main                       # fork-only vs in-place
git diff --shortstat "$BASE" main -- <file>               # per-file fork edits
grep -rn --include=*.rs -E 'xai_grok_overlay_api::|allows_implicit|allows_explicit|allows_session_auth' crates/ \
  | grep -vE 'crates/codegen/(xai-grok-overlay-api|pure-grok-overlay)/'   # enforcement census
```
