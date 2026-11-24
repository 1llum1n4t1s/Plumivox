# 安装

## Windows：通过 Setup 安装（推荐）

1. 下载并运行 [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)。这是按用户安装，无需管理员权限。Setup 带有作者本人的代码签名（`Open Source Developer Yuichiro Shinozaki`）。
2. 程序会安装到 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`；如果 PATH 中有 `claude`、`codex` CLI，会自动向 Claude Code（按用户）和 Codex 注册 MCP 服务器 `plumivox`（参数 `--stdio`）。如果未找到 CLI，可在之后安装 CLI，然后在下次登录时或从开始菜单启动 Plumivox 时完成注册。
3. 按照安装完成后界面上的提示，在终端中运行 `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup`，进行[初始设置](../../../README.zh-CN.md#初始设置)。Claude Code 和 Codex 从新会话开始即可使用。

更新是自动进行的。MCP 运行期间会从 R2（`https://plumivox-updates.kagayoi.com/velopack/`）获取新版本，并在登录电脑时，仅当 Plumivox（MCP、共享中继）未运行时才应用。如果正在运行，则推迟到下次登录，不会强制终止。应用前会确认签名者，不一致则不应用。要停止自动更新，请将 `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` 设为 `{"auto_update": false}`。运行记录位于同一文件夹下的 `app-update.log`。

卸载时，在“设置”→“应用”列表中选择 Plumivox。卸载只会移除指向自身的 Claude Code、Codex 的 `plumivox` 注册以及登录时更新的注册，并保留设置和会话的数据文件夹（`%LOCALAPPDATA%\Kagayoi\Plumivox`）。如果只想先移除注册，可用 `plumivox.exe --uninstall-setup` 确认计划，再用 `--uninstall-setup --apply` 执行。

从插件（见下文）迁移时：Setup 会把指向旧插件 `dist` 等位置的同名 `plumivox` 注册替换为指向安装位置的注册。插件自身携带的 MCP（`plumivox@plumivox-local`）是另一个注册，为避免 Plumivox 通过两条途径启动，请将其禁用或移除该插件。在 Codex 中，添加与[Toolport 一节](#通过-toolport-使用)相同的 `enabled = false`（或移除插件）；在 Claude Code 中，通过 `/plugin` 禁用或卸载该插件。Codex 会优先使用同名注册而不是插件的 MCP，因此只要 Setup 的注册存在，通常只会启动一个，但 Claude Code 可能会同时启动两个。Setup 不会修改 Codex 或 Claude Code 的设置；如果插件仍处于启用状态，它只会发出警告（启动 Plumivox 时显示的指引、`app-update.log`，以及 `app-update\setup-registration.json` 中的 `legacyPlugins`）。Toolport 的注册同样不会被自动改写；如果使用 Toolport，请按照[Toolport 一节](#通过-toolport-使用)自行检查。

## 通过插件安装（macOS、传统方式）

在开始分发 Setup 之前，macOS（Apple Silicon）使用此方式。Windows 也可以继续使用。

从 GitHub 仓库 `main` 的最新提交安装。不引用用户的开发用克隆，也不固定到标签或提交 SHA。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` 是市场的标识符，获取来源是上述 GitHub URL。由于随附了 Windows x64 用的 `dist/plumivox.exe` 和 Apple Silicon Mac 用的 `dist/plumivox`（ARM64），用户无需自行安装依赖或手动添加 MCP 设置。

开发用 CI 会把在 Windows 和 macOS 上验证过的产物保存到 Cloudflare R2，并在分发准备时交接。[产物的保存与获取步骤](../../distribution.md)面向开发者。插件用户无需配置 R2 或凭据，安装和更新仍然从 GitHub 的 `main` 进行。

更新时，获取 `main` 的最新状态并安装，然后重启 Codex 以加载。

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

确认列表中 `installed` 显示的版本号。已安装的插件在更新后不要再次 `add`，而是重启 Codex 以加载新的 MCP 和技能。

跟踪 GitHub 的注册与更新方法请参阅 [Codex 插件文档](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)。

## 通过 Toolport 使用

可以只把 MCP 注册到 Toolport。Plumivox 不会自动改写 Toolport 的注册，请根据安装方式添加或修改以下 stdio 服务器。不要指定开发用克隆或带版本号的安装缓存。

- 名称：`plumivox`
- 命令：Windows 的 Setup 版为 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`（请将 `%LOCALAPPDATA%` 展开为实际路径后指定。`current` 即使在更新后内容被替换，路径也不会改变）。插件版请用 `codex plugin marketplace list` 确认 `plumivox-local` 的已获取根目录，Windows 为 `<已获取根目录>/dist/plumivox.exe`，macOS 为 `<已获取根目录>/dist/plumivox`
- 参数：`--stdio`
- 工作目录：用户的主目录
- 请求超时：300000 毫秒（包括登录和中继的准备时间）

在 Toolport 中为 Codex 使用的配置文件启用它，并用 `toolport_search_tools` 的 `server="plumivox"` 确认 11 个工具。搜索并调用 `list_accounts`，确认设置可以读取后再禁用直接连接。Setup 版使用 `codex mcp remove plumivox` 和 `claude mcp remove plumivox --scope user` 移除直接注册（注册完成一次之后，在用 Setup 重新安装之前不会自动重新注册）。插件版请在 Codex 的用户设置 `config.toml` 中添加以下内容，插件本体保持启用，以保留用于设置的技能。

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

重启 Codex 后直接连接即被禁用。此后的操作是在 Toolport 中搜索工具，并把返回的实际工具名传给 `toolport_call_tool`。API 密钥、Cookie 和 SSH 密钥从 Plumivox 的现有设置中读取，不会移到 Toolport 的参数中。

插件版的更新通过上述 `codex plugin marketplace upgrade plumivox-local` 进行。Toolport 正在运行的服务器使用已加载的代码，因此更新后请在 Toolport 中禁用并重新启用 `plumivox`，以加载新的可执行文件。如果已获取根目录发生变化，也请修改已注册的命令。启动时不会进行 Git 获取或依赖安装。
