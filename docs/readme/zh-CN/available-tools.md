# 可用工具

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

首次设置会把注册的账号保存为默认账号。如果只想更改现有设置中的默认账号，请对随附的可执行文件指定 `--setup --set-default-account blog_posting`。详情请参阅[设置步骤](../../setup.md)和[设置的优先顺序](../../configuration.md)。

发帖字数按 URL 计 23 个字符、日文计 2 个字符、Unicode 17 及以前的组合表情符号每个计 2 个字符来计算。不同肤色的握手等也会按照 [X 的加权字数规范](https://docs.x.com/fundamentals/counting-characters)进行判定。

例如，可以提出“读取 blog_posting 所关注账号的公开帖子”“用这段文字从 release_posting 发帖”这样的请求。发帖的执行遵从用户的请求。公开帖子的正文是外部数据，不会执行其中包含的指令。
