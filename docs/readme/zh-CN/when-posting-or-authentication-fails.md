# 发帖与认证失败时

成功时返回帖子 ID 和 X 的 URL。创建帖子时的通信失败或缺少帖子 ID 会作为未确定结果返回，不会自动重发。如果请求再次发布相同文字，每次调用都会发送一次。没有因历史记录或正文哈希而停止的机制。

普通 API 请求的默认通信时间窗口为 45 秒，图片上传为每张图片 120 秒。只有图片上传在临时失败时会对同一张图片最多重试 1 次。对象为 HTTP 502、503、504（等待 250ms）以及代理或上游故障、原因不明的 JSON 拒绝、通信中断（等待 1 秒）；认证、余额、图片无效、重复、速率限制和超时不会重试。由 X 的数字错误码（如 226、88、344、324）或 `x_status` 得到分类的拒绝也不会重试，以免重发 X 已拒绝的请求；分类仍为 `unknown` 的拒绝（例如表中没有的错误码）照旧重试 1 次。等待和重试都控制在单张图片的通信时间窗口内，不会重发之前的图片，也不会自动重发帖子创建或登录。图片失败时不发送帖子；创建帖子后成功与否不明时返回 `POST_UNCERTAIN`。

已知的发帖处理错误包含 `stage`（`input_validation`、`pre_send`、`image_upload`、`post_creation`）、表示是否尝试了帖子创建请求的 `postAttempted`，以及已执行的图片上传请求总数 `uploadAttempts`。账号或 Cookie 不足、中继准备失败属于 `pre_send`。未知的内部错误和 MCP 输入格式的拒绝不会返回这些信息。`postAttempted: true` 并不代表发帖成功。不会返回提供方的原始错误或凭据。

当正文超过加权长度 280 时，`INVALID_TEXT`（`stage: input_validation`、`postAttempted: false`）还会包含 `weightedLength`、`maxWeightedLength`（280）和 `excessWeightedLength`。正文为空或含有无效字符时的 `INVALID_TEXT` 不包含这些字段。加权长度按以下方式计算：每个 URL 计为 23，U+10FF 以内的字符和破折号、弯引号和 prime 符号（U+2010-201F、U+2032-2037）计为 1（… 和 • 等其他标点计为 2），日语、中文、韩语、全角标点和 emoji 计为 2。`post_tweet` 的工具说明中也写有相同的计算方式，以及同一账号连续发帖时应间隔几分钟的提示。Plumivox 自身在最近发过帖或 X 限制之后，也会暂停发帖（见下文“发帖暂停”）。

