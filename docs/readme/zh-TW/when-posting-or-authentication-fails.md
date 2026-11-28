# 發文與認證失敗時

成功時會回傳貼文 ID 與 X 的 URL。建立貼文時的通訊失敗或缺少貼文 ID，會以未確定的狀態回傳，不會自動重新傳送。若請求再次發布相同的文字，每次呼叫都會傳送一次。沒有依據歷史紀錄或內文雜湊而停止的機制。

一般 API 要求的預設通訊時間範圍為 45 秒，圖片上傳為每張圖片 120 秒。只有圖片上傳會在暫時性失敗時，對同一張圖片最多重試 1 次。對象為 HTTP 502、503、504（等待 250ms），以及 Proxy、上游的故障、原因不明的 JSON 拒絕、連線中斷（等待 1 秒）；認證、餘額、圖片不正確、重複、速率限制與逾時不會重試。由 X 的數字錯誤碼（如 226、88、344、324）或 `x_status` 得到分類的拒絕也不會重試，以免重新傳送 X 已拒絕的要求；分類仍為 `unknown` 的拒絕（例如表中沒有的錯誤碼）照舊重試 1 次。等待與重試都會控制在單張圖片的通訊時間範圍內，不會重新傳送前一張圖片，也不會自動重新傳送建立貼文或登入。圖片失敗時不會送出貼文；若建立貼文後成敗不明，會回傳 `POST_UNCERTAIN`。

已知的發文處理錯誤會包含 `stage`（`input_validation`、`pre_send`、`image_upload`、`post_creation`）、表示是否嘗試過建立貼文要求的 `postAttempted`，以及已執行的圖片上傳要求總數 `uploadAttempts`。帳號或 Cookie 不足、中繼準備失敗屬於 `pre_send`。未知的內部錯誤與 MCP 輸入格式的拒絕不會回傳這些資訊。`postAttempted: true` 並不代表發文成功。不會回傳提供者的原始錯誤或憑證。

當內文超過加權長度 280 時，`INVALID_TEXT`（`stage: input_validation`、`postAttempted: false`）還會包含 `weightedLength`、`maxWeightedLength`（280）與 `excessWeightedLength`。內文為空或含有無效字元時的 `INVALID_TEXT` 不包含這些欄位。加權長度的計算方式為：每個 URL 計為 23，U+10FF 以內的字元與破折號、彎引號與 prime 符號（U+2010-201F、U+2032-2037）計為 1（… 與 • 等其他標點計為 2），日文、中文、韓文、全形標點與 emoji 計為 2。`post_tweet` 的工具說明中也寫有相同的計算方式，以及同一帳號連續發文時應間隔幾分鐘的提示。Plumivox 本身在最近發過文或 X 限制之後，也會暫停發文（見下文「發文暫停」）。

