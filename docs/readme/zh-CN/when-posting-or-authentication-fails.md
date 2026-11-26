# 发帖与认证失败时

成功时返回帖子 ID 和 X 的 URL。创建帖子时的通信失败或缺少帖子 ID 会作为未确定结果返回，不会自动重发。如果请求再次发布相同文字，每次调用都会发送一次。没有因历史记录或正文哈希而停止的机制。

普通 API 请求的默认通信时间窗口为 45 秒，图片上传为每张图片 120 秒。只有图片上传在临时失败时会对同一张图片最多重试 1 次。对象为 HTTP 502、503、504（等待 250ms）以及代理或上游故障、原因不明的 JSON 拒绝、通信中断（等待 1 秒）；认证、余额、图片无效、重复、速率限制和超时不会重试。等待和重试都控制在单张图片的通信时间窗口内，不会重发之前的图片，也不会自动重发帖子创建或登录。图片失败时不发送帖子；创建帖子后成功与否不明时返回 `POST_UNCERTAIN`。

已知的发帖处理错误包含 `stage`（`input_validation`、`pre_send`、`image_upload`、`post_creation`）、表示是否尝试了帖子创建请求的 `postAttempted`，以及已执行的图片上传请求总数 `uploadAttempts`。账号或 Cookie 不足、中继准备失败属于 `pre_send`。未知的内部错误和 MCP 输入格式的拒绝不会返回这些信息。`postAttempted: true` 并不代表发帖成功。不会返回提供方的原始错误或凭据。

当正文超过加权长度 280 时，`INVALID_TEXT`（`stage: input_validation`、`postAttempted: false`）还会包含 `weightedLength`、`maxWeightedLength`（280）和 `excessWeightedLength`。正文为空或含有无效字符时的 `INVALID_TEXT` 不包含这些字段。加权长度按以下方式计算：每个 URL 计为 23，U+10FF 以内的字符和破折号、弯引号和 prime 符号（U+2010-201F、U+2032-2037）计为 1（… 和 • 等其他标点计为 2），日语、中文、韩语、全角标点和 emoji 计为 2。`post_tweet` 的工具说明中也写有相同的计算方式，以及同一账号连续发帖时应间隔几分钟的提示。Plumivox 不强制冷却时间。

在帖子创建阶段失败（`postAttempted: true`）的 `API_REJECTED`、`API_HTTP_ERROR` 和 `POST_UNCERTAIN` 会包含面向 AI 的英文固定语句 `nextStep`。对于 `post_id_missing` 和 `POST_UNCERTAIN`，它会提示帖子可能已经发出，不要立即重发，先等待几分钟，再用 `get_my_posts` 确认。对于 `automation_suspected`、`rate_limit` 和 `daily_limit`，它会提示 X 可能正在限制发帖，不要立即重发，先告知用户，等待数分钟以上，再用 `get_my_posts` 确认。其他情况则提示重发前先用 `get_my_posts` 确认，并且不要反复重发。图片上传阶段的错误，以及读取和登录的错误不包含 `nextStep`。上游的原始拒绝文本依然不会返回，也不会保存。

图片通信的 `API_UNAVAILABLE` 包含 `failureKind`，用于区分 `timeout`（超时）、`network`（通信故障）和 `cancelled`（取消）。在这些情况下也不会自动重发图片或帖子。

API 的拒绝错误（`API_REJECTED`、`API_HTTP_ERROR`）包含 `httpStatus`、固定分类的数组 `rejectionReasons` 以及分类来源 `rejectionReasonSource`。创建帖子时的 5xx 变为 `POST_UNCERTAIN` 时，也会保留这些诊断信息。分类来源为 `provider_message`（根据提供方的 JSON 消息推测的候选）或 `http_status`（根据 HTTP 状态的分类）。候选并不意味着确定原因或允许重发，无法判别的文字记为 `unknown`。不会返回或保存原始响应文本、任意错误代码和 request ID。

`provider_message` 的候选为 `api_key`、`authentication`、`account_locked`、`account_suspended`、`rate_limit`、`credits`、`proxy`、`media`、`duplicate`、`upstream_timeout`、`upstream_unavailable`、`automation_suspected`、`daily_limit`、`text_too_long`、`session_mismatch`、`post_id_missing`、`unknown`。`unknown` 之前的五项是基于公开已知措辞的候选，尚未确认提供方一定以此措辞返回，且不会重试。`post_id_missing` 表示帖子可能已发布，重新发送前请先在账号中核对。`http_status` 另外还有 `invalid_request`（400）、`unauthorized`（401）、`forbidden`（403）、`endpoint_not_found`（404）。例如，图片上传时出现 `rejectionReasons: ["proxy", "upstream_timeout"]` 和 `postAttempted: false`，就可以知道疑似代理或上游超时，并且在创建帖子之前就已停止。不会读取非 2xx 的正文，并维持现有的通信上限和重试条件。

API 响应超过 10MiB 时，会中止接收并返回失败。发帖响应超过上限时，由于可能已经发布，也不会重发。

已保存 Cookie 的核对失败只会阻止发帖。读取和搜索公开帖子只使用 API 密钥和用户名，无需读取已保存的 Cookie 即可使用。获取和更新已认证的 Cookie，请参阅[设置步骤](../../configuration.md)。

`login_account` 在已保存的 Cookie 格式可用时会复用它，但不会报告已在 X 上验证其有效性。确认失效或设置不一致时，请调用 `refresh_session`。新的 Cookie 会保存到 Plumivox 的保存位置，在 Windows 上使用 DPAPI、在 macOS 上使用 Keychain 的密钥进行加密，并且不会自动重发失败的帖子。

在替换保存之前中止时返回 `AUTH_CANCELLED`，无法维持认证更新的互斥时返回 `AUTH_LOCK_LOST`，无法保存的 Cookie 返回 `LOGIN_FAILED`。发出替换保存的指示后无法确认结果时为 `AUTH_SAVE_UNCERTAIN`，可能已经更新。`SESSION_STORAGE_UNAVAILABLE` 表示保存位置或 Windows 的 DPAPI、macOS 的 Keychain 等保存处理失败。
