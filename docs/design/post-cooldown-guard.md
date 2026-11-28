# 投稿の冷却ガード

[security.md](security.md#投稿の冷却ガード) の詳細。同じXアカウントへ別々のセッションから短い間隔で投稿すると、Xに自動化を疑われて拒否され、拒否直後の再送はさらに悪化させる。`src/post_guard.rs`はXのユーザー名ごとに全プロセス共通の冷却をかけ、冷却中の`post_tweet`はXへ何も送らず（課金も起こさず）`POST_COOLDOWN`で断る。値は固定の定数で、設定項目は無い。

## 順序と保存

- **順序**：`get_account`→`prepare_post`（検証）→`acquire`→`ensure_proxy`→`send_recorded`→`finish`。`ensure_proxy`の失敗は記録せず`release`で放す。
- **保存**：`<状態ルート>/post-guard/<hash>.json`と同じフォルダーの`<hash>.lock`。`hash`は`post-guard\n`＋小文字のXユーザー名（設定名ではない）のSHA-256。本人限定・リンク拒否・64KiBまで。一時ファイル（`.post-guard-<hash>-<乱数>.tmp`）から置換し、途中で残った自分の分（同じ`hash`の通常ファイルだけ）はロック取得後に片付ける。排他は約500ms待ち、取れなければ`post_in_progress`（1分）で断る。
- **状態に入れるもの**：schema、設定名（Xのユーザー名と一致するか含むだけなら`<user>`。4文字未満は完全一致のときだけ）、更新時刻、最後の試行時刻、送信中印（`inFlight`）、冷却（理由・開始・期限）、strikesと最後のstrike時刻、履歴（最大32件。時刻・段階・エラーコード・分類・`xErrorCode`・`xHttpStatus`の短い固定語）。本文・提供元の文面・ユーザー名・資格情報は入れない。

## 規則（時間は分）

| 結果 | 冷却の理由 | 長さ |
| --- | --- | --- |
| 成功・投稿作成まで進んだ失敗（`postAttempted`が真の`proxy`拒否を含む） | `min_interval` | 5（成功でstrikesを0に。strikeなし） |
| `POST_UNCERTAIN`・`post_id_missing` | `previous_uncertain` | 15 |
| 前回が`finish`なしで終わった（`inFlight`が残る） | `interrupted` | 15 |
| 状態が読めない・形式不正・解釈できないschema・上限超過で作り直した | `guard_state_reset` | 15 |
| この版より新しいschema | `guard_state_reset` | 15（書き換えず拒否だけ返す） |
| 分類できないXの拒否（`xRejected`） | `unclassified_rejection` | 15 |
| `rate_limit`でXの拒否（`xRejected`が真） | `rate_limit` | 15×2^(strikes-1) |
| `rate_limit`で`xRejected`が無い（提供元のHTTP 429など） | `rate_limit` | 15（strikeなし） |
| `automation_suspected` | `automation_suspected` | 180×2^(strikes-1) |
| `daily_limit` | `daily_limit` | 360×2^(strikes-1) |
| `account_locked`・`account_suspended` | `account_locked` | 1440×2^(strikes-1) |

複数に当たれば最長を採り、既存の冷却のほうが長ければ残す。上限は24時間。strikesは数える拒否ごとに1増え（上限64）、成功か最後のstrikeから48時間で0に戻る。Xまで届いていない拒否（HTTP 401・402、`api_key`・`credits`だけ、画像アップロード段階の`proxy`）と、投稿前の失敗・取消は冷却も記録もしない。

## 時計・停止・解除

- **時計**：保存時刻が120秒以上未来なら時計が戻ったとみなし、冷却を今から元の長さ（上限24時間）で置き直す。最後の試行が未来なら最小間隔の冷却をかける。
- **fail-closed**：状態ルートが無い、フォルダー・ロック・状態がリンクや通常ファイル以外・読めない、ユーザー名が空、送信前の書込み失敗では、送らず`POST_GUARD_UNAVAILABLE`で止める。冷却中の書込み失敗は無視する。
- **解除**：MCPからは解除できない。本人がCLI（`plumivox --post-guard status`、`clear [--account 設定名 | --user Xユーザー名]`）で行う。`<user>`の状態は`--account`で選べず、0件なら`--user`の案内を出す（終了コード0）。`clear`は端末での確認入力（対象名か`all`）が必須で、冷却・strikes・`inFlight`を消し、履歴と最後の試行は残す。送信中でロックが取れなければ解除しない。終了コードは成功0、保存先の異常・送信中1、引数の誤り・端末でない・確認の不一致2。

## 限界と設計判断

シェルを使えるエージェントはCLIや状態ファイルの削除で解除できるため、強い防御ではなく、短い間隔の投稿を誤って重ねないための仕組みである。AIが読む文言にCLIのコマンド名は書かない。共有daemonに置かないのは、daemonがmanagedの設定ごとにしかなく5分で終了して状態が消えるためで、ファイル＋ロックなら全プロセス・非managedでも共通にできる。冷却中に待って後から送らないのは自動再送と同じになるためで、待ち時間は`retryAfter`で返し、再開は本人に委ねる。
