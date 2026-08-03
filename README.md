# pure-grok-build

A self-hosted fork of [Grok Build](https://github.com/xai-org/grok-build) (`grok` CLI) with automated nightly releases and a Cloudflare R2 mirror for self-hosted auto-updates.

## What this fork does

- **Nightly upstream sync** — A scheduled GitHub Actions workflow merges `xai-org/grok-build@main` into this fork every day, builds release binaries for 4 platforms, and publishes a GitHub Release with a date-based semver tag (e.g. `2026.7.28`).
- **Self-hosted update source** — Two environment variables (`GROK_CLI_BASE_URL`, `GROK_UPDATE_REPO`) let the in-app auto-updater fetch new versions from this fork instead of the upstream CDN.
- **Cloudflare R2 mirror** — All nightly binaries and the `stable` channel pointer are mirrored to a Cloudflare R2 bucket served via `https://grok.cyncyn.xyz/cli`, providing global CDN-accelerated downloads with zero egress fees.

## Installation

### Option A: Install from the R2 mirror (recommended)

```sh
# 1. Download the latest version pointer
VERSION=$(curl -sS https://grok.cyncyn.xyz/cli/stable)

# 2. Download the binary for your platform
#    macOS Apple Silicon:
curl -fSL -o grok "https://grok.cyncyn.xyz/cli/grok-${VERSION}-macos-aarch64"
#    macOS Intel:
# curl -fSL -o grok "https://grok.cyncyn.xyz/cli/grok-${VERSION}-macos-x86_64"
#    Linux x86_64:
# curl -fSL -o grok "https://grok.cyncyn.xyz/cli/grok-${VERSION}-linux-x86_64"
#    Linux aarch64:
# curl -fSL -o grok "https://grok.cyncyn.xyz/cli/grok-${VERSION}-linux-aarch64"

# 3. Install
chmod +x grok
sudo mv grok /usr/local/bin/grok    # or anywhere on your PATH

# 4. Enable auto-updates from the R2 mirror + changelog display
echo 'export GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli' >> ~/.zshrc
echo 'export GROK_UPDATE_REPO=crayonlu/pure-grok-build' >> ~/.zshrc
source ~/.zshrc

grok --version
```

### Option B: Install from GitHub Releases

```sh
gh release download --repo crayonlu/pure-grok-build --pattern 'grok-*-macos-aarch64' --output grok
chmod +x grok && sudo mv grok /usr/local/bin/grok
grok --version
```

For auto-updates via GitHub Releases:

```sh
echo 'export GROK_UPDATE_REPO=crayonlu/pure-grok-build' >> ~/.zshrc
source ~/.zshrc
```

### Option C: Build from source

```sh
git clone https://github.com/crayonlu/pure-grok-build.git
cd pure-grok-build
cargo build -p xai-grok-pager-bin --release
# Binary: target/release/xai-grok-pager
cp target/release/xai-grok-pager /usr/local/bin/grok
```

## Switching from the official Grok Build

If you already have the official `grok` installed and want to switch to this fork:

### If installed via the official installer (internal)

```sh
# 1. Point the auto-updater at the R2 mirror
echo 'export GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli' >> ~/.zshrc
source ~/.zshrc

# 2. Manually install the fork's binary (one-time)
VERSION=$(curl -sS https://grok.cyncyn.xyz/cli/stable)
curl -fSL -o ~/.grok/downloads/grok-${VERSION}-macos-aarch64 \
  "https://grok.cyncyn.xyz/cli/grok-${VERSION}-macos-aarch64"
chmod +x ~/.grok/downloads/grok-${VERSION}-macos-aarch64
ln -sf ../downloads/grok-${VERSION}-macos-aarch64 ~/.grok/bin/grok
ln -sf ../downloads/grok-${VERSION}-macos-aarch64 ~/.grok/bin/agent

# 3. Verify
grok --version
```

Future updates will be fetched automatically from the R2 mirror — no manual steps needed.

### If installed via npm

```sh
# 1. Uninstall the npm version
npm uninstall -g @xai-official/grok

# 2. Follow Option A above to install from the R2 mirror
```

## How auto-updates work

Once `GROK_CLI_BASE_URL` is set, the grok auto-updater:

1. Fetches the latest version number from `<base>/stable` (plain-text semver)
2. Compares it against the running version using semver
3. If newer, downloads `<base>/grok-<version>-<platform>` and smoke-tests it
4. Atomically swaps the `~/.grok/bin/grok` symlink to the new binary

The date-based versioning (`YYYY.M.D`) ensures nightly builds are always semver-greater than the upstream stable line, so the updater treats each nightly as an upgrade.

## Available platforms

| Platform | Binary suffix |
|---|---|
| macOS Apple Silicon | `macos-aarch64` |
| Linux x86_64 | `linux-x86_64` |
| Linux aarch64 | `linux-aarch64` |
| Windows x86_64 | `windows-x86_64.exe` |

## License

First-party code is licensed under the Apache License, Version 2.0. See [`LICENSE`](LICENSE) and [`THIRD-PARTY-NOTICES`](THIRD-PARTY-NOTICES) for details.
