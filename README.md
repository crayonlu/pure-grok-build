<div align="center">

<h1>
  <picture>
    <source media="(prefers-color-scheme: dark)" srcset="https://media.x.ai/v1/website/spacexai-symbol-white-transparent-0c31957f.png">
    <source media="(prefers-color-scheme: light)" srcset="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png">
    <img alt="SpaceXAI logo" src="https://media.x.ai/v1/website/spacexai-symbol-black-transparent-6435cf42.png" width="96">
  </picture>
  <br>
  Grok Build (<code>grok</code>)
</h1>

<strong>Grok Build</strong> is SpaceXAI's terminal-based AI coding agent. It runs
as a full-screen TUI that understands your codebase, edits files, executes
shell commands, searches the web, and manages long-running tasks — interactively,
headlessly for scripting/CI, or embedded in editors via the Agent Client
Protocol (ACP).

<a href="#quick-start-english">Quick start (English)</a> ·
<a href="#快速开始中文">快速开始（中文）</a> ·
<a href="#building-from-source">Building from source</a> ·
<a href="#documentation">Documentation</a> ·
<a href="#repository-layout">Repository layout</a> ·
<a href="#development">Development</a> ·
<a href="#license">License</a>

<img alt="Grok Build TUI" src="https://media.x.ai/v1/website/universe-tui-screenshot-6f7a0837.png">

Learn more about Grok Build at <a href="https://x.ai/cli">x.ai/cli</a>.

This repository contains the Rust source for the <code>grok</code> CLI/TUI and
its agent runtime. It is synced periodically from the SpaceXAI monorepo.

A small <code>SOURCE_REV</code> file at the root records the full monorepo commit
SHA for the version of the code present in this tree.

</div>

---

## Quick start (English)

This is the <code>pure-grok-build</code> fork of Grok Build. The executable is
still called <code>grok</code>. The commands below install this fork from its
release mirror, not the first-party xAI service.

### 1. Install with one command

macOS, Linux, WSL, or Git Bash:

~~~sh
curl -fsSL https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.sh | GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli bash
~~~

Windows PowerShell:

~~~powershell
$env:GROK_CLI_BASE_URL="https://grok.cyncyn.xyz/cli"; irm https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.ps1 | iex
~~~

The installer puts the command under <code>~/.grok/bin</code> (PowerShell:
<code>%USERPROFILE%\.grok\bin</code>) and adds it to the user <code>PATH</code>.
Start a new shell if <code>grok</code> is not found, then verify:

~~~sh
grok --version
~~~

The current release mirror publishes macOS arm64, Linux arm64/x86_64, and
Windows x86_64 artifacts. macOS Intel is not currently published by this fork.

### 2. Configure the update source once

The <code>GROK_CLI_BASE_URL</code> environment variable above selects the mirror
during installation. In <code>open</code> mode, persist the same source so
<code>grok update</code> and the background updater continue to use this fork.

Edit <code>~/.grok/config.toml</code> (PowerShell:
<code>%USERPROFILE%\.grok\config.toml</code>) and add or update these tables.
Do not duplicate a table that already exists.

~~~toml
[overlay]
mode = "open"

[overlay.update_source]
kind = "base_url"
location = "https://grok.cyncyn.xyz/cli"
channel = "stable"
~~~

Now use Grok normally:

~~~sh
cd your-project
grok
grok -p "Explain this codebase"
grok update
~~~

In <code>open</code> mode, an explicit update source is required. If you prefer
a per-shell override instead of the config file, export
<code>GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli</code> before running
<code>grok update</code>. The persistent TOML setting is recommended because it
also covers automatic updates.

### 3. Configure your provider/model

The update source is independent from the model endpoint. Configure a BYOK
model in the same file, for example:

~~~toml
[model.my-model]
model = "your-model-id"
base_url = "https://api.example.com/v1"
env_key = "MY_MODEL_API_KEY"
api_backend = "chat_completions"

[models]
default = "my-model"
~~~

See [Custom Models](crates/codegen/xai-grok-pager/docs/user-guide/11-custom-models.md)
for Responses/Messages backends, image/video/search providers, and headers.
Memory embeddings have their own endpoint, model, and credential settings; see
[Cross-Session Memory](crates/codegen/xai-grok-pager/docs/user-guide/13-memory.md).

### Uninstall an existing Grok Build installation

The official shell and PowerShell installers do not provide a separate
uninstall command. They use the same <code>~/.grok</code> installation directory
as this fork.

For an installation made with <code>curl | bash</code>, the fork installer, or
the official PowerShell script, remove only the executable/cache files and keep
your login, sessions, and configuration.

macOS/Linux/WSL/Git Bash:

