[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

Plumivox 是一个基于 TwitterAPI.io 的本地 MCP 服务器／Codex 插件，提供向 X 发帖、按关键词和时间段搜索，以及读取所关注账号的公开帖子等功能。

## 环境要求

- Windows x64 或 Apple Silicon Mac（macOS ARM64）。MCP 本体是用 Rust 编写的原生可执行文件，无需安装 Node.js 或 Rust。
- Windows 和 macOS 的初始设置使用随附的可执行文件，无需 PowerShell 7 (`pwsh`)。
- TwitterAPI.io 的 API 密钥。使用 API 会产生该服务的按量计费。
- 发帖需要目标账号的 v2 登录 Cookie 和代理。除了已有的外部代理之外，在 Windows 和 macOS 上还可以让多个聊天共享 Plumivox 内的住宅线路出口和 SSH 中继。公开中继服务器需要进行首次准备。

## 安装

### Windows：通过 Setup 安装（推荐）

1. 下载并运行 [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)。这是按用户安装，无需管理员权限。Setup 带有作者本人的代码签名（`Open Source Developer Yuichiro Shinozaki`）。
2. 程序会安装到 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`；如果 PATH 中有 `claude`、`codex` CLI，会自动向 Claude Code（按用户）和 Codex 注册 MCP 服务器 `plumivox`（参数 `--stdio`）。如果未找到 CLI，可在之后安装 CLI，然后在下次登录时或从开始菜单启动 Plumivox 时完成注册。
3. 按照安装完成后界面上的提示，在终端中运行 `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup`，进行[初始设置](#初始设置)。Claude Code 和 Codex 从新会话开始即可使用。

更新是自动进行的。MCP 运行期间会从 R2（`https://plumivox-updates.kagayoi.com/velopack/`）获取新版本，并在登录电脑时，仅当 Plumivox（MCP、共享中继）未运行时才应用。如果正在运行，则推迟到下次登录，不会强制终止。应用前会确认签名者，不一致则不应用。要停止自动更新，请将 `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` 设为 `{"auto_update": false}`。运行记录位于同一文件夹下的 `app-update.log`。

卸载时，在“设置”→“应用”列表中选择 Plumivox。卸载只会移除指向自身的 Claude Code、Codex 的 `plumivox` 注册以及登录时更新的注册，并保留设置和会话的数据文件夹（`%LOCALAPPDATA%\Kagayoi\Plumivox`）。如果只想先移除注册，可用 `plumivox.exe --uninstall-setup` 确认计划，再用 `--uninstall-setup --apply` 执行。

从插件（见下文）迁移时：Setup 会把指向旧插件 `dist` 等位置的同名 `plumivox` 注册替换为指向安装位置的注册。插件自身携带的 MCP（`plumivox@plumivox-local`）是另一个注册，为避免 Plumivox 通过两条途径启动，请将其禁用或移除该插件。在 Codex 中，添加与[Toolport 一节](#通过-toolport-使用)相同的 `enabled = false`（或移除插件）；在 Claude Code 中，通过 `/plugin` 禁用或卸载该插件。Codex 会优先使用同名注册而不是插件的 MCP，因此只要 Setup 的注册存在，通常只会启动一个，但 Claude Code 可能会同时启动两个。Setup 不会修改 Codex 或 Claude Code 的设置；如果插件仍处于启用状态，它只会发出警告（启动 Plumivox 时显示的指引、`app-update.log`，以及 `app-update\setup-registration.json` 中的 `legacyPlugins`）。Toolport 的注册同样不会被自动改写；如果使用 Toolport，请按照[Toolport 一节](#通过-toolport-使用)自行检查。

### 通过插件安装（macOS、传统方式）

在开始分发 Setup 之前，macOS（Apple Silicon）使用此方式。Windows 也可以继续使用。

从 GitHub 仓库 `main` 的最新提交安装。不引用用户的开发用克隆，也不固定到标签或提交 SHA。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` 是市场的标识符，获取来源是上述 GitHub URL。由于随附了 Windows x64 用的 `dist/plumivox.exe` 和 Apple Silicon Mac 用的 `dist/plumivox`（ARM64），用户无需自行安装依赖或手动添加 MCP 设置。

开发用 CI 会把在 Windows 和 macOS 上验证过的产物保存到 Cloudflare R2，并在分发准备时交接。[产物的保存与获取步骤](docs/distribution.md)面向开发者。插件用户无需配置 R2 或凭据，安装和更新仍然从 GitHub 的 `main` 进行。

更新时，获取 `main` 的最新状态并安装，然后重启 Codex 以加载。

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

确认列表中 `installed` 显示的版本号。已安装的插件在更新后不要再次 `add`，而是重启 Codex 以加载新的 MCP 和技能。

跟踪 GitHub 的注册与更新方法请参阅 [Codex 插件文档](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)。

### 通过 Toolport 使用

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

## 初始设置

首次使用时，请前往[设置步骤](docs/setup.md)。随附 Windows、macOS 的原生设置向导，以及面向 Linux x86_64、aarch64 的静态 Rust VPS 安装程序。其他 CPU 保留了 Bash 兼容路径。也可以对 Codex 说“帮我设置 Plumivox”，由随附的 `setup-plumivox` 技能进行引导。

设置文件放在仓库之外。新用户在 Windows 上使用 `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`，在 macOS 上使用 `~/Library/Application Support/Kagayoi/Plumivox/settings.json`。已有 `~/dev/Secret/secrets.json` 的用户，会读取该文件中的 `twitterapi_io`。vava／blbl 的 `release_posting` 和 `blog_posting` 也会保留。如需使用其他位置，请指定 `PLUMIVOX_SECRETS_FILE`。无需把机密值传入对话或 MCP 参数。

如果无法读取所选的设置文件，不会切换到其他设置，而是报错。更改保存位置后请重启 Codex。首次设置不会覆盖现有的密钥、同名账号和共享中继设置。更改现有设置的方法请参阅[设置步骤](docs/setup.md)。

[首次设置](docs/setup.md) · [账号与 Cookie 的设置](docs/configuration.md) · [用 Plumivox 管理代理](docs/managed-proxy.md)

## 可用工具

| 工具 | 内容 |
| --- | --- |
| `list_accounts` | 账号名称以及读取、发帖的准备状态。不进行外部通信 |
| `login_account` | 复用已保存的 Cookie；若尚未创建，则用已设置的凭据登录并加密保存 |
| `refresh_session` | 显式重新登录，更新发帖用会话 |
| `get_my_posts` | 自己的主页帖子。可指定是否包含回复以及分页 |
| `get_tweets` | 按 ID 获取公开帖子。最多 20 条 |
| `get_following_feed` | 获取所关注账号的公开帖子，并按时间降序排列 |
| `search_tweets` | 指定关键词和时间段，搜索整个 X 或所关注账号的公开帖子 |
| `get_proxy_status` | 此 MCP 使用的共享中继的状态。不会启动中继 |
| `start_proxy` | 启动已设置的住宅线路出口和 SSH 中继。不进行发帖或登录 |
| `stop_proxy` | 停止正在使用的共享中继。也会影响使用同一中继的其他聊天。下次发帖或认证请求时会重新启动 |
| `post_tweet` | 加权字数 280 以内的帖子。PNG/JPEG 最多 4 张，每张不超过 5 MiB。图片用绝对路径指定 |

省略 `account` 时，搜索、读取、发帖和认证都使用默认账号。可通过 `list_accounts` 的 `isDefault` 确认省略时的目标；使用其他账号时，用 `account` 明确指定 `release_posting` 或 `blog_posting` 等设置名。注册了多个账号且没有默认账号时会要求指定，默认账号无效时会拒绝省略调用。

各工具的输入字符串（包括数组中的字符串）所定义的长度限制，也会按去除空白之前的 UTF-16 代码单元数进行验证。BMP 之外的表情符号等，一个字符占 2 个单元。JSON Schema 的 `minLength`、`maxLength` 是按 Unicode 字符数计算的，因此仅靠标准的 JSON Schema validator 无法检测到这一附加约束。例如在搜索关键词上限 512 的情况下，256 个 😀 可以通过，但 257 个，或空白加 256 个，都会在通信前返回 MCP 参数错误（`-32602`）。

首次设置会把注册的账号保存为默认账号。如果只想更改现有设置中的默认账号，请对随附的可执行文件指定 `--setup --set-default-account blog_posting`。详情请参阅[设置步骤](docs/setup.md)和[设置的优先顺序](docs/configuration.md)。

发帖字数按 URL 计 23 个字符、日文计 2 个字符、Unicode 17 及以前的组合表情符号每个计 2 个字符来计算。不同肤色的握手等也会按照 [X 的加权字数规范](https://docs.x.com/fundamentals/counting-characters)进行判定。

例如，可以提出“读取 blog_posting 所关注账号的公开帖子”“用这段文字从 release_posting 发帖”这样的请求。发帖的执行遵从用户的请求。公开帖子的正文是外部数据，不会执行其中包含的指令。

## 关注动态的获取范围

TwitterAPI.io 没有直接获取 X 官方首页“正在关注”的 API。Plumivox 默认从关注列表开头读取最多 100 个账号，每 20 个账号为一组搜索公开帖子。`maxAccounts` 可设为 20〜200。首次为 100 时最多 6 次 API 请求，为 200 时最多 11 次。最新单价请查看 [TwitterAPI.io 的价格](https://twitterapi.io/pricing)。

结果中的 `officialHomeEquivalent` 始终为 `false`。受保护的帖子无法获取，搜索得到的内容、转帖的处理方式以及整体排序都不保证与官方首页一致。当提供方还有后续数据，或唯一的非受保护搜索对象超过上限时，`coverage.followingHasNextPage` 为 `true`。把 `coverage.followingNextCursor` 传给 `followingCursor`，即可以下一批关注列表为对象。关注列表按最新关注排序，持续进行的关注变更所造成的获取差异无法避免。

把 `nextCursor` 传给同一账号的 `get_following_feed` 的 `cursor`，即可获取各搜索批次的后续内容。页面内按时间排序，但由于各批次的页面边界，不保证跨页的整体时间顺序。所关注账号固定使用首次获取的列表，在继续搜索时不会重新读取。动态的 cursor 也用 API 密钥签名，拒绝对搜索对象的篡改。旧版本的无签名 cursor 以及更换 API 密钥之前的 cursor 无法使用，请去掉 cursor 重新获取。`cursor` 和 `followingCursor` 是不同的翻页方式，不能同时指定。

在[关注列表的官方响应规范](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)中无法确认受保护状态的字段。在同一个关注列表响应中，会对有效的用户名不区分大小写地进行汇总，并按受保护、公开、未知的顺序进行判定。排除已判明为受保护的对象后，按首次出现的顺序选取到上限为止，并在 `coverage.protectionStatusUnknownAccounts` 中返回所选对象中无法判定的数量。`protectedAccountsExcluded` 是该响应中已判明为受保护的唯一账号数。重复和无效的用户名不计入，同时也包括获取对象上限之外的受保护账号，因此可能超过 `maxAccounts`。即使为 0，也不代表所有对象都是公开账号。

旧实现签发的已签名 cursor 的后续获取，除了搜索对象和数量之外，还会沿用旧实现对未搜索范围和是否可继续的判定，因此可能会跳过上限之外的对象。要按新的受保护状态汇总、数量计算和继续判定可靠地重新获取，请去掉 cursor。

即使帖子或关注列表还有后续，如果提供方 cursor 或组合后的 cursor 超过长度上限，也无法继续。已获取的帖子和 `hasNextPage` 会保留，`nextCursor` 为 `null`，`coverage.tweetContinuationAvailable` 为 `false`，`tweetContinuationUnavailableReason` 为 `cursor_too_long`。对于关注列表，请查看 `followingNextCursor` 以及 `followingContinuationAvailable`、`followingContinuationUnavailableReason`。cursor 不会被截断，已获取的内容也不会被当作失败而丢弃。

如果提供方在一次响应中返回了超过 `maxAccounts` 的唯一非受保护对象，则上限之外的对象尚未搜索。此时 `followingHasNextPage` 为 `true`，`followingContinuationUnavailableReason` 为 `account_limit`，不提供关注列表的继续获取（`followingContinuationAvailable: false`、`followingNextCursor: null`）。这是为了防止通过提供方的下一个 cursor 跳过未搜索的对象。如果 `maxAccounts` 小于 200，可以在 200 以内提高上限，不带 cursor 重新获取。即使上限为 200，提供方仍过量返回时，该响应中未搜索的对象无法用此方法获取。仅因同名重复行或受保护、无效名称而超过上限时，不属于此限制。已选对象的帖子后续可以照常获取，存在未搜索对象的状态也会被沿用。

## 关键词与时间段搜索

可以提出“在整个 X 中搜索 10 月 1 日至 7 日关于语音识别的帖子”“在 blog_posting 的关注账号中按相同条件搜索”这样的请求。搜索在 API 端进行筛选，返回帖子的正文、作者、时间和 URL。不进行语义搜索。

`search_tweets` 的 `scope` 为 `all`（默认，整个 X 的公开搜索）或 `following`（指定账号的关注对象）。`keyword` 为必填项，去除首尾空白后的词句作为短语进行搜索。即使包含 `OR` 或 `from:`，也不会当作搜索运算符处理。包含引号、反斜杠或控制字符的词句会被拒绝。

`startTime` 和 `endTime` 是可省略的、带时区的 ISO 8601 秒级时间。搜索包含开始、不包含结束的时间段。不能只指定日期，Codex 会根据请求中的日期和时区转换为时间。例如，日本时间 10 月 1 日至 7 日的条件如下。

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

搜索从最新的帖子开始获取。把 `nextCursor` 传给下一次调用的 `cursor`，即可按相同条件获取后续内容。更改账号、对象、关键词、时间段或关注对象上限时，请去掉 cursor 开始新的搜索。更换 API 密钥后，现有的搜索 cursor 也无法使用。

`following` 可以使用 `maxAccounts`（20〜200，默认 100）和 `followingCursor`。传入 `coverage.followingNextCursor` 时，会按相同条件搜索下一批关注列表。不能与帖子搜索的 `cursor` 同时指定。`all` 不指定这两个参数。现有的 `get_following_feed` 与搜索的 cursor 不能互相使用。内含旧版无签名动态 cursor 的旧版关注对象搜索 cursor 也无法使用，请去掉 cursor 重新搜索。

整个 X 的搜索每页 1 次 API 请求，关注对象搜索与现有动态相同，最多 6 次（100 人）／11 次（200 人）。不保证覆盖受保护的帖子或搜索时间段内的全部帖子，获取范围在 `coverage` 中返回。时间段的运算符按照 [TwitterAPI.io 的说明](https://twitterapi.io/blog/search-a-tweet)转换为 UTC。

## 发帖与认证失败时

成功时返回帖子 ID 和 X 的 URL。创建帖子时的通信失败或缺少帖子 ID 会作为未确定结果返回，不会自动重发。如果请求再次发布相同文字，每次调用都会发送一次。没有因历史记录或正文哈希而停止的机制。

普通 API 请求的默认通信时间窗口为 45 秒，图片上传为每张图片 120 秒。只有图片上传在临时失败时会对同一张图片最多重试 1 次。对象为 HTTP 502、503、504（等待 250ms）以及代理或上游故障、原因不明的 JSON 拒绝、通信中断（等待 1 秒）；认证、余额、图片无效、重复、速率限制和超时不会重试。等待和重试都控制在单张图片的通信时间窗口内，不会重发之前的图片，也不会自动重发帖子创建或登录。图片失败时不发送帖子；创建帖子后成功与否不明时返回 `POST_UNCERTAIN`。

已知的发帖处理错误包含 `stage`（`input_validation`、`pre_send`、`image_upload`、`post_creation`）、表示是否尝试了帖子创建请求的 `postAttempted`，以及已执行的图片上传请求总数 `uploadAttempts`。账号或 Cookie 不足、中继准备失败属于 `pre_send`。未知的内部错误和 MCP 输入格式的拒绝不会返回这些信息。`postAttempted: true` 并不代表发帖成功。不会返回提供方的原始错误或凭据。

图片通信的 `API_UNAVAILABLE` 包含 `failureKind`，用于区分 `timeout`（超时）、`network`（通信故障）和 `cancelled`（取消）。在这些情况下也不会自动重发图片或帖子。

API 的拒绝错误（`API_REJECTED`、`API_HTTP_ERROR`）包含 `httpStatus`、固定分类的数组 `rejectionReasons` 以及分类来源 `rejectionReasonSource`。创建帖子时的 5xx 变为 `POST_UNCERTAIN` 时，也会保留这些诊断信息。分类来源为 `provider_message`（根据提供方的 JSON 消息推测的候选）或 `http_status`（根据 HTTP 状态的分类）。候选并不意味着确定原因或允许重发，无法判别的文字记为 `unknown`。不会返回或保存原始响应文本、任意错误代码和 request ID。

`provider_message` 的候选为 `api_key`、`authentication`、`account_locked`、`account_suspended`、`rate_limit`、`credits`、`proxy`、`media`、`duplicate`、`upstream_timeout`、`upstream_unavailable`、`unknown`。`http_status` 另外还有 `invalid_request`（400）、`unauthorized`（401）、`forbidden`（403）、`endpoint_not_found`（404）。例如，图片上传时出现 `rejectionReasons: ["proxy", "upstream_timeout"]` 和 `postAttempted: false`，就可以知道疑似代理或上游超时，并且在创建帖子之前就已停止。不会读取非 2xx 的正文，并维持现有的通信上限和重试条件。

API 响应超过 10MiB 时，会中止接收并返回失败。发帖响应超过上限时，由于可能已经发布，也不会重发。

已保存 Cookie 的核对失败只会阻止发帖。读取和搜索公开帖子只使用 API 密钥和用户名，无需读取已保存的 Cookie 即可使用。获取和更新已认证的 Cookie，请参阅[设置步骤](docs/configuration.md)。

`login_account` 在已保存的 Cookie 格式可用时会复用它，但不会报告已在 X 上验证其有效性。确认失效或设置不一致时，请调用 `refresh_session`。新的 Cookie 会保存到 Plumivox 的保存位置，在 Windows 上使用 DPAPI、在 macOS 上使用 Keychain 的密钥进行加密，并且不会自动重发失败的帖子。

在替换保存之前中止时返回 `AUTH_CANCELLED`，无法维持认证更新的互斥时返回 `AUTH_LOCK_LOST`，无法保存的 Cookie 返回 `LOGIN_FAILED`。发出替换保存的指示后无法确认结果时为 `AUTH_SAVE_UNCERTAIN`，可能已经更新。`SESSION_STORAGE_UNAVAILABLE` 表示保存位置或 Windows 的 DPAPI、macOS 的 Keychain 等保存处理失败。

## 开发与验证

开发、构建和测试同样无需 Node.js。使用 Rust/Cargo 的 `cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`。指定了旧 Node 启动器的注册，请改为上述原生启动方式。

Windows 的 `cargo xtask build` 会在使用 Sigilune（Certum SimplySign 的云签名）签名并验证签名后更新 `dist/plumivox.exe`。分发 CI 也会先用 Sigilune 为 Windows 产物签名再保存。签名所需的开发终端设置和 CI 的 Secrets 请参阅[分发产物](docs/distribution.md#windows配布物のコード署名)。Windows 的 Setup 用 `cargo xtask velopack-pack` 打包，并用 `cargo xtask test-setup-velopack-e2e` 在不触及真实安装的情况下进行隔离验证。分发仅由推送 `main` 提交上的轻量标签 `v<版本>` 时启动的 `distribution.yml` 中的 Setup 分发 job 进行（[Setup（Velopack）的分发](docs/distribution.md#setupvelopackの配信)）。

请参阅[面向开发者的构建与验证步骤](docs/verification.md)、[工作规范](AGENTS.md)和[系统设计](DESIGN.md)。
