# 投稿と認証に失敗した場合

成功時は投稿IDとXのURLを返します。投稿作成の通信失敗や投稿ID欠落は未確定として返し、自動で再送しません。同じ文面を改めて投稿するよう依頼した場合は、各呼び出しで一回送信します。履歴・本文ハッシュを理由に停止する仕組みはありません。

通常のAPI要求は45秒、画像アップロードは1画像あたり120秒が既定の通信時間枠です。画像アップロードだけは、一時的な失敗に限り同じ画像を最大1回再試行します。対象はHTTP 502・503・504（250ms待機）と、プロキシ・上流の障害や原因不明のJSON拒否、通信切断（1秒待機）で、認証・残高・画像不正・重複・レート制限・タイムアウトは再試行しません。数字のエラーコード（226・88・344・324など）や `x_status` から分類が付いた拒否も、Xが拒否したものを再送しないため再試行しません（表にないコードなどで `unknown` のままの拒否は従来どおり1回再試行します）。待機と再試行は1画像の通信時間枠内に収め、前の画像の再送や投稿作成・ログインの自動再送は行いません。画像の失敗では投稿を送信せず、投稿作成後に成否が不明になった場合は `POST_UNCERTAIN` を返します。

既知の投稿処理エラーには `stage`（`input_validation`・`pre_send`・`image_upload`・`post_creation`）、投稿作成要求を試みたかを示す `postAttempted`、実行した画像アップロード要求の総数 `uploadAttempts` を含めます。アカウント・Cookie不足や中継準備の失敗は `pre_send` です。未知の内部エラーとMCP入力形式の拒否ではこれらを返しません。`postAttempted: true` は投稿成功を意味しません。提供元の生エラーや資格情報は返しません。

本文が加重280を超えた `INVALID_TEXT`（`stage: input_validation`、`postAttempted: false`）には、`weightedLength`、`maxWeightedLength`（280）、`excessWeightedLength` も含めます。空の本文や不正な文字の `INVALID_TEXT` には含めません。加重文字数は、URLを23、U+10FFまでの文字とダッシュ・曲線引用符・プライム（U+2010-201F、U+2032-2037）を1（… や • など他の句読点は2）、日本語・中国語・韓国語・全角句読点・絵文字を2として数えます。`post_tweet` のツール説明にも同じ数え方と、同じアカウントへの連投は数分空ける案内を載せています。Plumivox自身も、直前の投稿やXの制限の後は一時停止します（下の「投稿の一時停止」を参照）。