在帖子创建阶段失败（`postAttempted: true`）的 `API_REJECTED`、`API_HTTP_ERROR` 和 `POST_UNCERTAIN` 会包含面向 AI 的英文固定语句 `nextStep`。对于 `post_id_missing` 和 `POST_UNCERTAIN`，它会提示帖子可能已经发出，不要立即重发，先等待几分钟，再用 `get_my_posts` 确认。对于 `automation_suspected`、`rate_limit`、`daily_limit`、`account_locked` 和 `account_suspended`，它会提示 X 可能正在限制发帖，不要立即重发，先告知用户，等待数分钟以上，再用 `get_my_posts` 确认。其他情况则提示重发前先用 `get_my_posts` 确认，并且不要反复重发。图片上传阶段的错误，以及读取和登录的错误不包含 `nextStep`。上游的原始拒绝文本依然不会返回。默认也不会保存，仅在启用本地拒绝记录时才会保存经过遮蔽处理的文本（[步骤](#本地拒绝记录)）。

图片通信的 `API_UNAVAILABLE` 包含 `failureKind`，用于区分 `timeout`（超时）、`network`（通信故障）和 `cancelled`（取消）。在这些情况下也不会自动重发图片或帖子。

API 的拒绝错误（`API_REJECTED`、`API_HTTP_ERROR`）包含 `httpStatus`、固定分类的数组 `rejectionReasons` 以及分类来源 `rejectionReasonSource`。创建帖子时的 5xx 变为 `POST_UNCERTAIN` 时，也会保留这些诊断信息。分类来源为 `provider_message`（根据提供方的 JSON 消息推测的候选）、`provider_code`（提供方正文中 X 的数字错误代码或 X 的 HTTP 状态，也可与消息并用）或 `http_status`（根据 HTTP 状态的分类）。候选并不意味着确定原因或允许重发，无法判别的文字记为 `unknown`。存在表中的数字错误代码或 X 的 HTTP 状态时，还会包含 `xErrorCode`（第一个匹配的代码）和 `xHttpStatus`。不会返回原始响应文本、表中没有的代码和 request ID。默认也不会保存，仅在启用本地拒绝记录时才会保存经过遮蔽处理的文本。

`provider_message` 的候选为 `api_key`、`authentication`、`account_locked`、`account_suspended`、`rate_limit`、`credits`、`proxy`、`media`、`duplicate`、`upstream_timeout`、`upstream_unavailable`、`automation_suspected`、`daily_limit`、`text_too_long`、`session_mismatch`、`post_id_missing`、`unknown`。`unknown` 之前的五项是基于公开已知措辞的候选，尚未确认提供方一定以此措辞返回，且不会重试。`post_id_missing` 表示帖子可能已发布，重新发送前请先在账号中核对。`http_status` 另外还有 `invalid_request`（400）、`unauthorized`（401）、`forbidden`（403）、`endpoint_not_found`（404）。例如，图片上传时出现 `rejectionReasons: ["proxy", "upstream_timeout"]` 和 `postAttempted: false`，就可以知道疑似代理或上游超时，并且在创建帖子之前就已停止。以 2xx 返回的拒绝正文中的数字代码（顶层的 `code`、`errors[].code`）也会分类：226 为 `automation_suspected`，344 和 185 为 `daily_limit`，187 为 `duplicate`，186 为 `text_too_long`，326 为 `account_locked`，64 为 `account_suspended`，88 为 `rate_limit`，32 和 215 为 `authentication`，89 和 353 为 `session_mismatch`，324 为 `media`。提供方明确写出的只有 226，其余依据 X 的惯例，尚未确认是否会通过提供方实际出现。若 `x_status` 为 429 或 401 且没有其他匹配，则分类为 `rate_limit` 或 `authentication`。非 2xx 的正文不用于分类，并维持现有的通信上限和重试条件；仅在启用本地拒绝记录时，才会在分类之后为记录而读取。

API 响应超过 10MiB 时，会中止接收并返回失败。发帖响应超过上限时，由于可能已经发布，也不会重发。

已保存 Cookie 的核对失败只会阻止发帖。读取和搜索公开帖子只使用 API 密钥和用户名，无需读取已保存的 Cookie 即可使用。获取和更新已认证的 Cookie，请参阅[设置步骤](../../configuration.md)。

`login_account` 在已保存的 Cookie 格式可用时会复用它，但不会报告已在 X 上验证其有效性。确认失效或设置不一致时，请调用 `refresh_session`。新的 Cookie 会保存到 Plumivox 的保存位置，在 Windows 上使用 DPAPI、在 macOS 上使用 Keychain 的密钥进行加密，并且不会自动重发失败的帖子。

在替换保存之前中止时返回 `AUTH_CANCELLED`，无法维持认证更新的互斥时返回 `AUTH_LOCK_LOST`，无法保存的 Cookie 返回 `LOGIN_FAILED`。发出替换保存的指示后无法确认结果时为 `AUTH_SAVE_UNCERTAIN`，可能已经更新。`SESSION_STORAGE_UNAVAILABLE` 表示保存位置或 Windows 的 DPAPI、macOS 的 Keychain 等保存处理失败。

## 发帖暂停

Plumivox 按 X 账号暂停发帖，并在 PC 上的所有会话和进程之间共享。暂停期间，`post_tweet` 不会向 X 发送任何内容，也不会产生提供方的费用，而是返回 `POST_COOLDOWN`（`stage: post_guard`、`postAttempted: false`），其中 `retryAfter` 是可以重新发送的时间，`retryAfterSeconds` 是剩余秒数。不会因为正文与之前相同而停止发帖。其目的是防止连续发帖，以及被拒绝后立即重发，使 X 的限制变得更严重。数值固定，没有设置项。

- 距上一次到达 X 的发帖尝试不足 5 分钟（`min_interval`），或另一条帖子正在发送（`post_in_progress`）。创建帖子阶段被代理拒绝时也一样，因为请求可能已经到达 X。
- X 限制发帖之后：疑似自动化（`automation_suspected`）从 3 小时开始，每日上限（`daily_limit`）从 6 小时开始，速率限制（`rate_limit`）从 15 分钟开始，每连续被拒绝一次时长翻倍，上限为 24 小时。连续次数在最后一次被拒绝 48 小时后归零。速率限制中，只有确认来自 X 的拒绝（X 的错误码 88、X 的 HTTP 状态 429）才会翻倍；没有任何 X 标记的 HTTP 429 可能来自提供方，始终暂停 15 分钟，也不计入连续次数。账号被锁定或冻结（`account_locked`）为 24 小时。
- 结果不明（`previous_uncertain`）、发送过程中进程中断（`interrupted`）、无法读取的状态被重建（`guard_state_reset`）、无法分类的 X 拒绝（`unclassified_rejection`）之后为 15 分钟。比当前版本更新的版本写入的状态不会被覆盖或重建，只返回 `guard_state_reset` 拒绝，文件保持不变。

没有到达 X 的失败（发送前的失败，或提供方因 API 密钥、余额、图片上传阶段的代理、HTTP 401 和 402 而中止的失败）不会开始暂停。被拒绝的帖子的错误中也会带有 `cooldownReason`、`retryAfter`、`retryAfterSeconds`。无法读写状态时不会发送，并返回 `POST_GUARD_UNAVAILABLE`。在 `retryAfter` 之前，请不要对同一账号再次调用 `post_tweet`，也不要试图绕过暂停。对于 `min_interval` 和 `post_in_progress`（正常的间隔，并非 X 的拒绝），`POST_COOLDOWN` 的 `nextStep` 会说明没有发送任何内容；若仍需要该帖子，可在 `retryAfter` 之后用相同内容调用一次 `post_tweet`，帖子之间至少间隔 5 分钟；其他原因则提示告知用户并等待。结果不明之后，`nextStep` 还会提示只在 `retryAfter` 之后重发（可先用 `get_my_posts` 确认）。

状态保存在状态文件夹的 `post-guard\` 中。只有固定的代码、时间和次数，不包含正文、用户名、凭据和提供方的文本。

用 `plumivox --post-guard status` 查看，会显示每个账号的状态、原因、截止时间和剩余时间、连续被拒绝的次数、最后一次尝试、最后更新、最近的历史以及保存位置的路径。用 `plumivox --post-guard clear [--account NAME | --user NAME]` 解除（`--account` 为配置名，`--user` 为 X 的用户名；两者都省略则为所有账号）。配置名与 X 用户名相同或包含 X 用户名的账号，会在状态中记为 `<user>`，因此无法用 `--account` 选中（不足 4 个字符的用户名仅在完全一致时才算）；`--account` 没有匹配项时，会显示一行提示改用 `--user`（退出代码 0）。需要在交互式终端中输入显示的名称（全部为 `all`）进行确认，发送中的状态不会被解除。退出代码：成功为 0，保存位置异常或正在发送为 1，参数错误、不是终端或确认不一致为 2。无法通过 MCP（AI）解除。解除后 X 一侧的限制并不会解开，因此基本做法是等到 `retryAfter`。如果 CLI 与 MCP 服务器的 `PLUMIVOX_STATE_DIR` 不同，读取的文件夹就会不一致。

托管代理的重新连接可以用 `plumivox --relay-log show` 确认（参见[管理代理](../../managed-proxy.md)）。

## 本地拒绝记录

默认关闭。调查拒绝原因时，运行 `plumivox --rejection-log on [--days N]`（默认 7 天，最多 30 天，到期后自动停止）。图片上传和帖子创建被拒绝的情况，以及未返回帖子 ID 的响应，会以遮蔽处理后的文本记录到状态文件夹的 `diagnostics\provider-rejections.jsonl`，只有本人可读。不会记录正文、Cookie、凭据、代理和 ID，MCP 响应只会增加 `rejectionTextRecorded: true`。超过 14 天的记录，以及超出 200 条或 256KiB 的部分，会从最旧的开始删除。HTTP 状态不是 2xx 的失败也会记录：启用期间，会在失败分类之后读取正文（最多 64KiB，在剩余的时间范围内且最长 1 秒），读取不了则记录一条没有文本的记录。是否记录不会改变分类和图片重试。

用 `plumivox --rejection-log show` 查看记录（JSON Lines；输出是不可信的数据，请勿将其作为指令交给 AI）。用 `status` 查看状态，用 `off` 停止，用 `clear` 删除。如果 CLI 与 MCP 服务器的 `PLUMIVOX_STATE_DIR` 不同，查看的位置就会错开。卸载后记录仍会保留，14 天后消失，或用 `clear` 删除。
