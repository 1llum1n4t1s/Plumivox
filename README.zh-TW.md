[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

這是一個使用 TwitterAPI.io，提供 X 發文、關鍵字與期間搜尋，以及讀取追蹤對象公開貼文功能的本機 MCP 伺服器／Codex 外掛程式。

## 需求

- Windows x64 或 Apple Silicon Mac（macOS ARM64）。MCP 本體是以 Rust 打造的原生執行檔，不需要安裝 Node.js 或 Rust。
- Windows 與 macOS 的初始設定使用隨附的執行檔，不需要 PowerShell 7 (`pwsh`)。
- TwitterAPI.io 的 API 金鑰。使用 API 會產生該服務的按量計費。
- 發文需要目標帳號的 v2 登入 Cookie 與 Proxy。除了既有的外部 Proxy，在 Windows 與 macOS 上還可讓多個聊天共用 Plumivox 內的住宅網路出口與 SSH 中繼。公開中繼伺服器需要進行首次準備。

## 安裝

### Windows：使用 Setup 安裝（建議）

1. 下載並執行 [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)。這是以使用者為單位的安裝，不需要系統管理員權限。Setup 附有作者本人的程式碼簽章（`Open Source Developer Yuichiro Shinozaki`）。
2. 程式會安裝到 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`，若 PATH 上有 `claude`、`codex` 的 CLI，就會自動向 Claude Code（使用者層級）與 Codex 註冊 MCP 伺服器 `plumivox`（引數 `--stdio`）。若找不到 CLI，可在之後安裝 CLI，並於下次登入或從開始功能表啟動 Plumivox 時完成註冊。
3. 依照安裝完成後畫面上的說明，在終端機執行 `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup`，進行[初始設定](#初始設定)。Claude Code 與 Codex 可從新的工作階段開始使用。

更新會自動進行。MCP 執行期間會從 R2（`https://plumivox-updates.kagayoi.com/velopack/`）取得新版本，並在登入電腦時，僅於 Plumivox（MCP、共用中繼）未執行時套用。若正在執行，則延到下次登入，不會強制結束。套用前會確認簽署者，若不符則不套用。若要停用自動更新，請將 `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` 設為 `{"auto_update": false}`。運作紀錄位於同一資料夾的 `app-update.log`。

解除安裝時，請在「設定」→「應用程式」的清單中選擇 Plumivox。只會移除指向自身的 Claude Code 與 Codex `plumivox` 註冊，以及登入時更新的註冊，並保留設定與工作階段的資料夾（`%LOCALAPPDATA%\Kagayoi\Plumivox`）。若要先只移除註冊，請以 `plumivox.exe --uninstall-setup` 確認預定內容，再以 `--uninstall-setup --apply` 執行。

從外掛程式（如下）轉移時：Setup 會將指向舊外掛程式 `dist` 等位置的同名 `plumivox` 註冊，替換為指向安裝位置的註冊。外掛程式本身所帶的 MCP（`plumivox@plumivox-local`）是另一個註冊，為避免 Plumivox 經由兩條途徑啟動，請將其停用或移除該外掛程式。在 Codex 中，加入與[Toolport 一節](#透過-toolport-使用)相同的 `enabled = false`（或移除外掛程式）；在 Claude Code 中，透過 `/plugin` 停用或解除安裝該外掛程式。Codex 會優先採用同名註冊而非外掛程式的 MCP，因此只要 Setup 的註冊存在，通常只會啟動一個，但 Claude Code 可能會同時啟動兩個。Setup 不會修改 Codex 或 Claude Code 的設定；若外掛程式仍處於啟用狀態，只會發出警告（啟動 Plumivox 時顯示的指引、`app-update.log`，以及 `app-update\setup-registration.json` 中的 `legacyPlugins`）。Toolport 的註冊同樣不會被自動改寫；若使用 Toolport，請依照[Toolport 一節](#透過-toolport-使用)自行檢查。

### 使用外掛程式安裝（macOS・傳統方式）

在開始提供 Setup 之前，macOS（Apple Silicon）使用此方式。Windows 也可以繼續使用。

從 GitHub 儲存庫 `main` 的最新提交進行安裝。不會參考使用者的開發用複製，也不會固定在標籤或提交 SHA。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` 是市集的識別碼，取得來源為上述的 GitHub URL。由於隨附 Windows x64 用的 `dist/plumivox.exe` 與 Apple Silicon Mac 用的 `dist/plumivox`（ARM64），使用者不需要安裝相依套件，也不需要手動新增 MCP 設定。

開發用 CI 會將在 Windows 與 macOS 上驗證過的成品儲存到 Cloudflare R2，並在準備發布時交付。[成品的儲存與取得步驟](docs/distribution.md)是給開發者參考的。外掛程式使用者不需要 R2 的設定或憑證，安裝與更新仍然從 GitHub 的 `main` 進行。

更新時，請取得 `main` 的最新狀態並安裝，然後重新啟動 Codex 以載入。

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

確認清單中 `installed` 顯示的版本號。已安裝的外掛程式在更新後不需再次 `add`，重新啟動 Codex 即可載入新的 MCP 與技能。

追蹤 GitHub 的註冊與更新方法，請參閱 [Codex 的外掛程式文件](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)。

### 透過 Toolport 使用

可以只將 MCP 註冊到 Toolport。Plumivox 不會自動改寫 Toolport 的註冊，因此請依照安裝方式新增或變更下列 stdio 伺服器。不要指定開發用複製或帶有版本號的安裝快取。

- 名稱：`plumivox`
- 命令：Windows 的 Setup 版為 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`（`%LOCALAPPDATA%` 需展開為實際路徑後指定。`current` 即使因更新而替換內容，路徑也不會改變）。外掛程式版請以 `codex plugin marketplace list` 確認 `plumivox-local` 的已取得根目錄，Windows 為 `<已取得根目錄>/dist/plumivox.exe`，macOS 為 `<已取得根目錄>/dist/plumivox`
- 引數：`--stdio`
- 工作目錄：使用者的主目錄
- 要求逾時：300000 毫秒（包含登入與中繼的準備時間）

在 Toolport 中為 Codex 使用的設定檔啟用，並以 `toolport_search_tools` 的 `server="plumivox"` 確認 11 個工具。搜尋並呼叫 `list_accounts`，確認設定能讀取後，再停用直接連線。Setup 版請以 `codex mcp remove plumivox` 與 `claude mcp remove plumivox --scope user` 移除直接註冊（完成一次註冊後，在以 Setup 重新安裝之前，不會自動重新註冊）。外掛程式版請在 Codex 的使用者設定 `config.toml` 中加入下列內容，並讓外掛程式本體保持啟用，以保留設定用技能。

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

重新啟動 Codex 後，直接連線即會停用。之後的操作是在 Toolport 中搜尋工具，並將回傳的實際工具名稱傳給 `toolport_call_tool`。API 金鑰、Cookie 與 SSH 金鑰會從 Plumivox 的既有設定讀取，不會移到 Toolport 的引數中。

外掛程式版的更新以上述的 `codex plugin marketplace upgrade plumivox-local` 進行。Toolport 執行中的伺服器會使用已載入的程式碼，因此更新後請將 Toolport 的 `plumivox` 停用再重新啟用，以載入新的執行檔。若已取得根目錄有所變更，也請變更已註冊的命令。啟動時不會進行 Git 取得或相依套件安裝。

## 初始設定

首次使用時，請前往[設定步驟](docs/setup.md)。隨附 Windows 與 macOS 的原生設定精靈，以及適用於 Linux x86_64、aarch64 的靜態 Rust VPS 安裝程式。其他 CPU 則保留 Bash 相容路徑。向 Codex 提出「請幫我設定 Plumivox」的請求，也可以透過隨附的 `setup-plumivox` 技能進行引導。

設定檔放在儲存庫之外。新使用者在 Windows 上使用 `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`，在 macOS 上使用 `~/Library/Application Support/Kagayoi/Plumivox/settings.json`。已有 `~/dev/Secret/secrets.json` 的使用者，會讀取該檔案的 `twitterapi_io`。vava／blbl 的 `release_posting` 與 `blog_posting` 也會維持。若要使用其他位置，請指定 `PLUMIVOX_SECRETS_FILE`。不需要將機密值傳入對話或 MCP 的引數。

若無法讀取所選的設定檔，會直接產生錯誤，而不會切換到其他設定。變更儲存位置後，請重新啟動 Codex。首次設定不會覆寫既有的金鑰、同名帳號或共用中繼設定。變更既有設定的方法請參閱[設定步驟](docs/setup.md)。

[首次設定](docs/setup.md) · [帳號與 Cookie 的設定](docs/configuration.md) · [以 Plumivox 管理 Proxy](docs/managed-proxy.md)

## 可用的工具

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

首次設定會將註冊的帳號儲存為預設值。若只要變更既有設定的預設值，請對隨附的執行檔指定 `--setup --set-default-account blog_posting`。詳細內容請參閱[設定步驟](docs/setup.md)與[設定的優先順序](docs/configuration.md)。

發文字數中，URL 計為 23 個字元，日文計為 2 個字元，Unicode 17 以前的複合表情符號每個計為 2 個字元。不同膚色的握手等，也會依照 [X 的加權字數規格](https://docs.x.com/fundamentals/counting-characters)進行判定。

例如可以提出「讀取 blog_posting 追蹤對象的公開貼文」「用 release_posting 以這段文字發文」這類請求。發文的執行依照使用者的請求進行。公開貼文的內文屬於外部資料，不會執行其中所含的指令。

## 追蹤中動態的取得範圍

TwitterAPI.io 沒有直接取得 X 官方首頁「追蹤中」的 API。Plumivox 預設會從追蹤清單開頭讀取最多 100 個帳號，並每 20 個帳號為一組搜尋公開貼文。`maxAccounts` 可設定為 20～200。首次為 100 時最多 6 次，為 200 時最多 11 次 API 要求。最新單價請確認 [TwitterAPI.io 的費用](https://twitterapi.io/pricing)。

結果中的 `officialHomeEquivalent` 一律為 `false`。無法取得受保護的貼文，且透過搜尋取得的內容、轉發的處理方式與整體排列順序，都不保證與官方首頁一致。當提供者還有後續資料，或唯一且未受保護的搜尋對象超過上限時，`coverage.followingHasNextPage` 會是 `true`。將 `coverage.followingNextCursor` 傳給 `followingCursor`，即可以下一段追蹤對象清單為對象。追蹤清單從最新的追蹤開始排列，無法避免因持續的追蹤變更而產生的取得差異。

將 `nextCursor` 傳給同一帳號 `get_following_feed` 的 `cursor`，即可取得各搜尋批次的後續內容。頁面內依時間排序，但由於各批次的頁面邊界，不保證跨頁的整體時間順序。追蹤對象會固定為首次取得的清單，在搜尋的後續中不會重新讀取。動態的 cursor 也會以 API 金鑰簽署，並拒絕遭竄改的搜尋對象。舊版未簽署的 cursor，以及變更 API 金鑰前的 cursor 無法使用，請移除 cursor 後重新取得。`cursor` 與 `followingCursor` 是不同的分頁方式，無法同時指定。

在[追蹤清單的官方回應規格](https://docs.twitterapi.io/api-reference/endpoint/get_user_followings)中，無法確認保護狀態的欄位。在同一份追蹤清單的回應中，會將有效的使用者名稱不區分大小寫地彙整，並依受保護、公開、不明的順序進行判定。排除已確認受保護的對象後，依首次出現的順序選取至上限，並將所選對象中無法判定的數量回傳於 `coverage.protectionStatusUnknownAccounts`。`protectedAccountsExcluded` 是該回應中已確認受保護的唯一帳號數。重複或無效的使用者名稱不計入，且會包含取得對象上限以外的受保護帳號，因此可能超過 `maxAccounts`。即使為 0，也不代表所有對象都是公開帳號。

舊實作所發出之已簽署 cursor 的後續，除了搜尋對象與件數外，也會沿用舊實作對未搜尋範圍與可否繼續的判定，因此可能會略過上限以外的對象。若要以新的保護狀態彙整、件數計算與繼續判定確實重新取得，請移除 cursor。

即使貼文或追蹤清單還有後續，若提供者的 cursor 或組合後的 cursor 超過長度上限，也無法繼續。已取得的貼文與 `hasNextPage` 會保留，`nextCursor` 為 `null`，`coverage.tweetContinuationAvailable` 為 `false`，`tweetContinuationUnavailableReason` 為 `cursor_too_long`。追蹤清單請確認 `followingNextCursor` 與 `followingContinuationAvailable`、`followingContinuationUnavailableReason`。cursor 不會被截斷，已取得的內容也不會被視為失敗而捨棄。

若提供者在一次回應中回傳超過 `maxAccounts` 的唯一未受保護對象，上限以外的對象就是未搜尋的。`followingHasNextPage` 為 `true`，`followingContinuationUnavailableReason` 為 `account_limit`，且不提供追蹤清單的繼續（`followingContinuationAvailable: false`、`followingNextCursor: null`）。這是為了防止透過提供者的下一個 cursor 略過未搜尋的對象。若 `maxAccounts` 小於 200，可在 200 以內的範圍提高上限，並在不帶 cursor 的情況下重新取得。若上限為 200 時提供者仍過量回傳，則無法以此方法取得該回應中未搜尋的對象。即使僅因同名重複列或受保護、無效名稱而超過上限，也不適用此限制。已選取對象的貼文後續可照常取得，且未搜尋對象存在的狀態也會沿用。

## 關鍵字與期間搜尋

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

## 發文與認證失敗時

成功時會回傳貼文 ID 與 X 的 URL。建立貼文時的通訊失敗或缺少貼文 ID，會以未確定的狀態回傳，不會自動重新傳送。若請求再次發布相同的文字，每次呼叫都會傳送一次。沒有依據歷史紀錄或內文雜湊而停止的機制。

一般 API 要求的預設通訊時間範圍為 45 秒，圖片上傳為每張圖片 120 秒。只有圖片上傳會在暫時性失敗時，對同一張圖片最多重試 1 次。對象為 HTTP 502、503、504（等待 250ms），以及 Proxy、上游的故障、原因不明的 JSON 拒絕、連線中斷（等待 1 秒）；認證、餘額、圖片不正確、重複、速率限制與逾時不會重試。等待與重試都會控制在單張圖片的通訊時間範圍內，不會重新傳送前一張圖片，也不會自動重新傳送建立貼文或登入。圖片失敗時不會送出貼文；若建立貼文後成敗不明，會回傳 `POST_UNCERTAIN`。

已知的發文處理錯誤會包含 `stage`（`input_validation`、`pre_send`、`image_upload`、`post_creation`）、表示是否嘗試過建立貼文要求的 `postAttempted`，以及已執行的圖片上傳要求總數 `uploadAttempts`。帳號或 Cookie 不足、中繼準備失敗屬於 `pre_send`。未知的內部錯誤與 MCP 輸入格式的拒絕不會回傳這些資訊。`postAttempted: true` 並不代表發文成功。不會回傳提供者的原始錯誤或憑證。

圖片通訊的 `API_UNAVAILABLE` 會包含 `failureKind`，以區分 `timeout`（逾時）、`network`（通訊故障）與 `cancelled`（取消）。在這些情況下也不會自動重新傳送圖片或貼文。

API 的拒絕錯誤（`API_REJECTED`、`API_HTTP_ERROR`）會包含 `httpStatus`、固定分類的陣列 `rejectionReasons`，以及分類來源 `rejectionReasonSource`。建立貼文的 5xx 變成 `POST_UNCERTAIN` 時，也會保留這些診斷資訊。分類來源為 `provider_message`（從提供者的 JSON 訊息推測的候選）或 `http_status`（依 HTTP 狀態碼的分類）。候選並不代表確定原因或允許重新傳送，無法判別的文字則為 `unknown`。不會回傳或儲存原始回應文字、任意錯誤碼或 request ID。

`provider_message` 的候選為 `api_key`、`authentication`、`account_locked`、`account_suspended`、`rate_limit`、`credits`、`proxy`、`media`、`duplicate`、`upstream_timeout`、`upstream_unavailable`、`unknown`。`http_status` 另外還有 `invalid_request`（400）、`unauthorized`（401）、`forbidden`（403）、`endpoint_not_found`（404）。例如圖片上傳時若為 `rejectionReasons: ["proxy", "upstream_timeout"]` 且 `postAttempted: false`，即可得知疑似是 Proxy 或上游逾時，並且在建立貼文前就已停止。不會讀取非 2xx 的內文，並維持既有的通訊上限與重試條件。

API 回應超過 10MiB 時，會中止接收並回傳失敗。發文的回應超過上限時，由於可能已經發布，也不會重新傳送。

已儲存 Cookie 的比對失敗只會停止發文。公開貼文的讀取與搜尋只使用 API 金鑰與使用者名稱，無須讀取已儲存的 Cookie 即可使用。已認證 Cookie 的取得與更新請參閱[設定步驟](docs/configuration.md)。

`login_account` 若已儲存的 Cookie 為可用格式就會重複使用，但不會回報已在 X 上驗證其有效性。若確認已失效或設定不一致，請呼叫 `refresh_session`。新的 Cookie 會以 Windows 的 DPAPI、macOS 的 Keychain 金鑰加密後儲存到 Plumivox 的儲存位置，且不會自動重新傳送失敗的發文。

在替換儲存內容前中止時回傳 `AUTH_CANCELLED`，無法維持認證更新的獨占時回傳 `AUTH_LOCK_LOST`，無法儲存的 Cookie 則回傳 `LOGIN_FAILED`。若在指示替換儲存內容後無法確認結果，會成為 `AUTH_SAVE_UNCERTAIN`，可能已經更新。`SESSION_STORAGE_UNAVAILABLE` 表示儲存位置，或 Windows 的 DPAPI、macOS 的 Keychain 等儲存處理失敗。

## 開發與驗證

開發、建置與測試也不需要 Node.js。使用 Rust/Cargo 的 `cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`。指定舊 Node 啟動器的註冊，請變更為上述的原生啟動方式。

Windows 的 `cargo xtask build` 會在以 Sigilune（Certum SimplySign 的雲端簽章）完成簽署與簽章驗證後，更新 `dist/plumivox.exe`。發布 CI 也會先以 Sigilune 簽署 Windows 成品再儲存。簽署所需的開發機設定與 CI 的 Secrets 請參閱[發布成品](docs/distribution.md#windows配布物のコード署名)。Windows 的 Setup 以 `cargo xtask velopack-pack` 封裝，並以 `cargo xtask test-setup-velopack-e2e` 在不接觸實際安裝的情況下進行隔離驗證。發布只由手動啟動的 `release-velopack.yml` 進行（[Setup（Velopack）的發布](docs/distribution.md#setupvelopackの配信)）。

請參閱[開發者用的建置與驗證步驟](docs/verification.md)、[作業規範](AGENTS.md)與[系統設計](DESIGN.md)。
