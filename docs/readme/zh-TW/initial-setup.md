# 初始設定

首次使用時，請前往[設定步驟](../../setup.md)。隨附 Windows 與 macOS 的原生設定精靈，以及適用於 Linux x86_64、aarch64 的靜態 Rust VPS 安裝程式。其他 CPU 則保留 Bash 相容路徑。向 Codex 提出「請幫我設定 Plumivox」的請求，也可以透過隨附的 `setup-plumivox` 技能進行引導。

設定檔放在儲存庫之外。新使用者在 Windows 上使用 `%LOCALAPPDATA%\Kagayoi\Plumivox\settings.json`，在 macOS 上使用 `~/Library/Application Support/Kagayoi/Plumivox/settings.json`。已有 `~/dev/Secret/secrets.json` 的使用者，會讀取該檔案的 `twitterapi_io`。vava／blbl 的 `release_posting` 與 `blog_posting` 也會維持。若要使用其他位置，請指定 `PLUMIVOX_SECRETS_FILE`。不需要將機密值傳入對話或 MCP 的引數。

若無法讀取所選的設定檔，會直接產生錯誤，而不會切換到其他設定。變更儲存位置後，請重新啟動 Codex。首次設定不會覆寫既有的金鑰、同名帳號或共用中繼設定。變更既有設定的方法請參閱[設定步驟](../../setup.md)。

[首次設定](../../setup.md) · [帳號與 Cookie 的設定](../../configuration.md) · [以 Plumivox 管理 Proxy](../../managed-proxy.md)
