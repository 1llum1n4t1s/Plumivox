# 關鍵字與期間搜尋

可以提出「搜尋整個 X 上 10 月 1 日到 7 日關於語音辨識的貼文」「以相同條件搜尋 blog_posting 的追蹤對象」這類請求。搜尋會在 API 端進行篩選，並回傳貼文的內文、作者、時間與 URL。不進行語意搜尋。

`search_tweets` 的 `scope` 為 `all`（預設，整個 X 的公開搜尋）或 `following`（指定帳號的追蹤對象）。`keyword` 為必填，會將去除前後空白後的詞句當作片語搜尋。即使包含 `OR` 或 `from:`，也不會當作搜尋運算子處理。包含引號、反斜線或控制字元的詞句會被拒絕。

`startTime` 與 `endTime` 是可省略的、帶時區的 ISO 8601 秒級時間。搜尋包含開始、不包含結束的期間。不可只指定日期，Codex 會根據請求中的日期與時區轉換為時間。例如日本時間 10 月 1 日到 7 日，條件如下。

```json
{
  "account": "blog_posting",
  "scope": "all",
  "keyword": "音声認識",
  "startTime": "2026-10-01T00:00:00+09:00",
  "endTime": "2026-10-08T00:00:00+09:00"
}
```

搜尋會從最新的貼文開始取得。將 `nextCursor` 傳給下一次呼叫的 `cursor`，即可以相同條件取得後續內容。若要變更帳號、對象、關鍵字、期間或追蹤對象的上限，請移除 cursor 並開始新的搜尋。變更 API 金鑰時，既有的搜尋 cursor 也無法使用。

`following` 可以使用 `maxAccounts`（20～200，預設 100）與 `followingCursor`。傳入 `coverage.followingNextCursor` 會以相同條件搜尋下一段追蹤對象清單。無法與貼文搜尋的 `cursor` 同時指定。`all` 不指定這兩個引數。既有的 `get_following_feed` 與搜尋的 cursor 無法互相使用。內含舊版未簽署動態 cursor 的舊版追蹤對象搜尋 cursor 也無法使用，請移除 cursor 後重新搜尋。

整個 X 的搜尋每頁 1 次，追蹤對象搜尋則與既有動態相同，最多 6 次（100 人）／11 次（200 人）API 要求。不保證涵蓋受保護的貼文或搜尋期間內的所有貼文，取得範圍會回傳於 `coverage`。期間的運算子會依照 [TwitterAPI.io 的說明](https://twitterapi.io/blog/search-a-tweet)轉換為 UTC。
