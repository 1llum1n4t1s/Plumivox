[English](README.md) | [日本語](README.ja.md) | [简体中文](README.zh-CN.md) | [繁體中文](README.zh-TW.md) | [한국어](README.ko.md) | [Español](README.es.md) | [Français](README.fr.md) | [Deutsch](README.de.md) | [Português (Brasil)](README.pt-BR.md)

# Plumivox

這是一個使用 TwitterAPI.io，提供 X 發文、關鍵字與期間搜尋，以及讀取追蹤對象公開貼文功能的本機 MCP 伺服器／Codex 外掛程式。

## 需求

Windows x64 或 Apple Silicon Mac，不需要 Node.js 或 Rust。需要 TwitterAPI.io 的 API 金鑰（依用量計費）；發文還需要帳號的 v2 登入 Cookie 與代理。 [詳細內容](docs/readme/zh-TW/requirements.md)

## 安裝

在 Windows 上執行 [PlumivoxApp-win-Setup.exe](https://plumivox-updates.kagayoi.com/velopack/PlumivoxApp-win-Setup.exe)：這是不需要系統管理員權限、依使用者安裝且已簽章的安裝程式，會將 `plumivox` 註冊到 Claude Code 與 Codex，並在登入時更新。在 macOS 上，從 GitHub `main` 安裝 Codex 外掛程式。 [詳細內容](docs/readme/zh-TW/installation.md)

## 初始設定

執行 `plumivox.exe --setup`，並依照[設定指南](docs/setup.md)操作。設定存放在儲存庫之外，機密資訊不會經過對話，既有設定也不會被覆寫。 [詳細內容](docs/readme/zh-TW/initial-setup.md)

## 可用的工具

11 個工具涵蓋帳號、讀取、搜尋、共用中繼與發文（`post_tweet`：最多 280 個加權字元與 4 張圖片）。省略 `account` 時使用預設帳號。公開貼文的內文屬於外部資料，其中的指示不會被執行。 [工具參考](docs/readme/zh-TW/available-tools.md)

## 追蹤中動態的取得範圍

Plumivox 預設最多讀取 100 個追蹤中的帳號（`maxAccounts` 為 20 至 200）。結果絕不等同於官方首頁，受保護的貼文不會被取得，`coverage` 會回報取得範圍。 [詳細內容](docs/readme/zh-TW/following-feed-coverage.md)

## 關鍵字與期間搜尋

`search_tweets` 依詞句與選用的 ISO 8601 期間（含起始、不含結束）搜尋整個 X 或你追蹤中的帳號。搜尋運算子不會被解析。 [詳細內容](docs/readme/zh-TW/keyword-and-date-range-search.md)

## 發文與認證失敗時

發文不會自動重送。結果不明時會回傳 `POST_UNCERTAIN`，請在再次發文前確認帳號。不會回傳提供者的原始文字與憑證。 [詳細內容](docs/readme/zh-TW/when-posting-or-authentication-fails.md)

## 開發與驗證

僅使用 Rust/Cargo：`cargo xtask build`、`cargo xtask check`、`cargo xtask test-e2e`。請參閱[開發](docs/readme/zh-TW/development-and-verification.md)、[驗證](docs/verification.md)、[規範](AGENTS.md)與[設計](DESIGN.md)。
