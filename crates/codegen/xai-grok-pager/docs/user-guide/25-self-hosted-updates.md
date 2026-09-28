# Self-Hosted Update Source

By default Grok checks for updates and downloads binaries from the upstream
`x.ai/cli` CDN (with a direct-GCS fallback). A fork or enterprise deployment
that publishes its own builds can redirect the in-app auto-updater and the
install scripts to a self-hosted source — no source patching beyond the two
environment variables documented here.

## Overview

Two environment variables control where Grok looks for updates. Both are
optional and unset-by-default, so existing installs keep using the upstream
endpoints unchanged.

| Variable | Format | What it overrides |
|----------|--------|-------------------|
| `GROK_CLI_BASE_URL` | A base URL, e.g. `https://releases.example.com/cli` | The channel-pointer fetch (`<base>/stable`) and binary download (`<base>/grok-<version>-<platform>`). When set, the upstream `x.ai/cli` + GCS fallbacks are skipped entirely. |
| `GROK_UPDATE_REPO` | `owner/repo`, e.g. `crayonlu/pure-grok-build` | The GitHub repository used by the `gh-release` installer path for version checks (`gh release list`) and binary downloads (`gh release download`). |

Pick one mechanism. `GROK_UPDATE_REPO` is the simplest if your builds already
ship as GitHub Releases — the updater uses the `gh` CLI against your repo
directly, so there is nothing to mirror. `GROK_CLI_BASE_URL` is for when you
host artifacts on your own domain/CDN.

## How the version check works

The updater is semver-based. It fetches a latest version string and compares
it to the running binary's version; it only downloads when the fetched version
is semver-greater. This means your published version must be:

1. A valid semver string (`YYYY.M.D`, `0.2.46`, etc.).
2. Greater than the version the user currently runs.

The nightly sync workflow in this fork tags releases with a date-derived
semver `YYYY.M.D` (e.g. `2026.7.28`). This is always greater than the upstream
`0.x` stable line and increases monotonically each day, so daily updates flow
through automatically. Binary assets are named `grok-<version>-<platform>`
(e.g. `grok-2026.7.28-macos-aarch64`), which is the exact object name the
updater downloads.

## Option A — `GROK_UPDATE_REPO` (GitHub Releases)

Set the variable to your fork's `owner/repo`. The updater runs `gh release
list --repo <repo>` to find the latest tag (strips a leading `v`), then
`gh release download` to fetch the matching `grok-<version>-<platform>` asset.

Requirements:

- The user must be authenticated to `gh` (or have `GH_TOKEN` set) with read
  access to the repo.
- Each release must be tagged `v<semver>` and contain a raw binary asset named
  `grok-<version>-<platform>` for each supported platform
  (`macos-aarch64`, `macos-x86_64`, `linux-aarch64`, `linux-x86_64`,
  `windows-x86_64` — Windows assets carry a `.exe` suffix).

```sh
export GROK_UPDATE_REPO=crayonlu/pure-grok-build
grok update
```

## Option B — `GROK_CLI_BASE_URL` (your own host)

Mirror the release artifacts to your host with this layout:

```
<base>/stable                                  # plain-text semver, e.g. 2026.7.28
<base>/grok-<version>-macos-aarch64
<base>/grok-<version>-macos-x86_64
<base>/grok-<version>-linux-aarch64
<base>/grok-<version>-linux-x86_64
<base>/grok-<version>-windows-x86_64.exe
```

The nightly workflow uploads a `stable` channel-pointer file (containing the
version string) to each GitHub Release — download it and mirror it to
`<base>/stable`. Then point Grok at the base URL:

```sh
export GROK_CLI_BASE_URL=https://releases.example.com/cli
grok update
```

The channel pointer file (`stable`) is fetched with a 15 s timeout and up to
three retries per base; the binary download follows the same single-host path
with no upstream fallback.

## Install scripts

All four install scripts (`install.sh`, `install.ps1`, and their `-enterprise`
variants) honor `GROK_CLI_BASE_URL`. When set, the script uses that host alone
for both the channel-pointer fetch and the binary download, skipping the
default `x.ai/cli` probe and GCS fallback.

```sh
# Install from a self-hosted source
GROK_CLI_BASE_URL=https://releases.example.com/cli \
  bash <(curl -fsSL https://releases.example.com/cli/install.sh)
```

The installer type is recorded in `~/.grok/config.toml` as
`installer = "internal"`; the in-app updater then uses the same
`GROK_CLI_BASE_URL` (read from the environment at update-check time) to keep
that install current.

## Notes and limitations

- Both variables are read from the environment at update-check time, so a
  change takes effect on the next check (within the auto-update TTL) without
  restarting the process.
- Setting `GROK_CLI_BASE_URL` to an empty or whitespace-only value is treated
  as unset and falls back to the upstream defaults.
- The variables are independent. Setting both is not recommended; prefer
  `GROK_UPDATE_REPO` when your artifacts are on GitHub, and
  `GROK_CLI_BASE_URL` when they are on your own host.
- Version comparison is strict semver. Non-semver tags (e.g. `nightly-2026-07-28`)
  are rejected by the channel-pointer parser, which is why this fork's nightly
  releases use `YYYY.M.D` semver tags instead.
