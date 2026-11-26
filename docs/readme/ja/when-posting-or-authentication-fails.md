# 投稿と認証に失敗した場合

成功時は投稿IDとXのURLを返します。投稿作成の通信失敗や投稿ID欠落は未確定として返し、自動で再送しません。同じ文面を改めて投稿するよう依頼した場合は、各呼び出しで一回送信します。履歴・本文ハッシュを理由に停止する仕組みはありません。

通常のAPI要求は45秒、画像アップロードは1画像あたり120秒が既定の通信時間枠です。画像アップロードだけは、一時的な失敗に限り同じ画像を最大1回再試行します。対象はHTTP 502・503・504（250ms待機）と、プロキシ・上流の障害や原因不明のJSON拒否、通信切断（1秒待機）で、認証・残高・画像不正・重複・レート制限・タイムアウトは再試行しません。待機と再試行は1画像の通信時間枠内に収め、前の画像の再送や投稿作成・ログインの自動再送は行いません。画像の失敗では投稿を送信せず、投稿作成後に成否が不明になった場合は `POST_UNCERTAIN` を返します。

既知の投稿処理エラーには `stage`（`input_validation`・`pre_send`・`image_upload`・`post_creation`）、投稿作成要求を試みたかを示す `postAttempted`、実行した画像アップロード要求の総数 `uploadAttempts` を含めます。アカウント・Cookie不足や中継準備の失敗は `pre_send` です。未知の内部エラーとMCP入力形式の拒否ではこれらを返しません。`postAttempted: true` は投稿成功を意味しません。提供元の生エラーや資格情報は返しません。

本文が加重280を超えた `INVALID_TEXT`（`stage: input_validation`、`postAttempted: false`）には、`weightedLength`、`maxWeightedLength`（280）、`excessWeightedLength` も含めます。空の本文や不正な文字の `INVALID_TEXT` には含めません。加重文字数は、URLを23、U+10FFまでの文字とダッシュ・曲線引用符・プライム（U+2010-201F、U+2032-2037）を1（… や • など他の句読点は2）、日本語・中国語・韓国語・全角句読点・絵文字を2として数えます。`post_tweet` のツール説明にも同じ数え方と、同じアカウントへの連投は数分空ける案内を載せています。クールダウンは強制しません。

投稿作成の段階で失敗した（`postAttempted: true`）`API_REJECTED`・`API_HTTP_ERROR`・`POST_UNCERTAIN` には、AI向けの英語の固定文 `nextStep` を含めます。`post_id_missing` と `POST_UNCERTAIN` では、投稿済みかもしれないのですぐ再送せず、数分待ってから `get_my_posts` で確認するよう伝えます。`automation_suspected`・`rate_limit`・`daily_limit` では、Xが投稿を制限している可能性があるのですぐ再送せず、本人に伝え、数分以上待って `get_my_posts` で確認するよう伝えます。それ以外では、再送前に `get_my_posts` で確認し、繰り返し再送しないよう伝えます。画像アップロード段階のエラーと、読取り・ログインのエラーには `nextStep` を含めません。上流の生の拒否文面は引き続き返さず、保存もしません。

画像通信の `API_UNAVAILABLE` には `failureKind` を含め、`timeout`（時間切れ）・`network`（通信障害）・`cancelled`（取消）を区別します。これらの場合も画像や投稿を自動再送しません。

APIの拒否エラー（`API_REJECTED`・`API_HTTP_ERROR`）には `httpStatus`、固定分類の配列 `rejectionReasons`、分類元 `rejectionReasonSource` を含めます。投稿作成の5xxが `POST_UNCERTAIN` になる場合にも、この診断情報を保持します。分類元は `provider_message`（提供元のJSONメッセージから推定した候補）または `http_status`（HTTPステータスからの分類）です。候補は原因の確定や再送の許可を意味せず、判別できない文面は `unknown` とします。生の応答文・任意のエラーコード・request IDは返さず、保存もしません。

`provider_message` の候補は `api_key`・`authentication`・`account_locked`・`account_suspended`・`rate_limit`・`credits`・`proxy`・`media`・`duplicate`・`upstream_timeout`・`upstream_unavailable`・`automation_suspected`・`daily_limit`・`text_too_long`・`session_mismatch`・`post_id_missing`・`unknown` です。`unknown` の前の5つは公知の文言に基づく候補で、提供元が必ずこの文言で返すとは確認していません。これらは再試行しません。`post_id_missing` は投稿済みの可能性があるため、再送前にアカウントで照合してください。`http_status` にはさらに `invalid_request`（400）・`unauthorized`（401）・`forbidden`（403）・`endpoint_not_found`（404）があります。たとえば画像アップロード時の `rejectionReasons: ["proxy", "upstream_timeout"]` と `postAttempted: false` なら、プロキシ・上流タイムアウトが疑われ、投稿作成前に止まったことが分かります。非2xxの本文は読まず、既存の通信上限・再試行条件を維持します。

API応答が10MiBを超えた場合は受信を打ち切って失敗を返します。投稿の応答で上限を超えた場合も、投稿済みの可能性があるため再送しません。

保存Cookieの照合失敗は投稿だけを止めます。公開投稿の読み込み・検索はAPIキーとユーザー名だけを使い、保存Cookieを読み込まずに利用できます。認証済みCookieの取得・更新は[設定手順](../../configuration.md)を参照してください。

`login_account` は保存Cookieが利用できる形式なら再利用し、X上での有効性を検証したとは報告しません。失効や設定不一致が確認された場合は `refresh_session` を呼びます。新しいCookieはPlumivoxの保存先へWindowsではDPAPI、macOSではKeychainの鍵で暗号化して保存し、失敗した投稿を自動再送しません。

保存の置換前に中止した場合は `AUTH_CANCELLED`、認証更新の排他を維持できない場合は `AUTH_LOCK_LOST`、保存できないCookieは `LOGIN_FAILED` で返します。保存の置換を指示した後に結果を確認できない場合は `AUTH_SAVE_UNCERTAIN` となり、更新済みの可能性があります。`SESSION_STORAGE_UNAVAILABLE` は保存先や、WindowsのDPAPI・macOSのKeychainなどの保存処理の失敗を表します。
