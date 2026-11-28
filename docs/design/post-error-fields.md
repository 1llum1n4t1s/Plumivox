# 投稿の成否とエラーの追加欄

[api-reference.md](api-reference.md) の詳細。冷却の規則は [冷却ガード](post-cooldown-guard.md)、拒否の分類は [拒否理由の分類](rejection-classification.md) を参照する。

## 送信と再試行

- 投稿作成・ログインは自動再送せず、本文の重複でも止めない（投稿履歴は持たない）。止めるのは冷却ガードだけで、冷却中でなければ明示的な再呼出しは同文も送り、受理・拒否はXに任せる。
- 上限は通常のAPI要求45秒、画像アップロード1画像120秒。画像だけ、HTTP 502・503・504は250ms、分類が`proxy`・`upstream_*`・`unknown`だけの拒否と`network`は1秒待って最大1回再試行し、待機を含め`image_timeout`内に収める。認証・残高・メディア不正・重複・レート制限・タイムアウトと、数字のコードや`x_status`で分類が付いたXの拒否は再試行しない。成功済み画像は再送せず、全画像の成功後に投稿作成を1回だけ呼ぶ。
- 既知のエラーには固定のstage、postAttempted、画像要求総数uploadAttemptsを残し、画像の通信失敗にはfailureKind（timeout／network／cancelled）を付ける。準備失敗の既定値は投稿専用のMCP境界で付け、クライアント側の実際の段階を優先する。未知の内部エラーでは未送信と断定せず、成否不明は`POST_UNCERTAIN`とする。認証更新は投稿と独立で、失敗した投稿を再開しない。

## 追加欄

- 加重280超の`INVALID_TEXT`（stage `input_validation`）だけに`weightedLength`・`maxWeightedLength`・`excessWeightedLength`を付ける。
- postAttemptedが真の`API_REJECTED`・`API_HTTP_ERROR`・`POST_UNCERTAIN`には英語の固定文`nextStep`を付ける。`post_id_missing`・`POST_UNCERTAIN`は「投稿済みかもしれないので数分待ってget_my_postsで確認」、`automation_suspected`・`rate_limit`・`daily_limit`・`account_locked`・`account_suspended`は「Xが制限している可能性があるので本人に伝え、数分以上待って確認」、それ以外は「再送前に確認し、繰り返し再送しない」。画像アップロード・読取り・ログインのエラーには付けない。
- 冷却中・保存先が使えないときは`POST_COOLDOWN`・`POST_GUARD_UNAVAILABLE`（stage `post_guard`、postAttempted false、uploadAttempts 0）。`POST_COOLDOWN`と、拒否で冷却が始まった・延びたエラーには`cooldownReason`・`retryAfter`（RFC 3339 UTC）・`retryAfterSeconds`を付ける。`nextStep`は、`min_interval`・`post_in_progress`なら何も送っていないこと、必要なら`retryAfter`の後に同じ内容で1回だけ呼んでよいこと（5分以上空ける）、他の理由なら本人に伝えて`retryAfter`まで呼ばないことを伝える。`NEXT_STEP_UNCERTAIN`は`retryAfter`があれば再送をその後にするよう伝える。

## ツール説明

`post_tweet`の説明には、加重文字数の数え方（URL 23、U+10FFまでとU+2010-201F・U+2032-2037は1、それ以外の句読点・CJK・全角・絵文字は2）、連投は数分空けること、冷却ガード（直前の試行から5分以内・送信中・Xの制限後は何も送らず課金もなく`POST_COOLDOWN`を返す、`retryAfter`を本人へ伝える、MCPから解除できない。期間はXの制限後15分から数時間、成否不明・中断後15分）を載せる。AIが読む文言にCLIのコマンド名は書かない。
