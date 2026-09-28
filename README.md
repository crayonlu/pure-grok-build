# pure-grok-build

A provider-neutral, BYOK-focused fork of Grok Build. This README documents only
the fork-specific installation, migration, update-source, provider, embedding,
and safety behavior.

The executable is still <code>grok</code>. For upstream Grok Build features and
commands, see the [upstream project](https://github.com/xai-org/grok-build) and
[official documentation](https://docs.x.ai/build/overview).

## Quick start (English)

### 1. Install this fork with one command

macOS, Linux, WSL, or Git Bash:

~~~sh
curl -fsSL https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.sh | GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli bash
~~~

Windows PowerShell:

~~~powershell
$env:GROK_CLI_BASE_URL="https://grok.cyncyn.xyz/cli"; irm https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.ps1 | iex
~~~

Verify:

~~~sh
grok --version
~~~

The installer stores binaries under <code>~/.grok/bin</code> (PowerShell:
<code>%USERPROFILE%\.grok\bin</code>) and adds that directory to the user
<code>PATH</code>. Open a new shell if the command is not found.

Current mirror artifacts:

| Platform | Architectures |
| --- | --- |
| macOS | arm64 |
| Linux | arm64, x86_64 |
| Windows | x86_64 |

macOS Intel is not currently published by this fork.

Do not use the unmodified official command
<code>curl -fsSL https://x.ai/cli/install.sh | bash</code> when you want this
fork; that command installs xAI's binaries and updater.

### 2. Configure the fork update source

The <code>GROK_CLI_BASE_URL</code> variable selects the mirror during install.
Persist the same source so <code>grok update</code> and background updates keep
using this fork.

Edit <code>~/.grok/config.toml</code> (PowerShell:
<code>%USERPROFILE%\.grok\config.toml</code>). Add or update these tables; do
not duplicate tables that already exist:

~~~toml
[overlay]
mode = "open"

[overlay.update_source]
kind = "base_url"
location = "https://grok.cyncyn.xyz/cli"
channel = "stable"
~~~

Then run:

~~~sh
cd your-project
grok
grok update
~~~

In <code>open</code> mode, an explicit update source is required. A temporary
per-shell override is also supported:

~~~sh
export GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli
grok update
~~~

The persistent TOML setting is recommended because it also applies to automatic
updates. The mirror layout and source-resolution rules are documented in
[Self-Hosted Update Source](crates/codegen/xai-grok-pager/docs/user-guide/25-self-hosted-updates.md).

### 3. Configure your model/provider

The update source is independent from the model endpoint. A minimal BYOK model
configuration looks like this:

~~~toml
[model.my-model]
model = "your-model-id"
base_url = "https://api.example.com/v1"
env_key = "MY_MODEL_API_KEY"
api_backend = "chat_completions"

[models]
default = "my-model"
~~~

Use the [Custom Models guide](crates/codegen/xai-grok-pager/docs/user-guide/11-custom-models.md)
for Responses/Messages backends, reasoning effort, image/video, web search,
extra headers, and provider-specific credentials.

### 4. Configure memory embeddings independently

Memory does not reuse the chat model accidentally. Set an embedding model when
you want semantic memory search:

~~~toml
[memory]
enabled = true

[memory.embedding]
provider = "api"
base_url = "https://embedding.example/v1"
model = "your-embedding-model"
env_key = "EMBEDDING_API_KEY"
dimensions = 1024
auth_scheme = "bearer"
~~~

If <code>model</code> is omitted, memory stays FTS-only. See
[Cross-Session Memory](crates/codegen/xai-grok-pager/docs/user-guide/13-memory.md)
for credential precedence, custom headers, and endpoint safety.

### What this fork changes

- <code>open</code> mode is provider-neutral and fail-closed: configure the
  endpoints you intend to use; session/OAuth credentials are not silently sent
  to unrelated hosts.
- Chat, image/video, web search, and memory embedding providers are configured
  independently.
- The updater can use the fork's GitHub Releases or a self-hosted base URL; the
  default fork mirror is <code>https://grok.cyncyn.xyz/cli</code>.
- Upstream synchronization is kept at composition roots so upstream updates can
  be applied with small, reviewable diffs.

## Uninstall and migration (English)

The official shell and PowerShell installers do not provide a separate
uninstall command. They and this fork use <code>~/.grok</code>.

### Remove a script-installed copy but keep user data

macOS/Linux/WSL/Git Bash:

~~~sh
# `agent` is included only to clean up the legacy alias from older releases.
rm -f "$HOME/.grok/bin/grok" "$HOME/.grok/bin/agent"
rm -rf "$HOME/.grok/downloads" "$HOME/.grok/completions"
~~~

Windows PowerShell:

~~~powershell
# `agent.exe` is included only to clean up the legacy alias from older releases.
Remove-Item "$env:USERPROFILE\.grok\bin\grok.exe","$env:USERPROFILE\.grok\bin\agent.exe" -Force -ErrorAction SilentlyContinue
Remove-Item "$env:USERPROFILE\.grok\downloads","$env:USERPROFILE\.grok\completions" -Recurse -Force -ErrorAction SilentlyContinue
~~~

Remove the installer block marked <code>grok installer</code> from
<code>~/.zshrc</code>, <code>~/.bashrc</code>,
<code>~/.config/fish/config.fish</code>, or the PowerShell profile. If
<code>command -v grok</code> resolves to <code>~/.local/bin</code> or
<code>/usr/local/bin</code>, remove that file only when it is a symlink pointing
into <code>~/.grok</code>.

### Remove other installation methods

| Original method | Uninstall |
| --- | --- |
| <code>npm i -g @xai-official/grok</code> | <code>npm uninstall -g @xai-official/grok</code> |
| <code>cargo install ...</code> | Run <code>cargo uninstall &lt;installed-package&gt;</code>; for a copied binary, delete that exact file. |
| Homebrew/apt/another package manager | Uninstall with the same manager, then check <code>command -v grok</code> for leftovers. |
| Downloaded/copied binary | Delete the exact binary or symlink reported by <code>command -v grok</code> (PowerShell: <code>Get-Command grok</code>). |

### Remove all local Grok data

Back up anything you need first. This deletes credentials, sessions, memory,
plugins, and configuration:

~~~sh
rm -rf "$HOME/.grok"
~~~

~~~powershell
Remove-Item "$env:USERPROFILE\.grok" -Recurse -Force
~~~

This does not remove an npm or package-manager installation outside
<code>~/.grok</code>; use the matching uninstall command as well.

### Migrate from the official installation

1. Uninstall the old copy using the matching method above. Keep
   <code>~/.grok/config.toml</code> if you want to preserve settings.
2. Run the fork installer from the [quick-start commands](#quick-start-english).
3. Set <code>[overlay] mode = "open"</code> and
   <code>[overlay.update_source]</code>.
4. Configure the model/API key and optional independent embedding provider.
5. Run <code>grok</code>; use <code>grok update</code> for future fork releases.

## 快速开始（中文）

这是一个以 BYOK 和 provider-neutral 为目标的 Grok Build fork。本 README 只说明
fork 自己的安装、迁移、更新源、模型、embedding 和安全策略。

安装后的命令仍然是 <code>grok</code>。上游功能和通用命令请看
[上游项目](https://github.com/xai-org/grok-build)和[官方文档](https://docs.x.ai/build/overview)。

### 1. 一行安装 fork

macOS、Linux、WSL 或 Git Bash：

~~~sh
curl -fsSL https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.sh | GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli bash
~~~

Windows PowerShell：

~~~powershell
$env:GROK_CLI_BASE_URL="https://grok.cyncyn.xyz/cli"; irm https://raw.githubusercontent.com/crayonlu/pure-grok-build/main/crates/codegen/xai-grok-pager/scripts/install.ps1 | iex
~~~

检查安装：

~~~sh
grok --version
~~~

程序位于 <code>~/.grok/bin</code>（PowerShell 为
<code>%USERPROFILE%\.grok\bin</code>），并会尝试加入用户 <code>PATH</code>。如果
提示找不到命令，请重新打开终端。

当前镜像提供 macOS arm64、Linux arm64/x86_64、Windows x86_64；暂时没有
macOS Intel 构建产物。

如果要安装本 fork，不要使用未修改的官方命令
<code>curl -fsSL https://x.ai/cli/install.sh | bash</code>，它会安装 xAI
官方二进制和更新服务。

### 2. 配置 fork 的更新源

<code>GROK_CLI_BASE_URL</code> 只负责安装时选择镜像。为了让
<code>grok update</code> 和后台更新继续使用 fork，请把同一个镜像写入
<code>~/.grok/config.toml</code>（PowerShell 为
<code>%USERPROFILE%\.grok\config.toml</code>）。

如果文件里已有对应表，请直接修改，不要重复创建：

~~~toml
[overlay]
mode = "open"

[overlay.update_source]
kind = "base_url"
location = "https://grok.cyncyn.xyz/cli"
channel = "stable"
~~~

使用：

~~~sh
cd your-project
grok
grok update
~~~

<code>open</code> 模式要求明确的更新源。如果只想临时设置，可以：

~~~sh
export GROK_CLI_BASE_URL=https://grok.cyncyn.xyz/cli
grok update
~~~

持久化写入 TOML 更好，因为后台自动更新也会读取它。完整规则请看
[Self-Hosted Update Source](crates/codegen/xai-grok-pager/docs/user-guide/25-self-hosted-updates.md)。

### 3. 配置模型和 provider

更新源和模型接口是两回事。最小 BYOK 配置示例：

~~~toml
[model.my-model]
model = "your-model-id"
base_url = "https://api.example.com/v1"
env_key = "MY_MODEL_API_KEY"
api_backend = "chat_completions"

[models]
default = "my-model"
~~~

Responses/Messages、reasoning effort、图片/视频、Web Search、额外 header
和各家 provider 的完整配置请看
[Custom Models](crates/codegen/xai-grok-pager/docs/user-guide/11-custom-models.md)。

### 4. 独立配置 memory embedding

embedding 不会误用聊天模型。需要语义 memory 搜索时配置独立模型：

~~~toml
[memory]
enabled = true

[memory.embedding]
provider = "api"
base_url = "https://embedding.example/v1"
model = "your-embedding-model"
env_key = "EMBEDDING_API_KEY"
dimensions = 1024
auth_scheme = "bearer"
~~~

不设置 <code>model</code> 时保持 FTS-only。凭据优先级、额外 header 和 endpoint
安全规则请看
[Cross-Session Memory](crates/codegen/xai-grok-pager/docs/user-guide/13-memory.md)。

### fork 自己做了什么

- <code>open</code> 模式是 provider-neutral 且 fail-closed：只使用明确配置的
  endpoint，不会把 session/OAuth 凭据静默转发到不相关的主机。
- 聊天、图片/视频、Web Search、memory embedding 可以分别配置。
- 更新器支持 fork 的 GitHub Releases 或自托管 base URL；默认镜像是
  <code>https://grok.cyncyn.xyz/cli</code>。
- upstream 同步集中在 composition root，尽量让后续同步只产生小而可审查的 diff。

## 卸载与迁移（中文）

官方 shell/PowerShell 安装脚本没有单独的卸载命令；官方版本和本 fork 都使用
<code>~/.grok</code>。

### 卸载脚本安装但保留用户数据

macOS/Linux/WSL/Git Bash：

~~~sh
# `agent` is included only to clean up the legacy alias from older releases。
rm -f "$HOME/.grok/bin/grok" "$HOME/.grok/bin/agent"
rm -rf "$HOME/.grok/downloads" "$HOME/.grok/completions"
~~~

Windows PowerShell：

~~~powershell
# `agent.exe` is included only to clean up the legacy alias from older releases。
Remove-Item "$env:USERPROFILE\.grok\bin\grok.exe","$env:USERPROFILE\.grok\bin\agent.exe" -Force -ErrorAction SilentlyContinue
Remove-Item "$env:USERPROFILE\.grok\downloads","$env:USERPROFILE\.grok\completions" -Recurse -Force -ErrorAction SilentlyContinue
~~~

请从 <code>~/.zshrc</code>、<code>~/.bashrc</code>、
<code>~/.config/fish/config.fish</code> 或 PowerShell profile 中删除带有
<code>grok installer</code> 标记的区块。若 <code>command -v grok</code> 指向
<code>~/.local/bin</code> 或 <code>/usr/local/bin</code>，只有确认它是指向
<code>~/.grok</code> 的软链接后才删除。

### 卸载其他安装方式

| 原来的安装方式 | 卸载命令/动作 |
| --- | --- |
| <code>npm i -g @xai-official/grok</code> | <code>npm uninstall -g @xai-official/grok</code> |
| <code>cargo install ...</code> | 执行 <code>cargo uninstall &lt;安装的包名&gt;</code>；复制的二进制则删除对应文件。 |
| Homebrew/apt/其他包管理器 | 用同一个包管理器卸载，再用 <code>command -v grok</code> 检查遗留文件。 |
| 手动下载/复制二进制 | 删除 <code>command -v grok</code> 找到的准确文件或软链接（PowerShell 用 <code>Get-Command grok</code>）。 |

### 删除全部本地数据

请先备份。以下命令会删除凭据、会话、memory、插件和配置：

~~~sh
rm -rf "$HOME/.grok"
~~~

~~~powershell
Remove-Item "$env:USERPROFILE\.grok" -Recurse -Force
~~~

npm 或其他包管理器安装在 <code>~/.grok</code> 之外，仍需使用对应卸载命令。

### 从官方版本迁移

1. 按对应方式卸载旧版本；想保留设置就不要删除
   <code>~/.grok/config.toml</code>。
2. 执行[快速开始](#快速开始中文)中的 fork 安装命令。
3. 设置 <code>[overlay] mode = "open"</code> 和
   <code>[overlay.update_source]</code>。
4. 配置模型/API Key，以及可选的独立 embedding provider。
5. 运行 <code>grok</code>，以后使用 <code>grok update</code> 更新 fork。

## Fork-specific documentation

- [Custom Models](crates/codegen/xai-grok-pager/docs/user-guide/11-custom-models.md)
- [Cross-Session Memory](crates/codegen/xai-grok-pager/docs/user-guide/13-memory.md)
- [Self-Hosted Update Source](crates/codegen/xai-grok-pager/docs/user-guide/25-self-hosted-updates.md)
- [Configuration reference](crates/codegen/xai-grok-pager/docs/user-guide/05-configuration.md)
- [Upstream Grok Build documentation](https://docs.x.ai/build/overview)

## License and upstream notice

The upstream source and third-party components retain their original licenses.
See [LICENSE](LICENSE) and [THIRD-PARTY-NOTICES](THIRD-PARTY-NOTICES). This fork
does not duplicate the upstream README; it documents only fork-specific behavior.