~~~sh
rm -f "$HOME/.grok/bin/grok" "$HOME/.grok/bin/agent"
rm -rf "$HOME/.grok/downloads" "$HOME/.grok/completions"
~~~

Windows PowerShell:

~~~powershell
Remove-Item "$env:USERPROFILE\.grok\bin\grok.exe","$env:USERPROFILE\.grok\bin\agent.exe" -Force -ErrorAction SilentlyContinue
Remove-Item "$env:USERPROFILE\.grok\downloads","$env:USERPROFILE\.grok\completions" -Recurse -Force -ErrorAction SilentlyContinue
~~~

The installer may have added a block marked <code>grok installer</code> to
<code>~/.zshrc</code>, <code>~/.bashrc</code>,
<code>~/.config/fish/config.fish</code>, or the PowerShell profile. Remove that
marked block, start a new shell, and check <code>command -v grok</code>
(PowerShell: <code>Get-Command grok -ErrorAction SilentlyContinue</code>).
If the command resolves to <code>~/.local/bin</code> or
<code>/usr/local/bin</code>, remove that file only when it is a symlink pointing
into <code>~/.grok</code>.

Other installation methods:

| Original method | Uninstall |
| --- | --- |
| <code>npm i -g @xai-official/grok</code> | <code>npm uninstall -g @xai-official/grok</code> |
| <code>cargo install ...</code> | Run <code>cargo uninstall &lt;installed-package&gt;</code>; if it was a copied binary, delete that exact file. |
| Homebrew/apt/another package manager | Uninstall the package with the same manager, then run <code>command -v grok</code> and remove only any leftover <code>grok</code>/<code>agent</code> file. |
| Downloaded/copied binary | Delete the exact binary or symlink reported by <code>command -v grok</code> (PowerShell: <code>Get-Command grok</code>). |

To remove all local Grok data as well, back it up first and then delete the
whole directory:

~~~sh
rm -rf "$HOME/.grok"
~~~

~~~powershell
Remove-Item "$env:USERPROFILE\.grok" -Recurse -Force
~~~

This also deletes <code>auth.json</code>, sessions, memory, plugins, and your
config. It does not uninstall an npm/package-manager copy outside
<code>~/.grok</code>; use the matching command in the table too.

### Migration from the official installation

1. Uninstall the old copy using the method above; keep
   <code>~/.grok/config.toml</code> if you want to preserve settings.
2. Run the one-line fork installer.
3. Set <code>[overlay] mode = "open"</code> and the
   <code>[overlay.update_source]</code> block.
4. Configure your model/API key, run <code>grok</code>, and use
   <code>grok update</code> for future fork releases.

Do not use the unmodified official command
<code>curl -fsSL https://x.ai/cli/install.sh | bash</code> when you intend to
install this fork: it points at xAI's binaries and update service.

## 快速开始（中文）

这是 <code>pure-grok-build</code> fork，安装后的命令仍然是 <code>grok</code>。
下面的命令会从 fork 自己的发布镜像安装，不会安装 xAI 官方版本。

### 1. 一行安装

macOS、Linux、WSL 或 Git Bash：

~~~sh
curl -fsSL https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.sh | GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli bash
~~~

Windows PowerShell：

~~~powershell
$env:GROK_CLI_BASE_URL="https://grok.cyncyn.xyz/cli"; irm https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.ps1 | iex
~~~

安装程序会把命令放在 <code>~/.grok/bin</code>（PowerShell 为
<code>%USERPROFILE%\.grok\bin</code>），并尝试加入用户 <code>PATH</code>。如果
新终端仍然提示找不到命令，请重新打开终端，然后检查：

~~~sh
grok --version
~~~

当前 fork 镜像发布 macOS arm64、Linux arm64/x86_64、Windows x86_64；暂时没有
macOS Intel 构建产物。

### 2. 一次配置更新源

上面的 <code>GROK_CLI_BASE_URL</code> 只负责安装时选择镜像。<code>open</code>
模式下还必须把同一个镜像写进配置，<code>grok update</code> 和后台自动更新才会
继续跟随 fork。

编辑 <code>~/.grok/config.toml</code>（PowerShell 为
<code>%USERPROFILE%\.grok\config.toml</code>），新增或修改下面的配置；如果文件里
已经有这些表，请直接修改，不要重复写同一个 TOML 表：

~~~toml
[overlay]
mode = "open"

[overlay.update_source]
kind = "base_url"
location = "https://grok.cyncyn.xyz/cli"
channel = "stable"
~~~

然后就可以使用：

~~~sh
cd your-project
grok
grok -p "Explain this codebase"
grok update
~~~

<code>open</code> 模式要求明确的更新源。如果不想写入配置，也可以每次运行更新
前设置 <code>GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli</code>；但写入 TOML 更好，
因为后台自动更新也会使用它。

