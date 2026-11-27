# 安裝

## Windows：使用 Setup 安裝（建議）

1. 下載並執行 [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)。這是以使用者為單位的安裝，不需要系統管理員權限。Setup 附有作者本人的程式碼簽章（`Open Source Developer Yuichiro Shinozaki`）。
2. 程式會安裝到 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`，若 PATH 上有 `claude`、`codex` 的 CLI，就會自動向 Claude Code（使用者層級）與 Codex 註冊 MCP 伺服器 `plumivox`（引數 `--stdio`）。若找不到 CLI，可在之後安裝 CLI，並於下次登入或從開始功能表啟動 Plumivox 時完成註冊。
3. 依照安裝完成後畫面上的說明，在終端機執行 `"%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe" --setup`，進行[初始設定](../../../README.zh-TW.md#初始設定)。Claude Code 與 Codex 可從新的工作階段開始使用。

更新會自動進行。MCP 執行期間會從 R2（`https://plumivox-updates.kagayoi.com/velopack/`）取得新版本，並在登入電腦時，僅於 Plumivox（MCP、共用中繼）未執行時套用。若正在執行，則延到下次登入，不會強制結束。套用前會確認簽署者，若不符則不套用。若要停用自動更新，請將 `%LOCALAPPDATA%\Kagayoi\Plumivox\app-update\app-update.json` 設為 `{"auto_update": false}`。運作紀錄位於同一資料夾的 `app-update.log`。

解除安裝時，請在「設定」→「應用程式」的清單中選擇 Plumivox。只會移除指向自身的 Claude Code 與 Codex `plumivox` 註冊，以及登入時更新的註冊，並保留設定與工作階段的資料夾（`%LOCALAPPDATA%\Kagayoi\Plumivox`）。Toolport 中的 `plumivox` 項目會保留，請自行在 Toolport 中刪除。若要先只移除註冊，請以 `plumivox.exe --uninstall-setup` 確認預定內容，再以 `--uninstall-setup --apply` 執行。

