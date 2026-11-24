# 关键词与时间段搜索

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
