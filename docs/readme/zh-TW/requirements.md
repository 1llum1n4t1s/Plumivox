# 需求

- Windows x64 或 Apple Silicon Mac（macOS ARM64）。MCP 本體是以 Rust 打造的原生執行檔，不需要安裝 Node.js 或 Rust。
- Windows 與 macOS 的初始設定使用隨附的執行檔，不需要 PowerShell 7 (`pwsh`)。
- TwitterAPI.io 的 API 金鑰。使用 API 會產生該服務的按量計費。
- 發文需要目標帳號的 v2 登入 Cookie 與 Proxy。除了既有的外部 Proxy，在 Windows 與 macOS 上還可讓多個聊天共用 Plumivox 內的住宅網路出口與 SSH 中繼。公開中繼伺服器需要進行首次準備。