### 3. 配置模型和 API Key

更新源和模型接口是两回事。在同一个配置文件中按需配置 BYOK 模型：

~~~toml
[model.my-model]
model = "your-model-id"
base_url = "https://api.example.com/v1"
env_key = "MY_MODEL_API_KEY"
api_backend = "chat_completions"

[models]
default = "my-model"
~~~

Responses/Messages、图片/视频/Web Search 的配置请看
[Custom Models](crates/codegen/xai-grok-pager/docs/user-guide/11-custom-models.md)；
memory embedding 有独立的 endpoint、model 和凭据，请看
[Cross-Session Memory](crates/codegen/xai-grok-pager/docs/user-guide/13-memory.md)。

### 卸载原来的 Grok Build

xAI 官方 shell/PowerShell 安装脚本没有单独的卸载命令，文件都在
<code>~/.grok</code> 下；本 fork 也使用同一目录。

如果原来是 <code>curl | bash</code>、本 fork 安装脚本或官方 PowerShell 脚本安装的，
下面的命令只删除程序和缓存，保留登录、会话和配置。

macOS/Linux/WSL/Git Bash：

~~~sh
rm -f "$HOME/.grok/bin/grok" "$HOME/.grok/bin/agent"
rm -rf "$HOME/.grok/downloads" "$HOME/.grok/completions"
~~~

Windows PowerShell：

~~~powershell
Remove-Item "$env:USERPROFILE\.grok\bin\grok.exe","$env:USERPROFILE\.grok\bin\agent.exe" -Force -ErrorAction SilentlyContinue
Remove-Item "$env:USERPROFILE\.grok\downloads","$env:USERPROFILE\.grok\completions" -Recurse -Force -ErrorAction SilentlyContinue
~~~

安装脚本可能在 <code>~/.zshrc</code>、<code>~/.bashrc</code>、
<code>~/.config/fish/config.fish</code> 或 PowerShell profile 中加入带有
<code>grok installer</code> 标记的区块。请删除该区块，重开终端，再用
<code>command -v grok</code>（PowerShell 用
<code>Get-Command grok -ErrorAction SilentlyContinue</code>）确认。
如果命令解析到 <code>~/.local/bin</code> 或 <code>/usr/local/bin</code>，只有在
确认它是指向 <code>~/.grok</code> 的软链接时才删除该文件。

其他安装方式对应如下：

| 原来的安装方式 | 卸载命令/动作 |
| --- | --- |
| <code>npm i -g @xai-official/grok</code> | <code>npm uninstall -g @xai-official/grok</code> |
| <code>cargo install ...</code> | 执行 <code>cargo uninstall &lt;安装的包名&gt;</code>；如果只是复制二进制，则删除对应文件。 |
| Homebrew/apt/其他包管理器 | 用同一个包管理器卸载，再用 <code>command -v grok</code> 查找并只删除遗留的 <code>grok</code>/<code>agent</code> 文件。 |
| 手动下载/复制二进制 | 删除 <code>command -v grok</code> 找到的准确文件或软链接（PowerShell 用 <code>Get-Command grok</code>）。 |

如果还要删除所有本地数据，请先备份，再删除整个目录：

~~~sh
rm -rf "$HOME/.grok"
~~~

~~~powershell
Remove-Item "$env:USERPROFILE\.grok" -Recurse -Force
~~~

这会同时删除 <code>auth.json</code>、会话、memory、插件和配置；npm/包管理器安装在
其他位置的程序仍需按上表卸载。

### 从官方版本迁移到本 fork

1. 按上面的对应方式卸载旧版本；想保留设置就不要删除
   <code>~/.grok/config.toml</code>。
2. 执行 fork 的一行安装命令。
3. 设置 <code>[overlay] mode = "open"</code> 和
   <code>[overlay.update_source]</code>。
4. 配置模型/API Key，运行 <code>grok</code>；以后用 <code>grok update</code> 更新 fork。

如果目标是本 fork，请不要使用未修改的官方命令
<code>curl -fsSL https://x.ai/cli/install.sh | bash</code>，因为它会指向 xAI 官方二进制
和更新服务。

## pure-grok-build overlay

This fork keeps the upstream Grok Build runtime but adds a merge-friendly,
provider-neutral overlay. In <code>open</code> mode the chat model and auxiliary
services use explicit BYOK endpoints; xAI session credentials are never silently
sent to third-party hosts. <code>upstream</code> and <code>xai_compat</code> modes remain
available for deployments that intentionally need first-party service behavior.

The update-source setting is the <code>[overlay.update_source]</code> block shown above.
For the complete provider-neutral schema, see [Custom Models](crates/codegen/xai-grok-pager/docs/user-guide/11-custom-models.md),
[Cross-Session Memory](crates/codegen/xai-grok-pager/docs/user-guide/13-memory.md),
and [Self-Hosted Update Source](crates/codegen/xai-grok-pager/docs/user-guide/25-self-hosted-updates.md).

