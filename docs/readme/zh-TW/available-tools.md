# 可用的工具

| 工具 | 內容 |
| --- | --- |
| `list_accounts` | 帳號名稱與讀取、發文的準備狀態。不進行外部通訊 |
| `login_account` | 重複使用已儲存的 Cookie，若尚未建立則以已設定的憑證登入並加密儲存 |
| `refresh_session` | 明確地重新登入，更新發文用的工作階段 |
| `get_my_posts` | 自己的個人檔案貼文。可指定是否包含回覆以及分頁 |
| `get_tweets` | 依 ID 取得公開貼文。最多 20 則 |
| `get_following_feed` | 取得追蹤對象的公開貼文，並依時間由新到舊排列 |
| `search_tweets` | 指定關鍵字與期間，搜尋整個 X 或追蹤對象的公開貼文 |
| `get_proxy_status` | 此 MCP 使用的共用中繼狀態。不會啟動中繼 |
| `start_proxy` | 啟動已設定的住宅網路出口與 SSH 中繼。不進行發文或登入 |
| `stop_proxy` | 停止使用中的共用中繼。也會影響使用同一中繼的其他聊天。下次發文或認證要求時會重新啟動 |
| `post_tweet` | 加權字數 280 以內的發文。PNG/JPEG 最多 4 張，每張 5 MiB 以下。圖片以絕對路徑指定 |

省略 `account` 時，搜尋、讀取、發文與認證會使用預設帳號。可以透過 `list_accounts` 的 `isDefault` 確認省略時的對象；若要使用其他帳號，請在 `account` 中明確指定 `release_posting` 或 `blog_posting` 等設定名稱。註冊多個帳號但沒有預設值時會要求指定，預設值無效時則會拒絕省略的呼叫。

各工具輸入字串（包含陣列中的字串）所定義的長度限制，也會以去除空白前的 UTF-16 程式碼單元數進行驗證。BMP 以外的表情符號等，1 個字元為 2 個單元。JSON Schema 的 `minLength`、`maxLength` 是 Unicode 字元數，因此僅靠標準的 JSON Schema validator 無法偵測這項額外限制。例如搜尋關鍵字的上限為 512 時，256 個 😀 可以通過，但 257 個，或空白加上 256 個，會在通訊前產生 MCP 引數錯誤（`-32602`）。

首次設定會將註冊的帳號儲存為預設值。若只要變更既有設定的預設值，請對隨附的執行檔指定 `--setup --set-default-account blog_posting`。詳細內容請參閱[設定步驟](../../setup.md)與[設定的優先順序](../../configuration.md)。

發文字數中，URL 計為 23 個字元，日文計為 2 個字元，Unicode 17 以前的複合表情符號每個計為 2 個字元。不同膚色的握手等，也會依照 [X 的加權字數規格](https://docs.x.com/fundamentals/counting-characters)進行判定。

例如可以提出「讀取 blog_posting 追蹤對象的公開貼文」「用 release_posting 以這段文字發文」這類請求。發文的執行依照使用者的請求進行。公開貼文的內文屬於外部資料，不會執行其中所含的指令。