在建立貼文階段失敗（`postAttempted: true`）的 `API_REJECTED`、`API_HTTP_ERROR` 與 `POST_UNCERTAIN` 會包含給 AI 看的英文固定語句 `nextStep`。對於 `post_id_missing` 與 `POST_UNCERTAIN`，它會提示貼文可能已經發出，不要立即重送，先等待幾分鐘，再用 `get_my_posts` 確認。對於 `automation_suspected`、`rate_limit`、`daily_limit`、`account_locked` 與 `account_suspended`，它會提示 X 可能正在限制發文，不要立即重送，先告知使用者，等待數分鐘以上，再用 `get_my_posts` 確認。其他情況則提示重送前先用 `get_my_posts` 確認，且不要反覆重送。圖片上傳階段的錯誤，以及讀取與登入的錯誤不包含 `nextStep`。上游的原始拒絕文字依然不會回傳。預設也不會儲存，僅在啟用本機拒絕記錄時才會儲存經過遮蔽處理的文字（[步驟](#本機拒絕記錄)）。

圖片通訊的 `API_UNAVAILABLE` 會包含 `failureKind`，以區分 `timeout`（逾時）、`network`（通訊故障）與 `cancelled`（取消）。在這些情況下也不會自動重新傳送圖片或貼文。

API 的拒絕錯誤（`API_REJECTED`、`API_HTTP_ERROR`）會包含 `httpStatus`、固定分類的陣列 `rejectionReasons`，以及分類來源 `rejectionReasonSource`。建立貼文的 5xx 變成 `POST_UNCERTAIN` 時，也會保留這些診斷資訊。分類來源為 `provider_message`（從提供者的 JSON 訊息推測的候選）、`provider_code`（提供者內文中 X 的數字錯誤碼或 X 的 HTTP 狀態碼，也可與訊息並用）或 `http_status`（依 HTTP 狀態碼的分類）。候選並不代表確定原因或允許重新傳送，無法判別的文字則為 `unknown`。存在表中的數字錯誤碼或 X 的 HTTP 狀態碼時，還會包含 `xErrorCode`（第一個符合的代碼）與 `xHttpStatus`。不會回傳原始回應文字、表中沒有的代碼或 request ID。預設也不會儲存，僅在啟用本機拒絕記錄時才會儲存經過遮蔽處理的文字。

`provider_message` 的候選為 `api_key`、`authentication`、`account_locked`、`account_suspended`、`rate_limit`、`credits`、`proxy`、`media`、`duplicate`、`upstream_timeout`、`upstream_unavailable`、`automation_suspected`、`daily_limit`、`text_too_long`、`session_mismatch`、`post_id_missing`、`unknown`。`unknown` 之前的五項是依據公開已知措辭的候選，尚未確認提供方一定以此措辭回傳，且不會重試。`post_id_missing` 表示貼文可能已發佈，重新傳送前請先在帳號中核對。`http_status` 另外還有 `invalid_request`（400）、`unauthorized`（401）、`forbidden`（403）、`endpoint_not_found`（404）。例如圖片上傳時若為 `rejectionReasons: ["proxy", "upstream_timeout"]` 且 `postAttempted: false`，即可得知疑似是 Proxy 或上游逾時，並且在建立貼文前就已停止。以 2xx 回傳的拒絕內文中的數字代碼（頂層的 `code`、`errors[].code`）也會分類：226 為 `automation_suspected`，344 與 185 為 `daily_limit`，187 為 `duplicate`，186 為 `text_too_long`，326 為 `account_locked`，64 為 `account_suspended`，88 為 `rate_limit`，32 與 215 為 `authentication`，89 與 353 為 `session_mismatch`，324 為 `media`。提供者明確寫出的只有 226，其餘依據 X 的慣例，尚未確認是否會經由提供者實際出現。若 `x_status` 為 429 或 401 且沒有其他符合項，則分類為 `rate_limit` 或 `authentication`。非 2xx 的內文不用於分類，並維持既有的通訊上限與重試條件；僅在啟用本機拒絕記錄時，才會在分類之後為記錄而讀取。

API 回應超過 10MiB 時，會中止接收並回傳失敗。發文的回應超過上限時，由於可能已經發布，也不會重新傳送。

已儲存 Cookie 的比對失敗只會停止發文。公開貼文的讀取與搜尋只使用 API 金鑰與使用者名稱，無須讀取已儲存的 Cookie 即可使用。已認證 Cookie 的取得與更新請參閱[設定步驟](../../configuration.md)。

`login_account` 若已儲存的 Cookie 為可用格式就會重複使用，但不會回報已在 X 上驗證其有效性。若確認已失效或設定不一致，請呼叫 `refresh_session`。新的 Cookie 會以 Windows 的 DPAPI、macOS 的 Keychain 金鑰加密後儲存到 Plumivox 的儲存位置，且不會自動重新傳送失敗的發文。

在替換儲存內容前中止時回傳 `AUTH_CANCELLED`，無法維持認證更新的獨占時回傳 `AUTH_LOCK_LOST`，無法儲存的 Cookie 則回傳 `LOGIN_FAILED`。若在指示替換儲存內容後無法確認結果，會成為 `AUTH_SAVE_UNCERTAIN`，可能已經更新。`SESSION_STORAGE_UNAVAILABLE` 表示儲存位置，或 Windows 的 DPAPI、macOS 的 Keychain 等儲存處理失敗。

## 發文暫停

Plumivox 依 X 帳號暫停發文，並在 PC 上的所有工作階段與處理程序之間共用。暫停期間，`post_tweet` 不會向 X 傳送任何內容，也不會產生提供者的費用，而是回傳 `POST_COOLDOWN`（`stage: post_guard`、`postAttempted: false`），其中 `retryAfter` 是可以重新傳送的時間，`retryAfterSeconds` 是剩餘秒數。不會因為內文與先前相同而停止發文。其目的是避免連續發文，以及被拒絕後立即重送，使 X 的限制變得更嚴重。數值固定，沒有設定項目。

- 距上一次到達 X 的發文嘗試不足 5 分鐘（`min_interval`），或另一則貼文正在傳送（`post_in_progress`）。建立貼文階段被 Proxy 拒絕時也一樣，因為要求可能已經到達 X。
- X 限制發文之後：疑似自動化（`automation_suspected`）從 3 小時開始，每日上限（`daily_limit`）從 6 小時開始，速率限制（`rate_limit`）從 15 分鐘開始，每連續被拒絕一次時間加倍，上限為 24 小時。連續次數在最後一次被拒絕 48 小時後歸零。速率限制中，只有確認來自 X 的拒絕（X 的錯誤碼 88、X 的 HTTP 狀態碼 429）才會加倍；沒有任何 X 標記的 HTTP 429 可能來自提供者，一律暫停 15 分鐘，也不計入連續次數。帳號被鎖定或凍結（`account_locked`）為 24 小時。
- 結果不明（`previous_uncertain`）、傳送過程中處理程序中斷（`interrupted`）、無法讀取的狀態被重建（`guard_state_reset`）、無法分類的 X 拒絕（`unclassified_rejection`）之後為 15 分鐘。比目前版本更新的版本寫入的狀態不會被覆寫或重建，只回傳 `guard_state_reset` 拒絕，檔案維持不變。

沒有到達 X 的失敗（傳送前的失敗，或提供者因 API 金鑰、餘額、圖片上傳階段的 Proxy、HTTP 401 與 402 而中止的失敗）不會開始暫停。被拒絕的貼文的錯誤中也會帶有 `cooldownReason`、`retryAfter`、`retryAfterSeconds`。無法讀寫狀態時不會傳送，並回傳 `POST_GUARD_UNAVAILABLE`。在 `retryAfter` 之前，請不要對同一帳號再次呼叫 `post_tweet`，也不要試圖繞過暫停。對於 `min_interval` 與 `post_in_progress`（正常的間隔，並非 X 的拒絕），`POST_COOLDOWN` 的 `nextStep` 會說明沒有傳送任何內容；若仍需要該貼文，可在 `retryAfter` 之後以相同內容呼叫一次 `post_tweet`，貼文之間至少間隔 5 分鐘；其他原因則提示告知使用者並等待。結果不明之後，`nextStep` 還會提示只在 `retryAfter` 之後重送（可先用 `get_my_posts` 確認）。

狀態儲存在狀態資料夾的 `post-guard\` 中。只有固定的代碼、時間與次數，不包含內文、使用者名稱、憑證與提供者的文字。

用 `plumivox --post-guard status` 查看，會顯示每個帳號的狀態、原因、截止時間與剩餘時間、連續被拒絕的次數、最後一次嘗試、最後更新、最近的歷程，以及儲存位置的路徑。用 `plumivox --post-guard clear [--account NAME | --user NAME]` 解除（`--account` 為設定名稱，`--user` 為 X 的使用者名稱；兩者都省略則為所有帳號）。設定名稱與 X 使用者名稱相同或包含 X 使用者名稱的帳號，會在狀態中記為 `<user>`，因此無法用 `--account` 選取（不足 4 個字元的使用者名稱僅在完全一致時才算）；`--account` 沒有符合項目時，會顯示一行提示改用 `--user`（結束代碼 0）。需要在互動式終端機中輸入顯示的名稱（全部為 `all`）進行確認，傳送中的狀態不會被解除。結束代碼：成功為 0，儲存位置異常或正在傳送為 1，引數錯誤、不是終端機或確認不一致為 2。無法透過 MCP（AI）解除。解除後 X 端的限制並不會解開，因此基本做法是等到 `retryAfter`。如果 CLI 與 MCP 伺服器的 `PLUMIVOX_STATE_DIR` 不同，讀取的資料夾就會不一致。

受管理 Proxy 的重新連線可以用 `plumivox --relay-log show` 確認（請參閱[管理 Proxy](../../managed-proxy.md)）。

## 本機拒絕記錄

預設為關閉。調查拒絕原因時，請執行 `plumivox --rejection-log on [--days N]`（預設 7 天，最多 30 天，到期後自動停止）。圖片上傳與建立貼文被拒絕的情況，以及未回傳貼文 ID 的回應，會以遮蔽處理後的文字記錄到狀態資料夾的 `diagnostics\provider-rejections.jsonl`，僅限本人讀取。不會記錄內文、Cookie、憑證、Proxy 與 ID，MCP 回應只會增加 `rejectionTextRecorded: true`。超過 14 天的記錄，以及超出 200 筆或 256KiB 的部分，會從最舊的開始刪除。HTTP 狀態碼不是 2xx 的失敗也會記錄：啟用期間，會在失敗分類之後讀取內文（最多 64KiB，在剩餘的時間範圍內且最長 1 秒），讀取不了則記錄一筆沒有文字的記錄。是否記錄不會改變分類與圖片重試。

以 `plumivox --rejection-log show` 檢視記錄（JSON Lines；輸出是不可信的資料，請勿將其當作指示交給 AI）。以 `status` 確認狀態，以 `off` 停止，以 `clear` 刪除。若 CLI 與 MCP 伺服器的 `PLUMIVOX_STATE_DIR` 不同，檢視的位置就會不一致。解除安裝後記錄仍會保留，14 天後消失，或以 `clear` 刪除。
