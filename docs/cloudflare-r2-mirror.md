# Cloudflare R2 Mirror Configuration

The nightly CI in this repository mirrors build artifacts to Cloudflare R2 object storage, served via the custom domain `https://grok.cyncyn.xyz/cli` with global CDN acceleration. Users only need to set one environment variable to use the self-hosted update source.

## Architecture Overview

```
GitHub Actions (nightly release)
  │
  ├── Build binaries + stable pointer -> GitHub Releases (existing)
  │
  └── Upload to Cloudflare R2 bucket "grok-build"
        ├── cli/stable                          (Cache-Control: no-cache)
        ├── cli/grok-<version>-macos-x86_64     (Cache-Control: immutable)
        ├── cli/grok-<version>-macos-aarch64    (Cache-Control: immutable)
        ├── cli/grok-<version>-linux-x86_64     (Cache-Control: immutable)
        └── cli/grok-<version>-linux-aarch64    (Cache-Control: immutable)

R2 bucket <- custom domain grok.cyncyn.xyz -> Cloudflare CDN (global edge cache + automatic HTTPS)
```

**No server required.** R2 is serverless object storage, delivered through Cloudflare's global CDN with zero egress fees.

## Usage

Add the following to your shell configuration (`~/.zshrc`, `~/.bashrc`, etc.):

```bash
export GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli
```

Once set, the grok auto-updater will:
1. Fetch the latest version number from `https://grok.cyncyn.xyz/cli/stable`
2. Download the platform-specific binary from `https://grok.cyncyn.xyz/cli/grok-<version>-<platform>`

No `gh` CLI or npm required — downloads happen over plain HTTPS.

### Caching Strategy

- **Channel pointer** (`stable`): `Cache-Control: no-cache` — every request revalidates with the origin (R2), ensuring new versions are picked up immediately.
- **Binaries**: `Cache-Control: public, max-age=31536000, immutable` — versioned URLs never change once published, so CDN edge nodes cache for one year, enabling fast global downloads.

## GitHub Secrets

The CI requires the following GitHub repository secret to upload to R2:

### Required

| Secret name | Description |
|---|---|
| `CLOUDFLARE_API_TOKEN` | Cloudflare API Token with **R2 Storage Edit** permission |

**Setup steps:**
1. Go to https://dash.cloudflare.com/profile/api-tokens
2. Click **Create Token**
3. Select **Create Custom Token**
4. Configure permissions:
   - Account -> **Workers R2 Storage** -> **Edit**
5. Copy the token after creation
6. In the GitHub repo: Settings -> Secrets and variables -> Actions -> New repository secret
7. Name: `CLOUDFLARE_API_TOKEN`, Value: paste the token

### Optional

| Variable name | Description | Default |
|---|---|---|
| `CLOUDFLARE_ACCOUNT_ID` | Cloudflare account ID (Repository Variable, not a Secret) | `6eb9ddbde814b5def318c50efc93a54f` |

If `CLOUDFLARE_API_TOKEN` is not set, the CI skips the R2 upload and emits a warning. GitHub Releases publishing is unaffected.

## Cloudflare Resource Inventory

| Resource | Value |
|---|---|
| Account | crayon (`6eb9ddbde814b5def318c50efc93a54f`) |
| R2 bucket | `grok-build` (APAC region) |
| Custom domain | `grok.cyncyn.xyz` |
| DNS record | CNAME `grok.cyncyn.xyz` -> `public.r2.dev` (Cloudflare proxied) |
| Zone | `cyncyn.xyz` (ID: `772d533eeea0aa5d417695cfb370f721`) |
| Min TLS | 1.2 |

## R2 Upload Logic in CI

Located in the `release` job of `.github/workflows/upstream-sync-release.yml`, executed after the GitHub Release is published:

1. **Check configuration**: Verify that `CLOUDFLARE_API_TOKEN` exists
2. **Install wrangler**: `npm install -g wrangler@3`
3. **Upload binaries**: Iterate over 4 platforms, upload each raw binary to `cli/grok-<version>-<platform>` using `wrangler r2 object put` with `--cache-control="public, max-age=31536000, immutable"`
4. **Upload channel pointer**: Upload the `stable` file to `cli/stable` with `--cache-control="no-cache"`

R2 object key structure:
```
grok-build/
└── cli/
    ├── stable                              # plain-text version number
    ├── grok-2026.7.28-macos-x86_64         # raw binary
    ├── grok-2026.7.28-macos-aarch64
    ├── grok-2026.7.28-linux-x86_64
    └── grok-2026.7.28-linux-aarch64
```

## Optional: Cloudflare Cache Rule

R2 custom domains have Origin Cache Control enabled by default, which respects the `Cache-Control` header on each object. The `--cache-control` flags set during upload are sufficient.

For additional control in the Cloudflare Dashboard (e.g., forcing cache bypass for `/cli/stable`), you can manually add Cache Rules:

1. Go to Cloudflare Dashboard -> cyncyn.xyz -> Caching -> Cache Rules
2. Add Rule 1 (bypass cache for channel pointers):
   - **Name**: Bypass cache for channel pointers
   - **Expression**: `Hostname equals "grok.cyncyn.xyz"` AND (`URI Path starts with "/cli/stable"` OR `URI Path starts with "/cli/alpha"`)
   - **Cache eligibility**: Bypass cache
3. Add Rule 2 (long cache for binaries):
   - **Name**: Cache binaries long-term
   - **Expression**: `Hostname equals "grok.cyncyn.xyz"` AND `URI Path starts with "/cli/grok-"`
   - **Cache eligibility**: Eligible for cache
   - **Edge TTL**: Override origin, 1 year
   - **Browser TTL**: Override origin, 1 year

> Note: This step requires a Cloudflare API Token with Cache Rules edit permission. The current MCP token lacks this permission, so it must be done manually in the Dashboard.

## Verification

After configuration, verify with:

```bash
# Check channel pointer
curl -sS https://grok.cyncyn.xyz/cli/stable
# Should return the latest version, e.g. 2026.7.28

# Check response headers and cache status
curl -sS -D - -o /dev/null https://grok.cyncyn.xyz/cli/stable
# cf-cache-status should be DYNAMIC or BYPASS (channel pointer is not cached)

# Check binary download (macOS aarch64 example)
VERSION=$(curl -sS https://grok.cyncyn.xyz/cli/stable)
curl -sS -D - -o /dev/null "https://grok.cyncyn.xyz/cli/grok-${VERSION}-macos-aarch64"
# HTTP 200; cf-cache-status will be MISS on first request, HIT on subsequent
```