The nightly sync/release workflow is source-revision aware and opens a pull
request when an upstream change cannot be applied automatically. This keeps the
overlay isolated at composition roots so future upstream syncs remain small and
reviewable.

## Building from source

Requirements:

- <strong>Rust</strong> — the toolchain is pinned by
  <a href="rust-toolchain.toml"><code>rust-toolchain.toml</code></a>;
  <code>rustup</code> installs it automatically on first build.
- <strong><a href="https://dotslash-cli.com">DotSlash</a></strong> — required so
  hermetic tools under <a href="bin/"><code>bin/</code></a> (notably
  <a href="bin/protoc"><code>bin/protoc</code></a>) can download and run.
  Install it and ensure <code>dotslash</code> is on your <code>PATH</code> before
  building:

~~~sh
cargo install dotslash
# or: prebuilt packages — https://dotslash-cli.com/docs/installation/
/usr/bin/env dotslash --help
~~~

- <strong>protoc</strong> — proto codegen resolves <a href="bin/protoc"><code>bin/protoc</code></a>
  via DotSlash, or falls back to a <code>protoc</code> on <code>PATH</code> / <code>$PROTOC</code>.
- macOS and Linux are supported build hosts. Windows release builds are
  produced by GitHub Actions; local Windows builds remain best-effort.

~~~sh
cargo run -p xai-grok-pager-bin
cargo build -p xai-grok-pager-bin --release
cargo check -p xai-grok-pager-bin
~~~

The binary artifact is named <code>xai-grok-pager</code>; release installs expose
it as <code>grok</code>. On first launch it opens your browser to authenticate —
see the [authentication guide](crates/codegen/xai-grok-pager/docs/user-guide/02-authentication.md).

## Documentation

Full online documentation is available at
[docs.x.ai/build/overview](https://docs.x.ai/build/overview).

The user guide ships with the pager crate:
[crates/codegen/xai-grok-pager/docs/user-guide/](crates/codegen/xai-grok-pager/docs/user-guide/)
— getting started, keyboard shortcuts, slash commands, configuration, theming,
MCP servers, skills, plugins, hooks, headless mode, sandboxing, and more.

## Repository layout

| Path | Contents |
|------|----------|
| <code>crates/codegen/xai-grok-pager-bin</code> | Composition-root package; builds the <code>xai-grok-pager</code> binary |
| <code>crates/codegen/xai-grok-pager</code> | The TUI: scrollback, prompt, modals, rendering |
| <code>crates/codegen/xai-grok-shell</code> | Agent runtime + leader/stdio/headless entry points |
| <code>crates/codegen/xai-grok-tools</code> | Tool implementations (terminal, file edit, search, ...) |
| <code>crates/codegen/xai-grok-workspace</code> | Host filesystem, VCS, execution, checkpoints |
| <code>crates/codegen/...</code> | The rest of the CLI crate closure (config, MCP, markdown, sandbox, ...) |
| <code>crates/common/</code>, <code>crates/build/</code>, <code>prod/mc/</code> | Small shared leaf crates pulled in by the closure |
| <code>third_party/</code> | Vendored upstream source (Mermaid diagram stack) — see below |

> [!IMPORTANT]
> The root <code>Cargo.toml</code> (workspace members, dependency versions, lints,
> profiles) is <strong>generated</strong> — treat it as read-only. Prefer editing
> per-crate <code>Cargo.toml</code> files.

## Development

~~~sh
cargo check -p &lt;crate&gt;
cargo test -p xai-grok-config
cargo clippy -p &lt;crate&gt;
cargo fmt --all
~~~

## Contributing

> [!NOTE]
> External contributions are not accepted. See <a href="CONTRIBUTING.md"><code>CONTRIBUTING.md</code></a>.

## License

First-party code in this repository is licensed under the <strong>Apache License,
Version 2.0</strong> — see <a href="LICENSE"><code>LICENSE</code></a>.

Third-party and vendored code remains under its original licenses. See:

- <a href="THIRD-PARTY-NOTICES"><code>THIRD-PARTY-NOTICES</code></a> — crates.io / git dependencies,
  bundled UI themes, and in-tree source ports (including openai/codex and
  sst/opencode tool implementations)
- <a href="crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md"><code>crates/codegen/xai-grok-tools/THIRD_PARTY_NOTICES.md</code></a>
  — crate-local notice for the codex and opencode ports (license texts plus Apache §4(b)
  change notice)
- <a href="third_party/NOTICE"><code>third_party/NOTICE</code></a> — vendored Mermaid-stack index
