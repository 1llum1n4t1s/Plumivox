# 發文與認證失敗時

成功時會回傳貼文 ID 與 X 的 URL。建立貼文時的通訊失敗或缺少貼文 ID，會以未確定的狀態回傳，不會自動重新傳送。若請求再次發布相同的文字，每次呼叫都會傳送一次。沒有依據歷史紀錄或內文雜湊而停止的機制。

一般 API 要求的預設通訊時間範圍為 45 秒，圖片上傳為每張圖片 120 秒。只有圖片上傳會在暫時性失敗時，對同一張圖片最多重試 1 次。對象為 HTTP 502、503、504（等待 250ms），以及 Proxy、上游的故障、原因不明的 JSON 拒絕、連線中斷（等待 1 秒）；認證、餘額、圖片不正確、重複、速率限制與逾時不會重試。等待與重試都會控制在單張圖片的通訊時間範圍內，不會重新傳送前一張圖片，也不會自動重新傳送建立貼文或登入。圖片失敗時不會送出貼文；若建立貼文後成敗不明，會回傳 `POST_UNCERTAIN`。

已知的發文處理錯誤會包含 `stage`（`input_validation`、`pre_send`、`image_upload`、`post_creation`）、表示是否嘗試過建立貼文要求的 `postAttempted`，以及已執行的圖片上傳要求總數 `uploadAttempts`。帳號或 Cookie 不足、中繼準備失敗屬於 `pre_send`。未知的內部錯誤與 MCP 輸入格式的拒絕不會回傳這些資訊。`postAttempted: true` 並不代表發文成功。不會回傳提供者的原始錯誤或憑證。

圖片通訊的 `API_UNAVAILABLE` 會包含 `failureKind`，以區分 `timeout`（逾時）、`network`（通訊故障）與 `cancelled`（取消）。在這些情況下也不會自動重新傳送圖片或貼文。

API 的拒絕錯誤（`API_REJECTED`、`API_HTTP_ERROR`）會包含 `httpStatus`、固定分類的陣列 `rejectionReasons`，以及分類來源 `rejectionReasonSource`。建立貼文的 5xx 變成 `POST_UNCERTAIN` 時，也會保留這些診斷資訊。分類來源為 `provider_message`（從提供者的 JSON 訊息推測的候選）或 `http_status`（依 HTTP 狀態碼的分類）。候選並不代表確定原因或允許重新傳送，無法判別的文字則為 `unknown`。不會回傳或儲存原始回應文字、任意錯誤碼或 request ID。

`provider_message` 的候選為 `api_key`、`authentication`、`account_locked`、`account_suspended`、`rate_limit`、`credits`、`proxy`、`media`、`duplicate`、`upstream_timeout`、`upstream_unavailable`、`automation_suspected`、`daily_limit`、`text_too_long`、`session_mismatch`、`post_id_missing`、`unknown`。`unknown` 之前的五項是依據公開已知措辭的候選，尚未確認提供方一定以此措辭回傳，且不會重試。`post_id_missing` 表示貼文可能已發佈，重新傳送前請先在帳號中核對。`http_status` 另外還有 `invalid_request`（400）、`unauthorized`（401）、`forbidden`（403）、`endpoint_not_found`（404）。例如圖片上傳時若為 `rejectionReasons: ["proxy", "upstream_timeout"]` 且 `postAttempted: false`，即可得知疑似是 Proxy 或上游逾時，並且在建立貼文前就已停止。不會讀取非 2xx 的內文，並維持既有的通訊上限與重試條件。

API 回應超過 10MiB 時，會中止接收並回傳失敗。發文的回應超過上限時，由於可能已經發布，也不會重新傳送。

已儲存 Cookie 的比對失敗只會停止發文。公開貼文的讀取與搜尋只使用 API 金鑰與使用者名稱，無須讀取已儲存的 Cookie 即可使用。已認證 Cookie 的取得與更新請參閱[設定步驟](../../configuration.md)。

`login_account` 若已儲存的 Cookie 為可用格式就會重複使用，但不會回報已在 X 上驗證其有效性。若確認已失效或設定不一致，請呼叫 `refresh_session`。新的 Cookie 會以 Windows 的 DPAPI、macOS 的 Keychain 金鑰加密後儲存到 Plumivox 的儲存位置，且不會自動重新傳送失敗的發文。

在替換儲存內容前中止時回傳 `AUTH_CANCELLED`，無法維持認證更新的獨占時回傳 `AUTH_LOCK_LOST`，無法儲存的 Cookie 則回傳 `LOGIN_FAILED`。若在指示替換儲存內容後無法確認結果，會成為 `AUTH_SAVE_UNCERTAIN`，可能已經更新。`SESSION_STORAGE_UNAVAILABLE` 表示儲存位置，或 Windows 的 DPAPI、macOS 的 Keychain 等儲存處理失敗。