投稿作成の段階で失敗した（`postAttempted: true`）`API_REJECTED`・`API_HTTP_ERROR`・`POST_UNCERTAIN` には、AI向けの英語の固定文 `nextStep` を含めます。`post_id_missing` と `POST_UNCERTAIN` では、投稿済みかもしれないのですぐ再送せず、数分待ってから `get_my_posts` で確認するよう伝えます。`automation_suspected`・`rate_limit`・`daily_limit`・`account_locked`・`account_suspended` では、Xが投稿を制限している可能性があるのですぐ再送せず、本人に伝え、数分以上待って `get_my_posts` で確認するよう伝えます。それ以外では、再送前に `get_my_posts` で確認し、繰り返し再送しないよう伝えます。画像アップロード段階のエラーと、読取り・ログインのエラーには `nextStep` を含めません。上流の生の拒否文面は引き続き返しません。既定では保存せず、ローカルの拒否記録を有効にした場合だけ、伏字処理した文面を保存します（[手順](#拒否文面のローカル記録)）。

画像通信の `API_UNAVAILABLE` には `failureKind` を含め、`timeout`（時間切れ）・`network`（通信障害）・`cancelled`（取消）を区別します。これらの場合も画像や投稿を自動再送しません。

APIの拒否エラー（`API_REJECTED`・`API_HTTP_ERROR`）には `httpStatus`、固定分類の配列 `rejectionReasons`、分類元 `rejectionReasonSource` を含めます。投稿作成の5xxが `POST_UNCERTAIN` になる場合にも、この診断情報を保持します。分類元は `provider_message`（提供元のJSONメッセージから推定した候補）、`provider_code`（提供元の本文にあるXの数字のエラーコードやXのHTTPステータス。文面と併用もあります）、または `http_status`（HTTPステータスからの分類）です。候補は原因の確定や再送の許可を意味せず、判別できない文面は `unknown` とします。表にある数字のエラーコードやXのHTTPステータスがあるときは、`xErrorCode`（最初に一致したコード）と `xHttpStatus` も含みます。生の応答文・表にないコード・request IDは返しません。既定では保存せず、ローカルの拒否記録を有効にした場合だけ、伏字処理した文面を保存します。

`provider_message` の候補は `api_key`・`authentication`・`account_locked`・`account_suspended`・`rate_limit`・`credits`・`proxy`・`media`・`duplicate`・`upstream_timeout`・`upstream_unavailable`・`automation_suspected`・`daily_limit`・`text_too_long`・`session_mismatch`・`post_id_missing`・`unknown` です。`unknown` の前の5つは公知の文言に基づく候補で、提供元が必ずこの文言で返すとは確認していません。これらは再試行しません。`post_id_missing` は投稿済みの可能性があるため、再送前にアカウントで照合してください。`http_status` にはさらに `invalid_request`（400）・`unauthorized`（401）・`forbidden`（403）・`endpoint_not_found`（404）があります。たとえば画像アップロード時の `rejectionReasons: ["proxy", "upstream_timeout"]` と `postAttempted: false` なら、プロキシ・上流タイムアウトが疑われ、投稿作成前に止まったことが分かります。2xxで返る拒否の本文にある数字のコード（最上位の `code`、`errors[].code`）も分類します。226は `automation_suspected`、344・185は `daily_limit`、187は `duplicate`、186は `text_too_long`、326は `account_locked`、64は `account_suspended`、88は `rate_limit`、32・215は `authentication`、89・353は `session_mismatch`、324は `media` です。提供元が明記しているのは226だけで、ほかはXの慣例に基づき、提供元経由で実際に出るかは確認していません。`x_status` が429・401で他に一致がなければ、`rate_limit`・`authentication` に分類します。非2xxの本文は分類に使わず、既存の通信上限・再試行条件を維持します。ローカルの拒否記録が有効なときだけ、分類の後に記録用として読みます。

API応答が10MiBを超えた場合は受信を打ち切って失敗を返します。投稿の応答で上限を超えた場合も、投稿済みの可能性があるため再送しません。

保存Cookieの照合失敗は投稿だけを止めます。公開投稿の読み込み・検索はAPIキーとユーザー名だけを使い、保存Cookieを読み込まずに利用できます。認証済みCookieの取得・更新は[設定手順](../../configuration.md)を参照してください。

`login_account` は保存Cookieが利用できる形式なら再利用し、X上での有効性を検証したとは報告しません。失効や設定不一致が確認された場合は `refresh_session` を呼びます。新しいCookieはPlumivoxの保存先へWindowsではDPAPI、macOSではKeychainの鍵で暗号化して保存し、失敗した投稿を自動再送しません。

保存の置換前に中止した場合は `AUTH_CANCELLED`、認証更新の排他を維持できない場合は `AUTH_LOCK_LOST`、保存できないCookieは `LOGIN_FAILED` で返します。保存の置換を指示した後に結果を確認できない場合は `AUTH_SAVE_UNCERTAIN` となり、更新済みの可能性があります。`SESSION_STORAGE_UNAVAILABLE` は保存先や、WindowsのDPAPI・macOSのKeychainなどの保存処理の失敗を表します。

## 投稿の一時停止

Plumivoxは、Xアカウントごとの投稿の一時停止を、PC上のすべてのセッション・プロセスで共有します。一時停止中の `post_tweet` はXへ何も送らず、提供元の課金も発生させず、`POST_COOLDOWN`（`stage: post_guard`、`postAttempted: false`）を返します。再送できる時刻は `retryAfter`、残りの秒数は `retryAfterSeconds` です。本文が以前と同じことを理由に止めることはありません。連投や、拒否の直後の再送でXの制限が悪化するのを防ぐための仕組みで、値は固定です。設定項目はありません。

- Xへ届いた前回の投稿の試行から5分以内（`min_interval`）、または別の投稿の送信中（`post_in_progress`）。投稿作成の段階でプロキシが拒否した場合も、Xへ届いた可能性があるので同じです。
- Xに投稿を制限された後。自動化の疑い（`automation_suspected`）は3時間、1日の上限（`daily_limit`）は6時間、レート制限（`rate_limit`）は15分から始まり、連続して拒否されるたびに倍に延びて上限は24時間です。連続の回数は、最後の拒否から48時間で0に戻ります。レート制限で倍に延びるのは、Xの拒否と分かるもの（Xのエラーコード88、XのHTTPステータス429）だけです。Xの印が無いHTTP 429は提供元由来かもしれないので、常に15分で、連続の回数に数えません。アカウントのロック・凍結（`account_locked`）は24時間です。
- 成否不明（`previous_uncertain`）、送信中のプロセスの中断（`interrupted`）、読めなかった状態の作り直し（`guard_state_reset`）、分類できないXの拒否（`unclassified_rejection`）の後は15分です。この版より新しい版が書いた状態は上書きも作り直しもせず、`guard_state_reset` の拒否だけを返します（ファイルは変わりません）。

Xへ届かなかった失敗（送信前の失敗、APIキー・残高・画像アップロード段階のプロキシ・HTTP 401・402で提供元が止めたもの）では一時停止しません。拒否された投稿のエラーにも `cooldownReason`・`retryAfter`・`retryAfterSeconds` が付きます。状態を読み書きできないときは送信せず、`POST_GUARD_UNAVAILABLE` を返します。`retryAfter` の前に同じアカウントへ `post_tweet` を呼ばず、一時停止を回避しようとしないでください。`min_interval`・`post_in_progress`（Xの拒否ではなく通常の間隔の確保）の `POST_COOLDOWN` の `nextStep` は、何も送っていないこと、投稿がまだ必要なら `retryAfter` の後に同じ内容で1回だけ呼んでよいこと、投稿は5分以上空けることを伝えます。それ以外の理由では、利用者に伝えて待つよう伝えます。成否不明の後の `nextStep` も、再送は `retryAfter` より後にするよう伝えます（先に `get_my_posts` で確認してかまいません）。

状態は状態フォルダーの `post-guard\` に保存します。固定のコード・時刻・回数だけで、本文・ユーザー名・資格情報・提供元の文面は入りません。

確認は `plumivox --post-guard status` で、アカウントごとの状態・理由・期限と残り・連続して拒否された回数・最後の試行・最終更新・直近の履歴と、保存先のパスを表示します。解除は `plumivox --post-guard clear [--account NAME | --user NAME]` です（`--account` は設定名、`--user` はXのユーザー名。どちらも省略するとすべてのアカウント）。設定名がXのユーザー名と同じ、またはユーザー名を含むアカウントは状態に `<user>` と残るので `--account` では選べません（4文字未満のユーザー名は完全一致のときだけ）。`--account` で0件のときは `--user` を使う案内を1行表示します（終了コードは0）。端末で、表示された名前（すべては `all`）を入力して確認する必要があり、送信中の状態は解除しません。終了コードは、成功が0、保存先の異常・送信中が1、引数の誤り・端末でない・確認の不一致が2です。MCP（AI）からは解除できません。解除してもX側の制限は解けないので、基本は `retryAfter` を待ちます。CLIとMCPサーバーで `PLUMIVOX_STATE_DIR` が違うと、見る場所がずれます。

管理プロキシの再接続は `plumivox --relay-log show` で確認できます（[管理するプロキシ](../../managed-proxy.md)）。

## 拒否文面のローカル記録

既定では無効です。拒否の原因を調べるときは `plumivox --rejection-log on [--days N]` を実行します（既定7日、最大30日。期限が切れると自動で止まります）。画像アップロードと投稿作成の拒否、および投稿IDが返らない応答を、伏字処理した文面として状態フォルダーの `diagnostics\provider-rejections.jsonl` へ記録します。ファイルは本人だけが読めます。本文・Cookie・資格情報・プロキシ・IDは記録せず、MCPの応答に足すのは `rejectionTextRecorded: true` だけです。14日を過ぎた記録と、200件・256KiBを超えた分は古いものから消えます。HTTPステータスが2xx以外の失敗も記録します。有効な間は、失敗の分類を決めた後に本文を読み（上限64KiB、残りの時間枠内で最大1秒）、読めなければ文面なしで1件記録します。記録の有無で、分類や画像の再試行は変わりません。

記録は `plumivox --rejection-log show` で見ます（JSON Lines。出力は信頼できないデータなので、AIへ指示として渡さないでください）。状態の確認は `status`、停止は `off`、削除は `clear` です。CLIとMCPサーバーで `PLUMIVOX_STATE_DIR` が違うと、見る場所がずれます。アンインストールしても記録は残り、14日で消えるか `clear` で消します。