`plumivox.exe` 的其他命令列選項：`--setup`（初始設定）、`--uninstall-setup`（移除註冊）、`--rejection-log on|off|status|show|clear`（經遮蔽處理的拒絕文字的限時本機記錄，預設關閉；請參閱[本機拒絕記錄](when-posting-or-authentication-fails.md#本機拒絕記錄)）。解除安裝後記錄仍會保留，以 `--rejection-log clear` 刪除。

從外掛程式（如下）轉移時：Setup 會將指向舊外掛程式 `dist` 等位置的同名 `plumivox` 註冊，替換為指向安裝位置的註冊。外掛程式本身所帶的 MCP（`plumivox@plumivox-local`）是另一個註冊，為避免 Plumivox 經由兩條途徑啟動，請將其停用或移除該外掛程式。在 Codex 中，加入與[Toolport 一節](#透過-toolport-使用)相同的 `enabled = false`（或移除外掛程式）；在 Claude Code 中，透過 `/plugin` 停用或解除安裝該外掛程式。Codex 會優先採用同名註冊而非外掛程式的 MCP，因此只要 Setup 的註冊存在，通常只會啟動一個，但 Claude Code 可能會同時啟動兩個。Setup 不會修改 Codex 或 Claude Code 的設定；若外掛程式仍處於啟用狀態，只會發出警告（啟動 Plumivox 時顯示的指引、`app-update.log`，以及 `app-update\setup-registration.json` 中的 `legacyPlugins`）。Toolport 的註冊同樣不會被自動改寫；若使用 Toolport，請依照[Toolport 一節](#透過-toolport-使用)自行檢查。轉移到 Setup 時，請將 Toolport 中 `plumivox` 的命令改為 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`（展開為實際路徑）。請勿將執行檔複製到其他位置，副本不會被更新。即使 Toolport 以另一個執行檔提供 `plumivox`，Setup 也會略過直接註冊並發出警告（指引與 `app-update.log`，不記錄 Toolport 端的命令）。將命令改指向上述路徑後，每次啟動時的判定會使警告消失。

## 使用外掛程式安裝（macOS・傳統方式）

在開始提供 Setup 之前，macOS（Apple Silicon）使用此方式。Windows 也可以繼續使用。

從 GitHub 儲存庫 `main` 的最新提交進行安裝。不會參考使用者的開發用複製，也不會固定在標籤或提交 SHA。

```powershell
codex plugin marketplace add https://github.com/1llum1n4t1s/Plumivox.git --ref main
codex plugin add plumivox@plumivox-local
```

`plumivox-local` 是市集的識別碼，取得來源為上述的 GitHub URL。由於隨附 Windows x64 用的 `dist/plumivox.exe` 與 Apple Silicon Mac 用的 `dist/plumivox`（ARM64），使用者不需要安裝相依套件，也不需要手動新增 MCP 設定。

開發用 CI 會將在 Windows 與 macOS 上驗證過的成品儲存到 Cloudflare R2，並在準備發布時交付。[成品的儲存與取得步驟](../../distribution.md)是給開發者參考的。外掛程式使用者不需要 R2 的設定或憑證，安裝與更新仍然從 GitHub 的 `main` 進行。

更新時，請取得 `main` 的最新狀態並安裝，然後重新啟動 Codex 以載入。

```powershell
codex plugin marketplace upgrade plumivox-local
codex plugin list --marketplace plumivox-local --json
```

確認清單中 `installed` 顯示的版本號。已安裝的外掛程式在更新後不需再次 `add`，重新啟動 Codex 即可載入新的 MCP 與技能。

追蹤 GitHub 的註冊與更新方法，請參閱 [Codex 的外掛程式文件](https://developers.openai.com/plugins/build/plugins#add-a-marketplace-from-the-cli)。

## 透過 Toolport 使用

可以只將 MCP 註冊到 Toolport。Plumivox 不會自動改寫 Toolport 的註冊，因此請依照安裝方式新增或變更下列 stdio 伺服器。不要指定開發用複製或帶有版本號的安裝快取。

- 名稱：`plumivox`
- 命令：Windows 的 Setup 版為 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`（`%LOCALAPPDATA%` 需展開為實際路徑後指定。`current` 即使因更新而替換內容，路徑也不會改變）。外掛程式版請以 `codex plugin marketplace list` 確認 `plumivox-local` 的已取得根目錄，Windows 為 `<已取得根目錄>/dist/plumivox.exe`，macOS 為 `<已取得根目錄>/dist/plumivox`
- 引數：`--stdio`
- 工作目錄：使用者的主目錄
- 要求逾時：300000 毫秒（包含登入與中繼的準備時間）

在 Toolport 中為 Codex 使用的設定檔啟用，並以 `toolport_search_tools` 的 `server="plumivox"` 確認 11 個工具。搜尋並呼叫 `list_accounts`，確認設定能讀取後，再停用直接連線。Setup 版若用戶端已使用 Toolport 閘道，且所用設定檔中已啟用 `plumivox`，會自動略過直接註冊（由 `app-update.json` 的 `mcp_registration` 控制：預設 `auto`，另有 `direct`、`off`；詳見 [Toolport configuration](../configuration.md)）。若是之後才加入 Toolport，請自行以 `codex mcp remove plumivox` 與 `claude mcp remove plumivox --scope user` 移除既有的直接註冊；Setup 不會自行移除（註冊完成後，在你以 Setup 重新安裝之前不會自動重新註冊；之後只會補上 Setup 當時略過的用戶端，例如 Toolport 閘道之後從用戶端中消失時，會在下次啟動 Plumivox 或登入時直接註冊）。判定與作業系統無關，在 Windows、macOS 與 Linux 上依相同規則進行，其他作業系統不做判定，一律直接註冊（Setup 本身目前只在 Windows 上提供）。若 Toolport 以另一個執行檔（舊外掛程式的 `dist` 或副本等）提供 `plumivox`，同樣會略過直接註冊，記錄為 `toolport-foreign` 並發出警告。請將 Toolport 中 `plumivox` 的命令改指向 `%LOCALAPPDATA%\PlumivoxApp\current\plumivox.exe`。每次啟動時都會重新判定。外掛程式版請在 Codex 的使用者設定 `config.toml` 中加入下列內容，並讓外掛程式本體保持啟用，以保留設定用技能。

```toml
[plugins."plumivox@plumivox-local".mcp_servers.plumivox]
enabled = false
```

重新啟動 Codex 後，直接連線即會停用。之後的操作是在 Toolport 中搜尋工具，並將回傳的實際工具名稱傳給 `toolport_call_tool`。API 金鑰、Cookie 與 SSH 金鑰會從 Plumivox 的既有設定讀取，不會移到 Toolport 的引數中。

更新外掛程式版時，為避免執行檔被鎖定而無法取代，請先在 Toolport 中停用 `plumivox`，接著執行上述的 `codex plugin marketplace upgrade plumivox-local`，最後重新啟用，以載入新的執行檔。若已取得根目錄有所變更，也請變更已註冊的命令。啟動時不會進行 Git 取得或相依套件安裝。
